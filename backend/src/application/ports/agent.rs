use super::{PortFuture, identity::AccessContext};
use crate::domain::agent::*;
use uuid::Uuid;

/// All mutations include durable audit and recheck membership in the storage transaction.
/// The adapter owns claiming/CAS and atomically commits the result and reply outbox.
pub trait AgentJobStore: Send + Sync {
    fn list(&self, ctx: AccessContext, after: Option<Uuid>) -> PortFuture<'_, Vec<JobSummary>>;
    fn read(&self, ctx: AccessContext, id: Uuid) -> PortFuture<'_, JobInput>;
    fn claim(&self, ctx: AccessContext, claim_id: Uuid) -> PortFuture<'_, Option<ClaimedJob>>;
    fn renew(&self, ctx: AccessContext, token: ClaimToken) -> PortFuture<'_, ClaimedJob>;
    fn complete(
        &self,
        ctx: AccessContext,
        token: ClaimToken,
        text: String,
    ) -> PortFuture<'_, CompletionReceipt>;
    fn fail(
        &self,
        ctx: AccessContext,
        token: ClaimToken,
        retry: bool,
    ) -> PortFuture<'_, JobSummary>;
}
