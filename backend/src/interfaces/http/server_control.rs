use super::tool_control::{error, rejection};
use crate::{
    application::ports::PortError,
    domain::mcp_server::{ServerChange, ServerDeletion},
    infrastructure::tool_runtime::ToolRuntime,
};
use axum::{
    Json,
    body::to_bytes,
    extract::{Path, Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub async fn list(State(runtime): State<Arc<ToolRuntime>>) -> Response {
    match runtime.list_servers().await {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
pub async fn item(
    State(runtime): State<Arc<ToolRuntime>>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Response {
    if request.method() == Method::GET {
        return match runtime.server_view(id).await {
            Ok(v) => Json(v).into_response(),
            Err(e) => error(e),
        };
    }
    if !request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next().unwrap_or("").trim() == "application/json")
    {
        return rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let delete = request.method() == Method::DELETE;
    let bytes = match tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 32 * 1024),
    )
    .await
    {
        Ok(Ok(b)) => b,
        Ok(Err(_)) => return rejection(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
        Err(_) => return rejection(StatusCode::REQUEST_TIMEOUT, "request_timeout"),
    };
    let result = if delete {
        match serde_json::from_slice::<ServerDeletion>(&bytes) {
            Ok(input) => runtime.delete_server(id, input).await,
            Err(_) => Err(PortError::InvalidInput {
                code: "invalid_mcp_server",
            }),
        }
    } else {
        match serde_json::from_slice::<ServerChange>(&bytes) {
            Ok(input) => runtime.save_server(id, input).await,
            Err(_) => Err(PortError::InvalidInput {
                code: "invalid_mcp_server",
            }),
        }
    };
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
