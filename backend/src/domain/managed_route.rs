//! Operator-managed paths. Protocol credentials remain in deployment-owned connections.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct RouteTarget {
    pub rule_id: Uuid,
    pub destination_id: Uuid,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ManagedRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<RouteResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<super::connection_routing::ConnectionRouting>,
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processor: Option<RouteProcessor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<super::command::CommandMapping>,
    pub event_type: Option<String>,
    pub revision: u64,
    pub targets: Vec<RouteTarget>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RouteSnapshot {
    pub revision: u64,
    pub routes: Vec<ManagedRoute>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteInput {
    pub operation_id: Uuid,
    pub expected_revision: u64,
    pub route_id: Uuid,
    pub change: RouteChange,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteChange {
    Save {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<RouteResponse>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        routing: Option<Box<super::connection_routing::ConnectionRouting>>,
        name: String,
        #[serde(default = "enabled_by_default")]
        enabled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        processor: Option<RouteProcessor>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "command_change"
        )]
        command: Option<Option<Box<super::command::CommandMapping>>>,
        event_type: Option<String>,
        destination_ids: Vec<Uuid>,
    },
    Delete,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RouteReceipt {
    pub operation_id: Uuid,
    pub revision: u64,
}

/// Agent drafts are displayable configuration, never an executable processing mode.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteProcessor {
    Forward,
    Respond,
    /// Invoke one configured connection operation. Addressing belongs to route bindings.
    InvokeCapability {
        tool_id: String,
        revision: u64,
        allowed_senders: Vec<String>,
    },
    AgentDraft {
        name: String,
    },
    /// Submit a task to an operator-configured HTTP worker; not in-process AI execution.
    AgentHttp {
        name: String,
    },
}

// Omitted keeps an existing mapping (older clients); explicit null removes it.
fn command_change<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<Box<super::command::CommandMapping>>>, D::Error> {
    Option::deserialize(deserializer).map(Some)
}

/// Reply policy belongs to the route, independently of its processing step.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteResponse {
    None,
    Source,
}

fn enabled_by_default() -> bool {
    true
}
