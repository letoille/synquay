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

/// Connection capabilities describe available operations and data, never grant access.
#[derive(Clone, serde::Serialize)]
pub struct StateGroupInfo {
    pub id: String,
    pub name: String,
    pub max_age_ms: u64,
    pub fields: Vec<StateFieldInfo>,
}
#[derive(Clone, serde::Serialize)]
pub struct StateFieldInfo {
    pub name: String,
    pub value_type: StateValueType,
}
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateValueType {
    Boolean,
    Number,
    String,
}
#[derive(Clone, serde::Serialize)]
pub struct StateResourceInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Current query operation, when the adapter provides one.
    pub query_operation: Option<ConnectionAction>,
    pub groups: Vec<StateGroupInfo>,
}
#[derive(Clone, serde::Serialize)]
pub struct MessageCapabilities {
    pub receive: bool,
    pub send: bool,
    /// The adapter can retain a bounded process-local recent text cache.
    pub recent_text_cache: bool,
}
#[derive(Clone, serde::Serialize)]
pub struct ConnectionCapabilitySet {
    pub operations: Vec<ConnectionAction>,
    pub resources: Vec<StateResourceInfo>,
    pub messages: MessageCapabilities,
}
pub trait ConnectionCapabilityCatalog {
    fn capabilities(&self) -> ConnectionCapabilitySet;
}
