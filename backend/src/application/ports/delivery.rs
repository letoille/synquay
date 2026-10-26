use super::PortFuture;
use crate::domain::delivery::{
    DeliveryOutcome, Destination, DispatchPlan, DispatchReceipt, LeaseToken, LeasedDelivery,
};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

pub trait DestinationRepository: Send + Sync {
    fn resolve(&self, workspace_id: Uuid, destination_id: Uuid) -> PortFuture<'_, Destination>;
}

pub trait DeliveryOutbox: Send + Sync {
    /// Atomic marker + all jobs, including empty plans. Duplicate keys return original IDs.
    /// Never overwrite a committed plan; reject conflicting reuse of reply IDs/payloads.
    fn enqueue(&self, plan: DispatchPlan) -> PortFuture<'_, DispatchReceipt>;
    /// Atomic exclusive lease. Recover expired Sending rows as Reconcile unless replay-safe.
    fn claim(
        &self,
        workspace_id: Uuid,
        now: SystemTime,
        lease_for: Duration,
    ) -> PortFuture<'_, Option<LeasedDelivery>>;
    /// Target-filtered claim for isolated schedulers. Strict order also holds later jobs
    /// behind unresolved earlier work; implementations without this capability reject it.
    fn claim_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
        strict_order: bool,
        now: SystemTime,
        lease_for: Duration,
    ) -> PortFuture<'_, Option<LeasedDelivery>> {
        let _ = (workspace_id, target_id, strict_order, now, lease_for);
        Box::pin(async { Err(super::PortError::Unsupported) })
    }
    /// CAS using lease_id and expiry. Must persist Sending BEFORE external I/O.
    fn mark_sending(&self, token: LeaseToken) -> PortFuture<'_, ()>;
    /// CAS using lease_id; reject stale owners. Submitted(Accepted) is not delivered/read.
    fn finish(&self, token: LeaseToken, outcome: DeliveryOutcome) -> PortFuture<'_, ()>;
}
