use super::{
    connector::ConnectionScope,
    message::{DeliveryReceipt, Message},
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplaySafety {
    Idempotent,
    NonIdempotent,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Destination {
    pub id: Uuid,
    pub scope: ConnectionScope,
    /// Derived from trusted adapter configuration, never a caller's claim.
    pub replay_safety: ReplaySafety,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PendingDelivery {
    pub delivery_id: Uuid,
    pub route_rule: Option<(Uuid, u64)>,
    pub destination: Destination,
    pub message: Message,
    pub correlation_id: Uuid,
    pub causation_id: Uuid,
    pub route_trace: Vec<ConnectionScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DispatchKey {
    Event {
        scope: ConnectionScope,
        event_id: Uuid,
    },
    Reply {
        workspace_id: Uuid,
        reply_id: Uuid,
    },
}

pub struct DispatchPlan {
    pub workspace_id: Uuid,
    pub key: DispatchKey,
    pub deliveries: Vec<PendingDelivery>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchReceipt {
    pub delivery_ids: Vec<Uuid>,
    pub duplicate: bool,
}

#[derive(Debug, Clone)]
pub struct LeaseToken {
    pub workspace_id: Uuid,
    pub delivery_id: Uuid,
    pub lease_id: Uuid,
    pub expires_at: SystemTime,
}

pub struct LeasedDelivery {
    pub token: LeaseToken,
    pub job: PendingDelivery,
    pub attempt: u32,
}

#[derive(Clone)]
pub enum DeliveryOutcome {
    Submitted(DeliveryReceipt),
    RetryAfter(Duration),
    /// A transient failure before external I/O; safe even for a non-idempotent target.
    Deferred(Duration),
    Failed {
        code: &'static str,
    },
    Reconcile,
}

pub struct DeliveryRunReport {
    pub delivery_id: Uuid,
    pub outcome: DeliveryOutcome,
    /// false requires recovery of the same lease/job; never blindly resend.
    pub state_recorded: bool,
    pub audit_recorded: bool,
}

pub struct DispatchReport {
    pub receipt: DispatchReceipt,
    pub audit_recorded: bool,
}
