mod agent_tools;
mod connection_control;
mod content;
mod dto;
mod error;
mod handlers;
mod management;
mod mcp;
mod message_connection_control;
mod middleware;
mod qq;
mod route_control;
mod server_control;
mod tool_control;
pub mod web;
mod webhook;
mod workspace_control;

use crate::AppState;
use axum::{
    Router,
    middleware::from_fn,
    routing::{get, post},
};

pub fn router(state: AppState) -> Router {
    let tools = state.tools.clone();
    let route_control = tools.as_ref().map(|_| route_control::router(state.clone()));
    let message_control = tools
        .as_ref()
        .map(|_| message_connection_control::router(state.clone()));
    let router = Router::new()
        .route("/api/health", get(handlers::health))
        .route("/api/info", get(handlers::info))
        .route("/api/webhooks/{connection_id}", post(webhook::receive))
        .route("/api/qq/{connection_id}/webhook", post(qq::receive))
        .route("/api/content/upload/{connection_id}", post(content::upload))
        .route("/api/content/{id}", get(content::download))
        .route("/api/ops", get(management::handle))
        .route("/api/ops/recover", post(management::handle))
        .route("/api/ops/recovery/{kind}/{id}", get(management::inspect))
        .fallback(error::not_found)
        .method_not_allowed_fallback(error::method_not_allowed)
        .with_state(state);
    match tools {
        Some(runtime) => router
            .merge(mcp::router(runtime))
            .merge(route_control.expect("route control"))
            .merge(message_control.expect("message connections")),
        None => router,
    }
    .layer(from_fn(middleware::observe_request))
}
