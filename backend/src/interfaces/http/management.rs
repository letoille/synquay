use crate::{AppState, application::ports::PortError, domain::recovery::RecoveryCommand};
use axum::{
    Json,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::time::Duration;
fn failure(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error":code}))).into_response()
}
pub async fn handle(State(state): State<AppState>, request: Request) -> Response {
    let Some(p) = state.pipeline else {
        return failure(StatusCode::NOT_FOUND, "not_configured");
    };
    let Some(principal) = p.authenticate_operator(
        request
            .headers()
            .get("authorization")
            .and_then(|x| x.to_str().ok()),
    ) else {
        return failure(StatusCode::UNAUTHORIZED, "operator_credentials_required");
    };
    let Ok(_slot) = p.try_admit_admin() else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "management_busy");
    };
    if request.method() == axum::http::Method::GET {
        return match p.operations(principal).await {
            Ok(v) => Json(v).into_response(),
            Err(_) => failure(StatusCode::SERVICE_UNAVAILABLE, "snapshot_unavailable"),
        };
    }
    if request
        .headers()
        .get("content-type")
        .and_then(|x| x.to_str().ok())
        .is_none_or(|x| {
            !x.split(';')
                .next()
                .is_some_and(|s| s.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return failure(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let bytes =
        match tokio::time::timeout(Duration::from_secs(5), to_bytes(request.into_body(), 8192))
            .await
        {
            Ok(Ok(b)) => b,
            Ok(Err(_)) => return failure(StatusCode::PAYLOAD_TOO_LARGE, "body_limit"),
            Err(_) => return failure(StatusCode::REQUEST_TIMEOUT, "body_timeout"),
        };
    let command: RecoveryCommand = match serde_json::from_slice(&bytes) {
        Ok(c) => c,
        Err(_) => return failure(StatusCode::BAD_REQUEST, "invalid_recovery_command"),
    };
    drop(bytes);
    match p.recover(principal, command).await {
        Ok(r) => Json(r).into_response(),
        Err(PortError::Conflict) => failure(StatusCode::CONFLICT, "state_or_command_conflict"),
        Err(PortError::NotFound) => failure(StatusCode::NOT_FOUND, "not_found"),
        Err(PortError::InvalidInput { code }) => failure(StatusCode::BAD_REQUEST, code),
        Err(PortError::PermissionDenied | PortError::ScopeMismatch) => {
            failure(StatusCode::FORBIDDEN, "forbidden")
        }
        Err(_) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "result_unconfirmed_reuse_command_id",
        ),
    }
}

pub async fn inspect(
    State(state): State<AppState>,
    axum::extract::Path((kind, id)): axum::extract::Path<(String, String)>,
    request: Request,
) -> Response {
    let Some(p) = state.pipeline else {
        return failure(StatusCode::NOT_FOUND, "not_configured");
    };
    let Some(principal) = p.authenticate_operator(
        request
            .headers()
            .get("authorization")
            .and_then(|x| x.to_str().ok()),
    ) else {
        return failure(StatusCode::UNAUTHORIZED, "operator_credentials_required");
    };
    let Ok(_slot) = p.try_admit_admin() else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "management_busy");
    };
    let Ok(id) = uuid::Uuid::parse_str(&id) else {
        return failure(StatusCode::BAD_REQUEST, "invalid_id");
    };
    match p.inspect_recovery(principal, &kind, id).await {
        Ok(v) => Json(v).into_response(),
        Err(PortError::NotFound) => failure(StatusCode::NOT_FOUND, "not_found"),
        Err(_) => failure(StatusCode::SERVICE_UNAVAILABLE, "snapshot_unavailable"),
    }
}
