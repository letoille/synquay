use super::tool_control::{error, rejection};
use crate::{domain::workspace::WorkspaceUpdate, infrastructure::tool_runtime::ToolRuntime};
use axum::{
    Json,
    body::to_bytes,
    extract::{Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub async fn view(State(runtime): State<Arc<ToolRuntime>>) -> Response {
    match runtime.workspace_view().await {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberQuery {
    after: Option<Uuid>,
    limit: Option<u32>,
}
pub async fn members(
    State(runtime): State<Arc<ToolRuntime>>,
    query: Result<Query<MemberQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return rejection(StatusCode::BAD_REQUEST, "invalid_query");
    };
    match runtime
        .workspace_members(q.after, q.limit.unwrap_or(50))
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
pub async fn update(State(runtime): State<Arc<ToolRuntime>>, request: Request) -> Response {
    if request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next().unwrap_or("").trim() != "application/json")
    {
        return rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let request_id = request
        .extensions()
        .get::<super::middleware::RequestContext>()
        .and_then(|ctx| Uuid::parse_str(&ctx.request_id).ok())
        .unwrap_or_else(Uuid::new_v4);
    let bytes =
        match tokio::time::timeout(Duration::from_secs(5), to_bytes(request.into_body(), 4096))
            .await
        {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => return rejection(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            Err(_) => return rejection(StatusCode::REQUEST_TIMEOUT, "request_timeout"),
        };
    let input: WorkspaceUpdate = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
    };
    match runtime.update_workspace(request_id, input).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => error(e),
    }
}
