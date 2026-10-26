use super::PortFuture;
use crate::domain::recovery::{RecoveryCommand, RecoveryReceipt};
use uuid::Uuid;
/// Trusted application boundary: caller must authorize the operator for this workspace.
pub trait RecoveryStore: Send + Sync {
    /// Atomic CAS, capacity update, immutable operator record and idempotent receipt.
    fn recover(
        &self,
        workspace: Uuid,
        principal: Uuid,
        command: RecoveryCommand,
    ) -> PortFuture<'_, RecoveryReceipt>;
}
