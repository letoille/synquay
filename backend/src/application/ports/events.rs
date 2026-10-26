use super::{OperationContext, PortFuture};
use crate::domain::event::{EventEnvelope, IncomingEvent, IngestReceipt};
use std::{collections::BTreeMap, sync::Arc};

/// Bounded by the transport before decoding. Metadata is untrusted and may be sensitive.
pub struct RawInbound {
    pub media_type: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// Normalize HTTP webhook bodies, CLI stdout frames, MQTT payloads, etc.
/// Transport authentication/signature verification happens BEFORE this call.
pub trait InboundDecoder: Send + Sync {
    fn decode(&self, context: OperationContext, input: RawInbound)
    -> PortFuture<'_, IncomingEvent>;
}

/// Success means durable acceptance, NOT merely queueing in memory or downstream delivery.
/// Awaiting this future is the producer's backpressure boundary.
pub trait EventIngress: Send + Sync {
    fn accept(
        &self,
        context: OperationContext,
        input: IncomingEvent,
    ) -> PortFuture<'_, IngestReceipt>;
}

/// Transactionally persist the envelope and dedupe on (workspace, connection, source_event_id).
/// No upstream ID means no implied content-based dedupe. A duplicate returns the original ID.
/// If receipt is lost after commit, retry must resolve against the same dedupe key.
pub trait EventStore: Send + Sync {
    /// Atomic source cursor + durable event; adapters without this guarantee must reject.
    fn append_checkpoint(
        &self,
        _event: EventEnvelope,
        _checkpoint: super::checkpoints::Checkpoint,
    ) -> PortFuture<'_, IngestReceipt> {
        Box::pin(async { Err(super::PortError::Unsupported) })
    }

    fn append(&self, event: EventEnvelope) -> PortFuture<'_, IngestReceipt>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceExit {
    Completed,
    Cancelled,
}

/// One supervised run per connection. Used for MQTT subscriptions, CLI streams and polling.
/// Await ingress receipts before application acknowledgement/checkpoint advancement.
/// MQTT protocol PUBACK may precede durable ingest depending on the client: do not claim
/// lossless delivery unless the concrete client supports suitable acknowledgement control.
pub trait EventSource: Send + Sync {
    fn run(
        &self,
        context: OperationContext,
        ingress: Arc<dyn EventIngress>,
    ) -> PortFuture<'_, SourceExit>;
}
