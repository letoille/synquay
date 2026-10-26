use super::{connector::ConnectionScope, tool::ToolDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct PublishedTool {
    pub public_name: String,
    pub scope: ConnectionScope,
    pub definition: ToolDefinition,
}

/// Immutable published snapshot. Draft configuration is a separate future model.
#[derive(Clone, Serialize, Deserialize)]
pub struct PublishedProfile {
    pub workspace_id: Uuid,
    pub profile_id: Uuid,
    pub revision: u64,
    pub tools: Vec<PublishedTool>,
}
impl PublishedProfile {
    pub fn is_valid(&self) -> bool {
        let mut names = HashSet::new();
        self.revision > 0
            && self.tools.iter().all(|tool| {
                tool.scope.workspace_id == self.workspace_id
                    && !tool.public_name.is_empty()
                    && tool.public_name.len() <= 128
                    && tool
                        .public_name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                    && names.insert(&tool.public_name)
                    && tool.definition.is_valid()
            })
    }
}
