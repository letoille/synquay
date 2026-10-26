//! Explicit composition root: no implicit allow-all policy, volatile storage or fake driver.
use super::{
    connectors::ConnectorRegistry,
    ports::{
        audit::AuditLog,
        delivery::{DeliveryOutbox, DestinationRepository},
        identity::{Authenticator, PolicyEngine},
        profiles::{ProfileRepository, SchemaValidator},
        routing::{EventReader, MessageRenderer, ReplyRouteStore, RoutingRules},
    },
    services::{
        delivery::DeliveryService, governance::Governance, identity::IdentityService,
        replies::ReplyService, routing::RoutingService, tools::ToolService,
    },
};
use std::sync::Arc;

pub struct ModuleDependencies {
    pub authenticator: Arc<dyn Authenticator>,
    pub policy: Arc<dyn PolicyEngine>,
    pub audit: Arc<dyn AuditLog>,
    pub profiles: Arc<dyn ProfileRepository>,
    pub validator: Arc<dyn SchemaValidator>,
    pub events: Arc<dyn EventReader>,
    pub rules: Arc<dyn RoutingRules>,
    pub renderer: Arc<dyn MessageRenderer>,
    pub destinations: Arc<dyn DestinationRepository>,
    pub outbox: Arc<dyn DeliveryOutbox>,
    pub reply_routes: Arc<dyn ReplyRouteStore>,
    pub connectors: Arc<ConnectorRegistry>,
}

pub struct BackendModules {
    pub identity: IdentityService,
    pub tools: ToolService,
    pub routing: RoutingService,
    pub delivery: DeliveryService,
    pub replies: ReplyService,
}
impl BackendModules {
    pub fn new(deps: ModuleDependencies) -> Self {
        let governance = Arc::new(Governance::new(deps.policy, deps.audit));
        Self {
            identity: IdentityService::new(deps.authenticator),
            tools: ToolService::new(
                deps.profiles,
                deps.validator,
                deps.connectors.clone(),
                governance.clone(),
            ),
            routing: RoutingService::new(
                deps.events,
                deps.rules,
                deps.renderer,
                deps.destinations.clone(),
                deps.outbox.clone(),
                governance.clone(),
            ),
            delivery: DeliveryService::new(
                deps.outbox.clone(),
                deps.destinations.clone(),
                deps.connectors,
                governance.clone(),
            ),
            replies: ReplyService::new(
                deps.reply_routes,
                deps.destinations,
                deps.outbox,
                governance,
            ),
        }
    }
}
