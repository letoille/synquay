use super::message::ContentPart;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolEffect {
    ReadOnly,
    SideEffecting,
    Unknown,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Stable within a connection; never use a display name as identity.
    pub id: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    /// A hint for policy, not evidence that an operation is authorized or safe.
    pub effect: ToolEffect,
    /// Optional to preserve old configurations and immutable stored snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<super::capability::documentation::CapabilityDocumentation>,
}

impl ToolDefinition {
    pub fn is_valid(&self) -> bool {
        super::capability::valid_id(&self.id)
            && super::capability::valid_text(&self.description, 8192)
            && self.input_schema.is_object()
            && self.output_schema.as_ref().is_none_or(Value::is_object)
            && self.documentation.as_ref().is_none_or(|d| d.is_valid())
    }
    pub fn agent_description(&self) -> String {
        let mut description = self.description.clone();
        if let Some(documentation) = &self.documentation {
            documentation.append_to(&mut description);
        }
        description
    }
}

pub struct ToolCall {
    pub invocation_id: Uuid,
    pub tool_id: String,
    /// JSON arguments for a configured operation, never an unrestricted shell command.
    pub arguments: Value,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<ContentPart>,
    pub structured_content: Option<Value>,
    /// Tool-level failure is separate from adapter/transport failure.
    pub is_error: bool,
}
