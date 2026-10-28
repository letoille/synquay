//! Operator-managed paths. Protocol credentials remain in deployment-owned connections.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct RouteTarget {
    pub rule_id: Uuid,
    pub destination_id: Uuid,
    #[serde(default)]
    pub branch: RouteBranch,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteBranch {
    #[default]
    Legacy,
    Forward,
    Action,
    Reply,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAction {
    pub destination_id: Uuid,
    pub tool_id: String,
    pub revision: u64,
    pub allowed_senders: Vec<String>,
    pub arguments: serde_json::Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCommand {
    pub id: Uuid,
    pub mapping: super::command::CommandMapping,
    pub action: RouteAction,
    pub reply: Option<RouteReply>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteReply {
    Fixed { text: String },
    ActionResult,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ManagedRoute {
    #[serde(default = "default_message_types")]
    pub message_types: Vec<MessageType>,
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
        #[serde(default = "default_message_types")]
        message_types: Vec<MessageType>,
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
    Combined {
        forward: bool,
        action: Option<Box<RouteAction>>,
        reply: Option<RouteReply>,
    },
    CommandSet {
        forward: bool,
        commands: Vec<RouteCommand>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Message,
    State,
    Command,
}

pub fn default_message_types() -> Vec<MessageType> {
    vec![MessageType::Message, MessageType::Command]
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
