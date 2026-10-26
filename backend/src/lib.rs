//! Composition root; business modules must not depend on HTTP adapters.
pub mod application;
pub mod config;
pub mod domain;
pub mod infrastructure;
pub mod interfaces;

use application::system::SystemService;
use infrastructure::pipeline::Pipeline;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct AppState {
    pub system: SystemService,
    pub tools: Option<Arc<infrastructure::tool_runtime::ToolRuntime>>,
    pub pipeline: Option<Arc<Pipeline>>,
}

pub fn app() -> axum::Router {
    interfaces::http::router(AppState::default())
}
