//! A connection exposes operations separately from its message transport facets.
use super::{PortError, tools::ToolExecutor};
use crate::domain::{connection_routing::RouteBinding, tool::ToolDefinition};
use std::sync::Arc;

#[derive(Clone, serde::Serialize)]
pub struct ConnectionAction {
    pub revision: u64,
    pub definition: ToolDefinition,
    pub target_required: bool,
    /// Whether successful execution consumes the first passive-reply slot at its target.
    pub consumes_reply: bool,
}

pub trait ConnectionActions: Send + Sync {
    fn definitions(&self) -> Vec<ConnectionAction>;
    /// Bind trusted operator-selected addressing outside user/Agent supplied arguments.
    fn executor(&self, target: Option<RouteBinding>) -> Result<Arc<dyn ToolExecutor>, PortError>;
}
