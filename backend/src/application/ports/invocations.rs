//! Durable admission for a logical tool call. An unfinished call is never replayed.
use super::{PortError, PortFuture};
use crate::domain::{audit::AuditRecord, tool::ToolResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone)]
pub struct InvocationKey {
    pub workspace_id: Uuid,
    pub principal_id: Uuid,
    pub invocation_id: Uuid,
}
#[derive(Clone, Serialize, Deserialize)]
pub enum SavedOutcome {
    Completed(ToolResult),
    Failed,
    Unknown,
}
impl SavedOutcome {
    pub fn capture(outcome: &Result<ToolResult, PortError>) -> Self {
        match outcome {
            Ok(result) => Self::Completed(result.clone()),
            Err(PortError::OutcomeUnknown | PortError::DeadlineExceeded | PortError::Cancelled) => {
                Self::Unknown
            }
            Err(_) => Self::Failed,
        }
    }
    pub fn replay(self) -> Result<ToolResult, PortError> {
        match self {
            Self::Completed(result) => Ok(result),
            Self::Unknown => Err(PortError::OutcomeUnknown),
            Self::Failed => Err(PortError::Failed {
                code: "previous_invocation_failed",
            }),
        }
    }
}
pub enum Admission {
    Fresh,
    Cached(SavedOutcome),
}
pub trait InvocationLedger: Send + Sync {
    /// Fingerprint includes revision, tool identity and canonical arguments. A reused
    /// key with different intent conflicts; pending/crashed calls return Unknown.
    fn begin(&self, key: InvocationKey, fingerprint: String) -> PortFuture<'_, Admission>;
    /// Result and terminal audit MUST commit in the same transaction.
    fn finish(
        &self,
        key: InvocationKey,
        outcome: SavedOutcome,
        audit: AuditRecord,
    ) -> PortFuture<'_, ()>;
}
