use super::governance::Governance;
use crate::{
    application::ports::{
        PortError,
        delivery::{DeliveryOutbox, DestinationRepository},
        identity::AccessContext,
        routing::ReplyRouteStore,
    },
    domain::{
        audit::AuditPhase,
        delivery::{DispatchKey, DispatchPlan, DispatchReport, PendingDelivery},
        identity::{Action, Resource},
        message::{ContentPart, Message},
    },
};
use std::{sync::Arc, time::SystemTime};
use uuid::Uuid;

pub struct ReplyService {
    routes: Arc<dyn ReplyRouteStore>,
    destinations: Arc<dyn DestinationRepository>,
    outbox: Arc<dyn DeliveryOutbox>,
    governance: Arc<Governance>,
}
impl ReplyService {
    pub fn new(
        routes: Arc<dyn ReplyRouteStore>,
        destinations: Arc<dyn DestinationRepository>,
        outbox: Arc<dyn DeliveryOutbox>,
        governance: Arc<Governance>,
    ) -> Self {
        Self {
            routes,
            destinations,
            outbox,
            governance,
        }
    }

    /// Agent supplies only a route reference and content, never the destination address.
    #[tracing::instrument(skip_all, fields(reply_id = %reply_id, route_id = %route_id))]
    pub async fn reply(
        &self,
        ctx: &AccessContext,
        route_id: Uuid,
        reply_id: Uuid,
        parts: Vec<ContentPart>,
    ) -> Result<DispatchReport, PortError> {
        let workspace = ctx.identity.workspace_id;
        let resource = Resource::ReplyRoute {
            workspace_id: workspace,
            route_id,
        };
        self.governance
            .authorize(ctx, Action::Reply, &resource)
            .await?;
        let route = self.routes.resolve(workspace, route_id).await?;
        if route.workspace_id != workspace || route.route_id != route_id {
            return Err(PortError::ScopeMismatch);
        }
        if route.owner_principal_id != ctx.identity.principal_id
            || route.expires_at <= SystemTime::now()
        {
            return Err(PortError::PermissionDenied);
        }
        if parts.is_empty() {
            return Err(PortError::InvalidInput {
                code: "empty_reply",
            });
        }
        let destination = self
            .destinations
            .resolve(workspace, route.destination_id)
            .await?;
        if destination.scope.workspace_id != workspace || destination.id != route.destination_id {
            return Err(PortError::ScopeMismatch);
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
        self.governance
            .record(ctx, reply_id, Action::Reply, &resource, AuditPhase::Started)
            .await?;
        ctx.check()?;
        let queued = self
            .outbox
            .enqueue(DispatchPlan {
                workspace_id: workspace,
                key: DispatchKey::Reply {
                    workspace_id: workspace,
                    reply_id,
                },
                deliveries: vec![PendingDelivery {
                    delivery_id: Uuid::new_v4(),
                    route_rule: None,
                    destination,
                    message: Message {
                        destination_binding: None,
                        parts,
                        reply_route_id: Some(route_id),
                    },
                    correlation_id: route.correlation_id,
                    causation_id: route.original_event_id,
                    // An explicitly authorized reply starts a new forwarding route; it may return to origin.
                    route_trace: vec![],
                }],
            })
            .await;
        let audit_recorded = self
            .governance
            .record(
                ctx,
                reply_id,
                Action::Reply,
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
