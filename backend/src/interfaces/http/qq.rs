//! QQ's protocol is deliberately separate from generic Webhook authentication.
use super::tool_control::rejection;
use crate::{
    AppState,
    application::ports::PortError,
    infrastructure::connectors::qq::{Callback, callback},
};
use axum::{
    Json,
    body::to_bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

pub async fn receive(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Response {
    let Some(p) = state.pipeline else {
        return rejection(StatusCode::NOT_FOUND, "not_found");
    };
    let configuration = p.connection_io.clone().read_owned().await;
    let c = match p.qq_settings(id) {
        Ok(c) => c,
        Err(_) => return rejection(StatusCode::NOT_FOUND, "not_found"),
    };
    let headers = request.headers();
    let one = |key: &str| -> Option<String> {
        if headers.get_all(key).iter().count() != 1 {
            return None;
        }
        headers.get(key)?.to_str().ok().map(str::to_owned)
    };
    let app = one("x-bot-appid").unwrap_or_default();
    let ts = one("x-signature-timestamp").unwrap_or_default();
    let sig = one("x-signature-ed25519").unwrap_or_default();
    if headers.contains_key("origin") || app != c.app_id {
        return rejection(StatusCode::UNAUTHORIZED, "invalid_qq_identity");
    }
    if headers.contains_key("content-encoding")
        || one("content-type").is_none_or(|s| {
            !s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let _permit = match p.try_admit_webhook() {
        Ok(v) => v,
        Err(_) => return rejection(StatusCode::SERVICE_UNAVAILABLE, "ingress_busy"),
    };
    let bytes = match tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 64 * 1024),
    )
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => return rejection(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large"),
        Err(_) => return rejection(StatusCode::REQUEST_TIMEOUT, "request_timeout"),
    };
    let event = match callback(&c, &app, &ts, &sig, &bytes, SystemTime::now()) {
        Ok(Callback::Challenge(v)) => return Json(v).into_response(),
        Ok(Callback::Ignore) => return Json(json!({"op":12})).into_response(),
        Ok(Callback::Event(e)) => *e,
        Err(PortError::PermissionDenied) => {
            return rejection(StatusCode::UNAUTHORIZED, "invalid_qq_signature");
        }
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_qq_callback"),
    };
    drop(bytes);
    // Complete an admitted durable write even if the peer disconnects. QQ gets an
    // ACK only after persistence; its retries reuse the upstream message identity.
    let result = tokio::spawn(async move {
        let _configuration = configuration;
        let _permit = _permit;
        p.accept_from(id, Uuid::new_v4(), event).await
    })
    .await
    .unwrap_or(Err(PortError::OutcomeUnknown));
    match result {
        Ok(_) => Json(json!({"op":12})).into_response(),
        Err(PortError::Conflict) => rejection(StatusCode::CONFLICT, "event_id_conflict"),
        Err(PortError::InvalidInput { .. } | PortError::ScopeMismatch) => {
            rejection(StatusCode::BAD_REQUEST, "invalid_event")
        }
        Err(_) => rejection(StatusCode::SERVICE_UNAVAILABLE, "persistence_unconfirmed"),
    }
}
