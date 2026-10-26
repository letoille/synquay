use super::identity::{Action, Resource};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditPhase {
    Started,
    Succeeded,
    Failed,
    Denied,
}

/// Intentionally contains no token, arguments, message bodies or tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub record_id: Uuid,
    pub operation_id: Uuid,
    pub request_id: Uuid,
    pub workspace_id: Uuid,
    pub principal_id: Uuid,
    pub action: Action,
    pub resource: Resource,
    pub phase: AuditPhase,
    pub occurred_at: SystemTime,
}
