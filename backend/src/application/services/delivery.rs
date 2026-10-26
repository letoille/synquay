use super::governance::Governance;
use crate::{
    application::{
        connectors::ConnectorRegistry,
        ports::{
            PortError, RetryAdvice,
            delivery::{DeliveryOutbox, DestinationRepository},
            identity::AccessContext,
        },
    },
    domain::{
        audit::AuditPhase,
        delivery::{DeliveryOutcome, DeliveryRunReport, ReplaySafety},
        identity::{Action, Resource},
        message::DeliveryRequest,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

pub struct DeliveryService {
    outbox: Arc<dyn DeliveryOutbox>,
    destinations: Arc<dyn DestinationRepository>,
    connectors: Arc<ConnectorRegistry>,
    governance: Arc<Governance>,
}
impl DeliveryService {
    pub fn new(
        outbox: Arc<dyn DeliveryOutbox>,
        destinations: Arc<dyn DestinationRepository>,
        connectors: Arc<ConnectorRegistry>,
        governance: Arc<Governance>,
    ) -> Self {
        Self {
            outbox,
            destinations,
            connectors,
            governance,
        }
    }

    /// Called by a trusted tenant-scoped worker, never directly by model input.
    /// Processes one exclusive lease; the scheduler controls concurrency and wakeups.
    #[tracing::instrument(skip_all, fields(request_id = %ctx.operation.request_id))]
    pub async fn run_one(
        &self,
        ctx: &AccessContext,
    ) -> Result<Option<DeliveryRunReport>, PortError> {
        self.run_selected(ctx, None, false).await
    }
    #[tracing::instrument(skip_all, fields(request_id = %ctx.operation.request_id, target_id = %target))]
    pub async fn run_target(
        &self,
        ctx: &AccessContext,
        target: uuid::Uuid,
        strict_order: bool,
    ) -> Result<Option<DeliveryRunReport>, PortError> {
        self.run_selected(ctx, Some(target), strict_order).await
    }
    async fn run_selected(
        &self,
        ctx: &AccessContext,
        target: Option<uuid::Uuid>,
        strict_order: bool,
    ) -> Result<Option<DeliveryRunReport>, PortError> {
        ctx.check()?;
        let leased = match target {
            Some(id) => {
                self.outbox
                    .claim_target(
                        ctx.identity.workspace_id,
                        id,
                        strict_order,
                        SystemTime::now(),
                        Duration::from_secs(60),
                    )
                    .await?
            }
            None => {
                self.outbox
                    .claim(
                        ctx.identity.workspace_id,
                        SystemTime::now(),
                        Duration::from_secs(60),
                    )
                    .await?
            }
        };
        let Some(leased) = leased else {
            return Ok(None);
        };
        let job = leased.job;
        if leased.token.workspace_id != ctx.identity.workspace_id
            || job.destination.scope.workspace_id != ctx.identity.workspace_id
            || leased.token.delivery_id != job.delivery_id
        {
            return Err(PortError::ScopeMismatch);
        }
        if leased.token.expires_at <= SystemTime::now() || leased.attempt == 0 {
            return Err(PortError::Conflict);
        }
        let resource = Resource::Destination {
            scope: job.destination.scope,
            destination_id: job.destination.id,
        };
        let preparation = async {
            self.governance
                .authorize(ctx, Action::DeliverMessage, &resource)
                .await?;
            let current = self
                .destinations
                .resolve(ctx.identity.workspace_id, job.destination.id)
                .await?;
            if current.scope != job.destination.scope || current.id != job.destination.id {
                return Err(PortError::ScopeMismatch);
            }
            if job.route_trace.len() >= 32 || job.route_trace.contains(&current.scope) {
                return Err(PortError::InvalidInput { code: "route_loop" });
            }
            let sink = self
                .connectors
                .get(current.scope)?
                .sink
                .clone()
                .ok_or(PortError::Unsupported)?;
            sink.validate(&job.message)?;
            self.governance
                .record(
                    ctx,
                    job.delivery_id,
                    Action::DeliverMessage,
                    &resource,
                    AuditPhase::Started,
                )
                .await?;
            ctx.check()?;
            Ok::<_, PortError>((sink, current.replay_safety))
        }
        .await;
        let outcome = match preparation {
            Err(
                PortError::Unavailable
                | PortError::RateLimited { .. }
                | PortError::CapacityExceeded { .. }
                | PortError::Cancelled
                | PortError::DeadlineExceeded,
            ) if leased.attempt < 5 => {
                DeliveryOutcome::Deferred(Duration::from_secs(1 << leased.attempt.min(6)))
            }
            Err(_) => DeliveryOutcome::Failed {
                code: "delivery_preflight_failed",
            },
            Ok((sink, replay_safety)) => {
                // If marking Sending fails, no external call is made. Storage recovery owns the lease.
                self.outbox.mark_sending(leased.token.clone()).await?;
                let mut operation = ctx.operation.clone();
                operation.scope = job.destination.scope;
                operation.correlation_id = job.correlation_id;
                operation.causation_id = Some(job.causation_id);
                operation.route_trace = job.route_trace;
                operation.route_trace.push(job.destination.scope);
                let now = Instant::now();
                let remaining = leased
                    .token
                    .expires_at
                    .duration_since(SystemTime::now())
                    .unwrap_or_default();
                let lease_deadline = now + remaining;
                operation.deadline = Some(
                    operation
                        .deadline
                        .map_or(lease_deadline, |deadline| deadline.min(lease_deadline)),
                );
                // No select!/future drop around side effects. Driver handles cooperative cancellation.
                match ctx.check().and_then(|_| operation.check_active()) {
                    Err(_) => DeliveryOutcome::Failed {
                        code: "delivery_cancelled_before_send",
                    },
                    Ok(()) => match sink
                        .deliver(
                            operation,
                            DeliveryRequest {
                                delivery_id: job.delivery_id,
                                destination_id: job.destination.id,
                                message: job.message,
                            },
                        )
                        .await
                    {
                        Ok(receipt) if receipt.delivery_id == job.delivery_id => {
                            DeliveryOutcome::Submitted(receipt)
                        }
                        Ok(_) => DeliveryOutcome::Reconcile,
                        Err(error) => match error.retry_advice() {
                            RetryAdvice::IfIdempotent { after }
                                if replay_safety == ReplaySafety::Idempotent
                                    && leased.attempt < 5 =>
                            {
                                DeliveryOutcome::RetryAfter(
                                    after
                                        .unwrap_or(Duration::from_secs(1 << leased.attempt.min(6))),
                                )
                            }
                            RetryAdvice::IfIdempotent { .. } | RetryAdvice::Reconcile => {
                                DeliveryOutcome::Reconcile
                            }
                            RetryAdvice::Never => DeliveryOutcome::Failed {
                                code: "delivery_failed",
                            },
                        },
                    },
                }
            }
        };
        let state_recorded = self
            .outbox
            .finish(leased.token, outcome.clone())
            .await
            .is_ok();
        let audit_recorded = self
            .governance
            .record(
                ctx,
                job.delivery_id,
                Action::DeliverMessage,
                &resource,
                if matches!(outcome, DeliveryOutcome::Submitted(_)) {
                    AuditPhase::Succeeded
                } else {
                    AuditPhase::Failed
                },
            )
            .await
            .is_ok();
        Ok(Some(DeliveryRunReport {
            delivery_id: job.delivery_id,
            outcome,
            state_recorded,
            audit_recorded,
        }))
    }
}
