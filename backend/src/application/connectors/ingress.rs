use crate::{
    application::ports::{
        OperationContext, PortError, PortFuture,
        events::{EventIngress, EventStore},
    },
    domain::{
        connector::ConnectionScope,
        event::{EventEnvelope, IncomingEvent, IngestReceipt},
    },
};
use std::{sync::Arc, time::SystemTime};
use tracing::Instrument;
use uuid::Uuid;

/// Binds ingestion to one trusted connection and a separately implemented durable store.
pub struct IngressService {
    scope: ConnectionScope,
    store: Arc<dyn EventStore>,
}

impl IngressService {
    pub fn new(scope: ConnectionScope, store: Arc<dyn EventStore>) -> Self {
        Self { scope, store }
    }
}

impl IngressService {
    pub fn accept_checkpoint(
        &self,
        context: OperationContext,
        input: IncomingEvent,
        checkpoint: Option<crate::application::ports::checkpoints::Checkpoint>,
    ) -> PortFuture<'_, IngestReceipt> {
        let span = tracing::info_span!("event.ingest",
            workspace_id = %self.scope.workspace_id, connection_id = %self.scope.connection_id,
            request_id = %context.request_id, correlation_id = %context.correlation_id);
        Box::pin(
            async move {
                context.check_active()?;
                if context.scope != self.scope {
                    return Err(PortError::ScopeMismatch);
                }
                if input.event_type.trim().is_empty()
                    || input.schema_version == 0
                    || input
                        .source_event_id
                        .as_ref()
                        .is_some_and(|id| id.trim().is_empty())
                {
                    return Err(PortError::InvalidInput {
                        code: "invalid_event_metadata",
                    });
                }
                if context.route_trace.len() >= 32 || context.route_trace.contains(&self.scope) {
                    return Err(PortError::InvalidInput {
                        code: "event_route_loop_or_limit",
                    });
                }
                let mut route_trace = context.route_trace;
                route_trace.push(self.scope);
                let envelope = EventEnvelope {
                    state_report: false,
                    route_context: input.route_context,
                    event_id: Uuid::new_v4(),
                    scope: self.scope,
                    event_type: input.event_type,
                    schema_version: input.schema_version,
                    occurred_at: input.occurred_at,
                    received_at: SystemTime::now(),
                    source_event_id: input.source_event_id,
                    correlation_id: context.correlation_id,
                    causation_id: context.causation_id,
                    route_trace,
                    payload: input.payload,
                };
                // Do not race persistence against cancellation: a commit may already have happened.
                // The store must bound its own I/O. Return the committed receipt even if a stop
                // arrives during commit, so the source can safely acknowledge/checkpoint.
                let receipt = match checkpoint {
                    Some(cp) => self.store.append_checkpoint(envelope, cp).await?,
                    None => self.store.append(envelope).await?,
                };
                tracing::debug!(event = "event.accepted", event_id = %receipt.event_id,
                disposition = ?receipt.disposition, "Event durably accepted");
                Ok(receipt)
            }
            .instrument(span),
        )
    }
}

impl EventIngress for IngressService {
    fn accept(
        &self,
        context: OperationContext,
        input: IncomingEvent,
    ) -> PortFuture<'_, IngestReceipt> {
        self.accept_checkpoint(context, input, None)
    }
}
