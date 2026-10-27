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
        let started = Instant::now();
        let job = leased.job;
        tracing::debug!(event="message.delivery_claimed",workspace_id=%ctx.identity.workspace_id,event_id=%job.causation_id,delivery_id=%job.delivery_id,connection_id=%job.destination.scope.connection_id,target_id=%job.destination.id,attempt=leased.attempt,"Delivery claimed");
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
        if let Err(error) = &preparation {
            tracing::warn!(event="message.delivery_preflight_failed",event_id=%job.causation_id,delivery_id=%job.delivery_id,error=%error,"Delivery preparation failed");
        }
        let outcome = match preparation {
            Err(
                PortError::Unavailable
                | PortError::RateLimited { .. }
                | PortError::CapacityExceeded { .. }
                | PortError::Cancelled
                | PortError::DeadlineExceeded,
            ) if leased.attempt < 8 => {
                DeliveryOutcome::Deferred(crate::application::retry::delay(leased.attempt, None))
            }
            Err(_) => DeliveryOutcome::Failed {
                code: "delivery_preflight_failed",
            },
            Ok((sink, replay_safety)) => {
                // If marking Sending fails, no external call is made. Storage recovery owns the lease.
                self.outbox.mark_sending(leased.token.clone()).await?;
                tracing::info!(event="message.send_started",workspace_id=%ctx.identity.workspace_id,event_id=%job.causation_id,correlation_id=%job.correlation_id,delivery_id=%job.delivery_id,connection_id=%job.destination.scope.connection_id,target_id=%job.destination.id,route_rule_id=?job.route_rule.map(|(id,_)|id),attempt=leased.attempt,"Sending message");
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
                    Ok(()) => {
                        match sink
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
                            Err(error) => {
                                tracing::warn!(event="message.send_error",event_id=%job.causation_id,delivery_id=%job.delivery_id,target_id=%job.destination.id,attempt=leased.attempt,error=%error,"Message send returned an error");
                                match error {
                                    PortError::RateLimited { retry_after }
                                        if leased.attempt < 8 =>
                                    {
                                        DeliveryOutcome::Deferred(crate::application::retry::delay(
                                            leased.attempt,
                                            Some(retry_after),
                                        ))
                                    }
                                    PortError::RateLimited { .. } => DeliveryOutcome::Failed {
                                        code: "rate_limit_retry_exhausted",
                                    },
                                    PortError::NotSent { retry_after } if leased.attempt < 8 => {
                                        DeliveryOutcome::Deferred(crate::application::retry::delay(
                                            leased.attempt,
                                            retry_after,
                                        ))
                                    }
                                    PortError::NotSent { .. } => DeliveryOutcome::Failed {
                                        code: "connection_retry_exhausted",
                                    },
                                    _ => match error.retry_advice() {
                                        RetryAdvice::IfIdempotent { after }
                                            if replay_safety == ReplaySafety::Idempotent
                                                && leased.attempt < 8 =>
                                        {
                                            DeliveryOutcome::RetryAfter(
                                                crate::application::retry::delay(
                                                    leased.attempt,
                                                    after,
                                                ),
                                            )
                                        }
                                        RetryAdvice::IfIdempotent { .. }
                                        | RetryAdvice::Reconcile => DeliveryOutcome::Reconcile,
                                        RetryAdvice::Never => DeliveryOutcome::Failed {
                                            code: "delivery_failed",
                                        },
                                    },
                                }
                            }
                        }
                    }
                }
            }
        };
        let state_recorded = self
            .outbox
            .finish(leased.token, outcome.clone())
            .await
            .is_ok();
        let (outcome_name, retry_after_ms) = match &outcome {
            DeliveryOutcome::Submitted(_) => ("submitted", 0),
            DeliveryOutcome::Deferred(d) => ("deferred", d.as_millis() as u64),
            DeliveryOutcome::RetryAfter(d) => ("retry_wait", d.as_millis() as u64),
            DeliveryOutcome::Failed { .. } => ("failed", 0),
            DeliveryOutcome::Reconcile => ("reconcile", 0),
        };
        if state_recorded && matches!(outcome, DeliveryOutcome::Submitted(_)) {
            tracing::info!(event="message.send_completed",event_id=%job.causation_id,correlation_id=%job.correlation_id,delivery_id=%job.delivery_id,target_id=%job.destination.id,attempt=leased.attempt,outcome=outcome_name,elapsed_ms=started.elapsed().as_millis() as u64,"Message accepted by destination");
        } else {
            tracing::warn!(event="message.delivery_outcome",event_id=%job.causation_id,delivery_id=%job.delivery_id,target_id=%job.destination.id,attempt=leased.attempt,outcome=outcome_name,state_recorded,retry_after_ms,elapsed_ms=started.elapsed().as_millis() as u64,"Delivery requires retry or attention");
        }
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
