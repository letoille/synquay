use super::tool_control::{error, guard, rejection};
use crate::{AppState, application::ports::PortError, domain::managed_route::RouteInput};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Query, Request, State, rejection::QueryRejection},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use uuid::Uuid;

pub(super) fn router(state: AppState) -> Router {
    let runtime = state
        .tools
        .clone()
        .expect("route control requires tools runtime");
    Router::new()
        .route("/api/routes", get(view).post(change))
        .route("/api/routes/operations/{id}", get(operation))
        .layer(middleware::from_fn_with_state(runtime, guard))
        .with_state(state)
}
pub(super) async fn access(state: &AppState, write: bool) -> Result<Uuid, PortError> {
    let access = state
        .tools
        .as_ref()
        .ok_or(PortError::Unsupported)?
        .workspace_admin(write)
        .await?;
    if let Some(p) = &state.pipeline
        && p.source.workspace_id != access.workspace.id
    {
        return Err(PortError::PermissionDenied);
    }
    Ok(access.member.principal_id)
}
async fn view(
    State(state): State<AppState>,
    query: Result<Query<crate::infrastructure::pipeline::routes::RoutesQuery>, QueryRejection>,
) -> Response {
    if let Err(e) = access(&state, false).await {
        return error(e);
    }
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_route_query"),
    };
    match &state.pipeline {
        Some(p) => match p.routes_page(query).await {
            Ok(view) => Json(view).into_response(),
            Err(PortError::InvalidInput {
                code: "invalid_route_query",
            }) => rejection(StatusCode::BAD_REQUEST, "invalid_route_query"),
            Err(e) => error(e),
        },
        None => Json(serde_json::json!({"available":false,"reason":"pipeline_not_configured"}))
            .into_response(),
    }
}
async fn change(State(state): State<AppState>, request: Request) -> Response {
    let actor = match access(&state, true).await {
        Ok(actor) => actor,
        Err(e) => return error(e),
    };
    let Some(p) = state.pipeline else {
        return error(PortError::Unsupported);
    };
    if request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next().unwrap_or("").trim() != "application/json")
    {
        return rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let bytes = match to_bytes(request.into_body(), 16 * 1024).await {
        Ok(b) => b,
        Err(_) => return error(PortError::PayloadTooLarge),
    };
    let input: RouteInput = match serde_json::from_slice(&bytes) {
        Ok(i) => i,
        Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
    };
    match p.change_route(actor, input).await {
        Ok(receipt) => Json(receipt).into_response(),
        Err(PortError::InvalidInput { code }) => rejection(StatusCode::BAD_REQUEST, code),
        Err(e) => error(e),
    }
}
async fn operation(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    let actor = match access(&state, false).await {
        Ok(actor) => actor,
        Err(e) => return error(e),
    };
    let Some(p) = state.pipeline else {
        return error(PortError::Unsupported);
    };
    match p.route_operation(actor, id).await {
        Ok(receipt) => Json(serde_json::json!({"receipt":receipt})).into_response(),
        Err(e) => error(e),
    }
}
