//! Console configuration APIs. Login is temporarily absent; origin, resource limits,
//! workspace invariants and audit scoping remain enforced. MCP auth is independent.
use crate::{
    application::ports::{
        PortError,
        publications::{DraftInput, PublishInput},
    },
    infrastructure::tool_runtime::ToolRuntime,
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub(super) fn rejection(status: StatusCode, code: &'static str) -> Response {
    let mut response = (status, Json(serde_json::json!({"code":code}))).into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}
pub(super) fn error(e: PortError) -> Response {
    let (status, code) = match e {
        PortError::Conflict => (StatusCode::CONFLICT, "version_conflict"),
        PortError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        PortError::InvalidInput { .. } => (StatusCode::BAD_REQUEST, "invalid_input"),
        PortError::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
        PortError::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        PortError::PermissionDenied => (StatusCode::FORBIDDEN, "permission_denied"),
        PortError::Unsupported => (StatusCode::NOT_FOUND, "not_supported"),
        PortError::OutcomeUnknown | PortError::DeadlineExceeded => {
            (StatusCode::SERVICE_UNAVAILABLE, "outcome_unknown")
        }
        PortError::CapacityExceeded { .. } => {
            (StatusCode::SERVICE_UNAVAILABLE, "capacity_exceeded")
        }
        _ => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    };
    rejection(status, code)
}
pub(super) async fn guard(
    State(runtime): State<Arc<ToolRuntime>>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let origins = headers.get_all("origin");
    if origins.iter().count() > 1
        || origins.iter().next().is_some_and(|v| {
            !v.to_str().ok().is_some_and(|origin| {
                let same_origin = headers
                    .get("host")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|host| {
                        origin == format!("http://{host}") || origin == format!("https://{host}")
                    });
                same_origin
                    || runtime
                        .security
                        .admin
                        .as_ref()
                        .is_some_and(|a| a.allowed_origins.iter().any(|s| s == origin))
            })
        })
        || headers
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site")
    {
        return rejection(StatusCode::FORBIDDEN, "origin_denied");
    }
    let Ok(_permit) = runtime.control_requests.clone().try_acquire_owned() else {
        return rejection(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    };
    let mut response = match tokio::time::timeout(Duration::from_secs(35), next.run(request)).await
    {
        Ok(response) => response,
        Err(_) => rejection(StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
    };
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}
async fn view(State(runtime): State<Arc<ToolRuntime>>) -> Response {
    match runtime.control_view().await {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    before_revision: Option<u64>,
    limit: Option<u32>,
}
async fn history(
    State(runtime): State<Arc<ToolRuntime>>,
    query: Result<Query<HistoryQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(q)) = query else {
        return rejection(StatusCode::BAD_REQUEST, "invalid_query");
    };
    match runtime
        .publication_history(q.before_revision, q.limit.unwrap_or(10))
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationQuery {
    expected_revision: u64,
    draft_generation: u64,
}
async fn operation(
    State(runtime): State<Arc<ToolRuntime>>,
    path: Result<Path<Uuid>, axum::extract::rejection::PathRejection>,
    query: Result<Query<OperationQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let (Ok(Path(operation_id)), Ok(Query(q))) = (path, query) else {
        return rejection(StatusCode::BAD_REQUEST, "invalid_query");
    };
    match runtime
        .publication_result(PublishInput {
            operation_id,
            expected_revision: q.expected_revision,
            draft_generation: q.draft_generation,
        })
        .await
    {
        Ok(Some(receipt)) => {
            Json(serde_json::json!({"status":"committed","receipt":receipt})).into_response()
        }
        Ok(None) => {
            Json(serde_json::json!({"status":"unconfirmed","receipt":null})).into_response()
        }
        Err(e) => error(e),
    }
}
async fn mutate(State(runtime): State<Arc<ToolRuntime>>, request: Request) -> Response {
    let method = request.method().clone();
    if request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next().unwrap_or("").trim() != "application/json")
    {
        return rejection(StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
    }
    let bytes = match tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), 16 * 1024),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return rejection(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
        Err(_) => return rejection(StatusCode::REQUEST_TIMEOUT, "request_timeout"),
    };
    if method == Method::PUT {
        let input: DraftInput = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
        };
        match runtime.save_exposure_draft(input).await {
            Ok(draft) => Json(draft).into_response(),
            Err(e) => error(e),
        }
    } else {
        let input: PublishInput = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return rejection(StatusCode::BAD_REQUEST, "invalid_input"),
        };
        match runtime.publish_exposure_draft(input).await {
            Ok(receipt) => Json(receipt).into_response(),
            Err(e) => error(e),
        }
    }
}
pub fn router(runtime: Arc<ToolRuntime>) -> Router {
    Router::new()
        .route(
            "/api/workspace",
            get(super::workspace_control::view).patch(super::workspace_control::update),
        )
        .route(
            "/api/workspace/members",
            get(super::workspace_control::members),
        )
        .route("/api/tools/config", get(view))
        .route("/api/tools/servers", get(super::server_control::list))
        .route(
            "/api/tools/servers/{id}",
            get(super::server_control::item)
                .put(super::server_control::item)
                .delete(super::server_control::item),
        )
        .route(
            "/api/tools/connections",
            post(super::connection_control::create),
        )
        .route(
            "/api/tools/connections/{operation_id}",
            get(super::connection_control::inspect)
                .put(super::connection_control::change)
                .delete(super::connection_control::change),
        )
        .route("/api/tools/verify", post(super::connection_control::verify))
        .route("/api/tools/history", get(history))
        .route("/api/tools/operations/{operation_id}", get(operation))
        .route("/api/tools/draft", axum::routing::put(mutate))
        .route("/api/tools/publish", post(mutate))
        .layer(middleware::from_fn_with_state(runtime.clone(), guard))
        .with_state(runtime)
}
