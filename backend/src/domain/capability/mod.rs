//! Protocol-independent contracts. Descriptions are data, never authority.
pub mod binding;
pub mod documentation;
pub mod resource;

use super::{connector::ConnectionScope, tool::ToolDefinition};
use resource::ResourceRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRef {
    pub scope: ConnectionScope,
    pub id: String,
    pub revision: u64,
}
impl CapabilityRef {
    pub fn is_valid(&self) -> bool {
        !self.scope.workspace_id.is_nil()
            && !self.scope.connection_id.is_nil()
            && valid_id(&self.id)
            && self.revision > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionOrigin {
    Operator,
    Imported,
    Discovered,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapabilitySpec {
    Action {
        tool: ToolDefinition,
        execution: ExecutionSemantics,
    },
    State {
        description: String,
        schema: Value,
        /// Readers report stale/unknown instead of presenting old data as current.
        max_age_ms: u64,
    },
    Event {
        description: String,
        schema: Value,
        /// Replay support is not a guarantee against source-side loss.
        replay: ReplaySupport,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplaySupport {
    None,
    SourceCursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionCondition {
    ResponseValidated,
    ProcessExited,
    TransportAccepted,
    TargetConfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Inline,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    Unknown,
    NonIdempotent,
    Idempotent,
    DeduplicatedByTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSemantics {
    pub mode: ExecutionMode,
    pub completion: CompletionCondition,
    pub idempotency: Idempotency,
    pub timeout_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDescriptor {
    pub reference: CapabilityRef,
    pub origin: DefinitionOrigin,
    /// Fixed business object, distinct from an MCP resource URI.
    pub resource: Option<ResourceRef>,
    pub spec: CapabilitySpec,
}
impl CapabilityDescriptor {
    pub fn is_valid(&self) -> bool {
        self.reference.is_valid()
            && self
                .resource
                .as_ref()
                .is_none_or(|r| r.is_valid() && r.scope == self.reference.scope)
            && match &self.spec {
                CapabilitySpec::Action { tool, execution } => {
                    tool.id == self.reference.id
                        && tool.is_valid()
                        && (1..=86_400_000).contains(&execution.timeout_ms)
                }
                CapabilitySpec::State {
                    description,
                    schema,
                    max_age_ms,
                } => valid_text(description, 8192) && schema.is_object() && *max_age_ms > 0,
                CapabilitySpec::Event {
                    description,
                    schema,
                    ..
                } => valid_text(description, 8192) && schema.is_object(),
            }
    }

    /// Subscriptions and state are never fabricated as callable tools.
    pub fn tool(&self) -> Option<&ToolDefinition> {
        match &self.spec {
            CapabilitySpec::Action { tool, .. } => Some(tool),
            _ => None,
        }
    }
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
pub(crate) fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}
