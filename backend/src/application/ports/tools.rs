use super::{OperationContext, PortFuture};
use crate::domain::tool::{ToolCall, ToolDefinition, ToolResult};

pub trait ToolExecutor: Send + Sync {
    fn discover(&self, context: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>>;
    /// Validate schema and policy before invoking; CLI uses configured program + argv,
    /// HTTP uses configured operation IDs, and MCP uses selected upstream capabilities.
    /// After ambiguous side effects return OutcomeUnknown, never imply rollback on timeout.
    fn invoke(&self, context: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult>;
}
