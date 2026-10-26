use super::tool_control::{error, rejection};
use crate::infrastructure::tool_runtime::{
    ToolRuntime,
    connections::{AddConnectionInput, ChangeConnectionInput, ConnectionChange, VerifyInput},
};
use axum::{
    Json,
    body::to_bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub async fn create(State(runtime): State<Arc<ToolRuntime>>, request: Request) -> Response {
    let bytes = match body(request).await {
        Ok(v) => v,
        Err((status, code)) => return rejection(status, code),
    };
    let input: AddConnectionInput = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
    };
    match runtime.add_connection(input).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
pub async fn inspect(
    State(runtime): State<Arc<ToolRuntime>>,
    path: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
) -> Response {
    let Ok(Path(id)) = path else {
        return rejection(StatusCode::BAD_REQUEST, "invalid_input");
    };
    match runtime.added_connection(id).await {
        Ok(Some(v)) => Json(serde_json::json!({"status":"committed","receipt":v})).into_response(),
        Ok(None) => {
            Json(serde_json::json!({"status":"unconfirmed","receipt":null})).into_response()
        }
        Err(e) => error(e),
    }
}
pub async fn verify(State(runtime): State<Arc<ToolRuntime>>, request: Request) -> Response {
    let bytes = match body(request).await {
        Ok(v) => v,
        Err((status, code)) => return rejection(status, code),
    };
    let input: VerifyInput = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
    };
    match runtime.verify_connection(input).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
async fn body(request: Request) -> Result<axum::body::Bytes, (StatusCode, &'static str)> {
    if request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next().unwrap_or("").trim() != "application/json")
    {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required"));
    }
    match tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 32 * 1024),
    )
    .await
    {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) => Err((StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")),
        Err(_) => Err((StatusCode::REQUEST_TIMEOUT, "request_timeout")),
    }
}

pub async fn change(
    State(runtime): State<Arc<ToolRuntime>>,
    path: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
    request: Request,
) -> Response {
    let Ok(Path(id)) = path else {
        return rejection(StatusCode::BAD_REQUEST, "invalid_input");
    };
    let deleting = request.method() == axum::http::Method::DELETE;
    let bytes = match body(request).await {
        Ok(v) => v,
        Err((status, code)) => return rejection(status, code),
    };
    let input: ChangeConnectionInput = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
    };
    if deleting != matches!(input.change, ConnectionChange::Delete) {
        return rejection(StatusCode::BAD_REQUEST, "invalid_input");
    }
    match runtime.change_connection(id, input).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
