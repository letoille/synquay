use super::{error, middleware::RequestContext};
use crate::infrastructure::pipeline::webhook::{
    WebhookAuth, WebhookFormat, fresh_timestamp, valid_event_id,
};
use crate::{
    AppState,
    application::ports::PortError,
    domain::event::{EventPayload, IncomingEvent, IngestDisposition},
};
use axum::{
    Extension, Json,
    body::to_bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebhookEvent {
    source_event_id: String,
    event_type: String,
    payload: Value,
    content: Option<crate::domain::content::ContentRef>,
}

pub async fn receive(
    State(state): State<AppState>,
    Path(connection): Path<String>,
    Extension(context): Extension<RequestContext>,
    request: Request,
) -> Response {
    let Some(pipeline) = state.pipeline else {
        return error::response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Webhook is not configured.",
            context,
        );
    };
    let Ok(connection) = Uuid::parse_str(&connection) else {
        return error::response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Webhook is not configured.",
            context,
        );
    };
    let configuration = pipeline.connection_io.clone().read_owned().await;
    let (policy, secret) = match pipeline.webhook_settings(connection) {
        Ok(v) => v,
        Err(_) => {
            return error::response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Webhook is not configured.",
                context,
            );
        }
    };
    let headers = request.headers();
    let unique = |name: &str| -> Option<&str> {
        if headers.get_all(name).iter().count() != 1 {
            return None;
        }
        headers.get(name)?.to_str().ok()
    };
    let raw_id = match &policy.format {
        WebhookFormat::Envelope => String::new(),
        WebhookFormat::Json { id_header, .. } => {
            match unique(id_header).filter(|s| valid_event_id(s, 256)) {
                Some(id) => id.to_owned(),
                None => {
                    return error::response(
                        StatusCode::BAD_REQUEST,
                        "invalid_delivery_id",
                        "A unique configured event ID header is required.",
                        context,
                    );
                }
            }
        }
    };
    let timestamp = unique("x-synquay-timestamp").unwrap_or("").to_owned();
    let signature = unique("x-synquay-signature").unwrap_or("").to_owned();
    let authenticated = match policy.authentication {
        WebhookAuth::Bearer => {
            use subtle::ConstantTimeEq;
            unique("authorization")
                .and_then(|v| v.strip_prefix("Bearer "))
                .is_some_and(|v| bool::from(v.as_bytes().ct_eq(secret.as_bytes())))
        }
        WebhookAuth::HmacSha256 => {
            fresh_timestamp(&timestamp, std::time::SystemTime::now()) && signature.len() == 67
        }
    };
    if headers.contains_key("origin") || !authenticated {
        return error::response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Valid webhook credentials are required.",
            context,
        );
    }
    if headers.contains_key("content-encoding")
        || unique("content-type").is_none_or(|v| {
            !v.split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return error::response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Use one application/json Content-Type header and an unencoded body.",
            context,
        );
    }
    let _admission = match pipeline.try_admit_webhook() {
        Ok(permit) => permit,
        Err(_) => {
            let mut response = error::response(
                StatusCode::SERVICE_UNAVAILABLE,
                "ingress_busy",
                "Ingress is busy. Retry with the same source_event_id.",
                context,
            );
            response
                .headers_mut()
                .insert("retry-after", "1".parse().unwrap());
            return response;
        }
    };
    let bytes = match tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 64 * 1024),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => {
            return error::response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "invalid_body",
                "Request body cannot be read within the size limit.",
                context,
            );
        }
        Err(_) => {
            return error::response(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout",
                "Request body timed out.",
                context,
            );
        }
    };
    if policy.authentication == WebhookAuth::HmacSha256
        && !crate::infrastructure::pipeline::webhook::verify_signature(
            secret.as_bytes(),
            connection,
            &timestamp,
            &raw_id,
            &signature,
            &bytes,
            std::time::SystemTime::now(),
        )
    {
        return error::response(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "Webhook signature is invalid or expired.",
            context,
        );
    }
    let parsed = match &policy.format {
        WebhookFormat::Envelope => serde_json::from_slice::<WebhookEvent>(&bytes),
        WebhookFormat::Json { event_type, .. } => {
            serde_json::from_slice::<Value>(&bytes).map(|payload| WebhookEvent {
                source_event_id: raw_id,
                event_type: event_type.clone(),
                payload,
                content: None,
            })
        }
    };
    let input = match parsed {
        Ok(value) => value,
        Err(_) => {
            return error::response(
                StatusCode::BAD_REQUEST,
                "invalid_event",
                "A valid JSON event is required.",
                context,
            );
        }
    };
    // The parsed DTO owns its fields; release the wire buffer before storage waits.
    drop(bytes);
    if !valid_event_id(&input.source_event_id, 256) || !valid_event_id(&input.event_type, 128) {
        return error::response(
            StatusCode::BAD_REQUEST,
            "invalid_event",
            "Event identifiers are missing or too long.",
            context,
        );
    }
    if input.content.is_some() && !input.payload.is_null() {
        return error::response(
            StatusCode::BAD_REQUEST,
            "ambiguous_payload",
            "Use payload:null with a content reference.",
            context,
        );
    }
    if input.content.is_some()
        && (pipeline.content.is_none() || connection != pipeline.source.connection_id)
    {
        return error::response(
            StatusCode::BAD_REQUEST,
            "content_disabled",
            "Content transfer is not configured.",
            context,
        );
    }
    let payload = match input.content {
        Some(r) => EventPayload::Content(r),
        None => EventPayload::Json(input.payload),
    };
    let request_id = Uuid::parse_str(&context.request_id).expect("server generated UUID");
    let result = tokio::spawn(async move {
        let _configuration = configuration;
        pipeline
            .accept_from(
                connection,
                request_id,
                IncomingEvent {
                    route_context: None,
                    source_event_id: Some(input.source_event_id),
                    event_type: input.event_type,
                    schema_version: 1,
                    occurred_at: None,
                    payload,
                },
            )
            .await
    })
    .await
    .unwrap_or(Err(PortError::OutcomeUnknown));
    match result {
        Ok(receipt)=>(StatusCode::ACCEPTED,Json(json!({"event_id":receipt.event_id,"disposition":match receipt.disposition {IngestDisposition::Stored=>"stored",IngestDisposition::Duplicate=>"duplicate"}}))).into_response(),
        Err(PortError::InvalidInput{..}|PortError::NotFound|PortError::ScopeMismatch)=>error::response(StatusCode::BAD_REQUEST,"invalid_event","The event is invalid.",context),
        Err(PortError::Conflict)=>error::response(StatusCode::CONFLICT,"event_id_conflict","The event ID was already used with different content, or its legacy content cannot be verified.",context),
        Err(PortError::PayloadTooLarge)=>error::response(StatusCode::PAYLOAD_TOO_LARGE,"event_too_large","The encoded event exceeds the storage limit.",context),
        Err(PortError::CapacityExceeded{reason})=>{
            tracing::warn!(event="pipeline.ingress_paused",reason);
            let mut response=error::response(StatusCode::SERVICE_UNAVAILABLE,"capacity_exceeded","Capacity is unavailable. Retry with the same source_event_id.",context);
            response.headers_mut().insert("retry-after","10".parse().unwrap());
            response
        },
        Err(_)=>error::response(StatusCode::SERVICE_UNAVAILABLE,"persistence_unconfirmed","Persistence was not confirmed. Retry with the same source_event_id.",context),
    }
}
