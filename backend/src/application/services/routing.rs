use super::governance::Governance;
use crate::{
    application::ports::{
        PortError,
        delivery::{DeliveryOutbox, DestinationRepository},
        identity::AccessContext,
        routing::{EventReader, MessageRenderer, RoutingRules},
    },
    domain::{
        audit::AuditPhase,
        delivery::{DispatchKey, DispatchPlan, DispatchReport, PendingDelivery},
        identity::{Action, Resource},
    },
};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

pub struct RoutingService {
    events: Arc<dyn EventReader>,
    rules: Arc<dyn RoutingRules>,
    renderer: Arc<dyn MessageRenderer>,
    destinations: Arc<dyn DestinationRepository>,
    outbox: Arc<dyn DeliveryOutbox>,
    governance: Arc<Governance>,
}
impl RoutingService {
    pub fn new(
        events: Arc<dyn EventReader>,
        rules: Arc<dyn RoutingRules>,
        renderer: Arc<dyn MessageRenderer>,
        destinations: Arc<dyn DestinationRepository>,
        outbox: Arc<dyn DeliveryOutbox>,
        governance: Arc<Governance>,
    ) -> Self {
        Self {
            events,
            rules,
            renderer,
            destinations,
            outbox,
            governance,
        }
    }

    #[tracing::instrument(skip_all, fields(event_id = %event_id, request_id = %ctx.operation.request_id))]
    pub async fn route(
        &self,
        ctx: &AccessContext,
        event_id: Uuid,
    ) -> Result<DispatchReport, PortError> {
        let scope = ctx.operation.scope;
        let resource = Resource::Event { scope, event_id };
        self.governance
            .authorize(ctx, Action::RouteEvent, &resource)
            .await?;
        let event = self.events.load(scope, event_id).await?;
        if event.scope != scope || event.event_id != event_id {
            return Err(PortError::ScopeMismatch);
        }
        if event.route_trace.len() >= 32 {
            return Err(PortError::InvalidInput {
                code: "route_limit",
            });
        }
        let mut rules_seen = HashSet::new();
        let mut deliveries = vec![];
        for rule in self.rules.matching(&event).await? {
            if rule.workspace_id != scope.workspace_id {
                return Err(PortError::ScopeMismatch);
            }
            if rule.revision == 0 || !rules_seen.insert(rule.rule_id) {
                return Err(PortError::InvalidInput {
                    code: "invalid_route_snapshot",
                });
            }
            let destination = self
                .destinations
                .resolve(scope.workspace_id, rule.destination_id)
                .await?;
            if destination.id != rule.destination_id
                || destination.scope.workspace_id != scope.workspace_id
            {
                return Err(PortError::ScopeMismatch);
            }
            if destination.scope == scope || event.route_trace.contains(&destination.scope) {
                return Err(PortError::InvalidInput { code: "route_loop" });
            }
            self.governance
                .authorize(
                    ctx,
                    Action::DeliverMessage,
                    &Resource::Destination {
                        scope: destination.scope,
                        destination_id: destination.id,
                    },
                )
                .await?;
            let mut message = self.renderer.render(&event, &rule).await?;
            // Forwarding does not grant permission to use a reply-route reference from payloads.
            message.reply_route_id = None;
            deliveries.push(PendingDelivery {
                delivery_id: Uuid::new_v4(),
                route_rule: Some((rule.rule_id, rule.revision)),
                destination,
                message,
                correlation_id: event.correlation_id,
                causation_id: event_id,
                route_trace: event.route_trace.clone(),
            });
        }
        self.governance
            .record(
                ctx,
                event_id,
                Action::RouteEvent,
                &resource,
                AuditPhase::Started,
            )
            .await?;
        ctx.check()?;
        let queued = self
            .outbox
            .enqueue(DispatchPlan {
                workspace_id: scope.workspace_id,
                key: DispatchKey::Event { scope, event_id },
                deliveries,
            })
            .await;
        let audit_recorded = self
            .governance
            .record(
                ctx,
                event_id,
                Action::RouteEvent,
                &resource,
                if queued.is_ok() {
                    AuditPhase::Succeeded
                } else {
                    AuditPhase::Failed
                },
            )
            .await
            .is_ok();
        Ok(DispatchReport {
            receipt: queued?,
            audit_recorded,
        })
    }
}
