use super::PortFuture;
use crate::domain::{
    connector::ConnectionScope,
    event::EventEnvelope,
    message::Message,
    routing::{ReplyRoute, RouteRule},
};
use uuid::Uuid;

pub trait EventReader: Send + Sync {
    fn load(&self, scope: ConnectionScope, event_id: Uuid) -> PortFuture<'_, EventEnvelope>;
    /// Durable catch-up for the scheduler: committed events without a dispatch marker.
    fn unrouted(
        &self,
        workspace_id: Uuid,
        limit: usize,
    ) -> PortFuture<'_, Vec<(ConnectionScope, Uuid)>>;
}

pub trait RoutingRules: Send + Sync {
    fn matching<'a>(&'a self, event: &'a EventEnvelope) -> PortFuture<'a, Vec<RouteRule>>;
}

pub trait MessageRenderer: Send + Sync {
    fn render<'a>(
        &'a self,
        event: &'a EventEnvelope,
        rule: &'a RouteRule,
    ) -> PortFuture<'a, Message>;
}

pub trait ReplyRouteStore: Send + Sync {
    /// Route creation/binding is a server-side responsibility, never inferred from AI output.
    fn resolve(&self, workspace_id: Uuid, route_id: Uuid) -> PortFuture<'_, ReplyRoute>;
}
