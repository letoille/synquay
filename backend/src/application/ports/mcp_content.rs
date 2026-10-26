use super::{OperationContext, PortFuture};
use crate::domain::mcp_content::{McpContentConfig, PromptText};
use std::{collections::BTreeMap, sync::Arc};

/// Implementations cannot expand the reviewed descriptor set at runtime.
pub trait McpContentProvider: Send + Sync {
    fn catalog(&self) -> Arc<McpContentConfig>;
    fn read<'a>(
        &'a self,
        context: OperationContext,
        id: &'a str,
        key: Option<&'a str>,
    ) -> PortFuture<'a, Arc<str>>;
    fn render<'a>(
        &'a self,
        context: OperationContext,
        id: &'a str,
        arguments: &'a BTreeMap<String, String>,
    ) -> PortFuture<'a, Vec<PromptText>>;
}
