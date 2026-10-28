use crate::{
    application::ports::{
        PortError, PortFuture,
        delivery::DestinationRepository,
        identity::PolicyEngine,
        routing::{MessageRenderer, RoutingRules},
    },
    domain::{
        connector::ConnectionScope,
        delivery::Destination,
        event::{EventEnvelope, EventPayload},
        identity::{Action, Identity, Resource},
        message::{ContentPart, Message},
        routing::RouteRule,
    },
};
use uuid::Uuid;

/// Single operator-configured route. No general-purpose policy engine or wildcard grants.
pub(super) struct Bindings {
    pub connections:
        std::sync::Arc<std::sync::RwLock<super::message_connections::ConnectionSnapshot>>,
    pub source: ConnectionScope,
    pub sources: Vec<ConnectionScope>,
    pub destinations: Vec<Destination>,
    pub principal: Uuid,
    pub routes: std::sync::RwLock<crate::domain::managed_route::RouteSnapshot>,
}
impl PolicyEngine for Bindings {
    fn authorize(
        &self,
        identity: Identity,
        action: Action,
        resource: Resource,
    ) -> PortFuture<'_, ()> {
        Box::pin(async move {
            let allowed = identity.workspace_id == self.source.workspace_id
                && identity.principal_id == self.principal
                && match (action, resource) {
                    (Action::RouteEvent, Resource::Event { scope, .. }) => self.has_source(scope),
                    (
                        Action::DeliverMessage,
                        Resource::Destination {
                            scope,
                            destination_id,
                        },
                    ) => self
                        .destination(destination_id)
                        .is_some_and(|d| scope == d.scope),
                    _ => false,
                };
            if allowed {
                Ok(())
            } else {
                Err(PortError::PermissionDenied)
            }
        })
    }
}
impl DestinationRepository for Bindings {
    fn resolve(&self, workspace: Uuid, id: Uuid) -> PortFuture<'_, Destination> {
        Box::pin(async move {
            if workspace == self.source.workspace_id
                && let Some(d) = self.destination(id)
            {
                Ok(d.clone())
            } else {
                Err(PortError::NotFound)
            }
        })
    }
}
impl RoutingRules for Bindings {
    fn matching<'a>(&'a self, event: &'a EventEnvelope) -> PortFuture<'a, Vec<RouteRule>> {
        Box::pin(async move {
            if !self.has_source(event.scope) {
                return Err(PortError::ScopeMismatch);
            }
            let snapshot = self.routes.read().map_err(|_| PortError::Unavailable)?;
            use crate::domain::managed_route::MessageType;
            // A command label is produced only by a matching, enabled route for
            // this same source and source binding. It is not an execution grant.
            let needs_command = snapshot.routes.iter().any(|r| {
                r.enabled
                    && r.message_types
                        .iter()
                        .any(|kind| matches!(kind, MessageType::Message | MessageType::Command))
            });
            let command = needs_command
                && snapshot.routes.iter().any(|r| {
                    r.enabled
                        && (!event.state_report || r.message_types.contains(&MessageType::State))
                        && r.source_id.unwrap_or(self.source.connection_id)
                            == event.scope.connection_id
                        && self.source_matches(r, event)
                        && r.event_type
                            .as_ref()
                            .is_none_or(|kind| *kind == event.event_type)
                        && r.command
                            .as_ref()
                            .is_some_and(|c| c.capture(&event.payload).is_some())
                });
            let matches_type = |kinds: &[MessageType]| {
                kinds.iter().any(|kind| match kind {
                    MessageType::Message => !event.state_report && !command,
                    MessageType::State => event.state_report,
                    MessageType::Command => command,
                })
            };
            Ok(snapshot
                .routes
                .iter()
                .filter(|r| {
                    r.enabled
                        && matches_type(&r.message_types)
                        && (!event.state_report || r.message_types.contains(&MessageType::State))
                        && r.source_id.unwrap_or(self.source.connection_id)
                            == event.scope.connection_id
                        && self.source_matches(r, event)
                        && r.command
                            .as_ref()
                            .is_none_or(|c| c.capture(&event.payload).is_some())
                        && r.event_type
                            .as_ref()
                            .is_none_or(|kind| *kind == event.event_type)
                })
                .flat_map(|r| {
                    r.targets.iter().map(|t| RouteRule {
                        workspace_id: self.source.workspace_id,
                        rule_id: t.rule_id,
                        revision: r.revision,
                        destination_id: t.destination_id,
                    })
                })
                .collect())
        })
    }
}
impl MessageRenderer for Bindings {
    fn render<'a>(
        &'a self,
        event: &'a EventEnvelope,
        rule: &'a RouteRule,
    ) -> PortFuture<'a, Message> {
        Box::pin(async move {
            if !self.has_source(event.scope)
                || !self.matching(event).await?.iter().any(|r| {
                    r.rule_id == rule.rule_id
                        && r.destination_id == rule.destination_id
                        && r.revision == rule.revision
                })
            {
                return Err(PortError::ScopeMismatch);
            }
            let response_route = self
                .routes
                .read()
                .map_err(|_| PortError::Unavailable)?
                .routes
                .iter()
                .find(|r| {
                    r.revision == rule.revision
                        && r.targets.iter().any(|t| t.rule_id == rule.rule_id)
                        && matches!(
                            r.processor,
                            Some(crate::domain::managed_route::RouteProcessor::Respond)
                        )
                })
                .cloned();
            if let Some(r) = response_route {
                let mapping = r.command.as_ref().ok_or(PortError::PermissionDenied)?;
                let captured = mapping
                    .capture(&event.payload)
                    .ok_or(PortError::PermissionDenied)?;
                let (_, value) = mapping
                    .render(&event.payload, &captured)
                    .map_err(|_| PortError::InvalidInput {
                        code: "command_mapping_failed",
                    })?
                    .ok_or(PortError::Unsupported)?;
                let text = value["content"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty() && s.len() <= 4000)
                    .ok_or(PortError::InvalidInput {
                        code: "invalid_direct_response",
                    })?;
                let catalog = self
                    .connections
                    .read()
                    .map_err(|_| PortError::Unavailable)?;
                let source = catalog
                    .connections
                    .iter()
                    .find(|c| c.id == event.scope.connection_id && c.enabled)
                    .ok_or(PortError::PermissionDenied)?;
                return adapter(&source.config).command_reply(event, text.into(), None);
            }
            let action_route=self.routes.read().map_err(|_|PortError::Unavailable)?.routes.iter().find(|r|
                r.revision==rule.revision && r.targets.iter().any(|t|t.rule_id==rule.rule_id)
                && matches!(r.processor,Some(crate::domain::managed_route::RouteProcessor::InvokeCapability{..}))).map(|r|r.id);
            if let Some(route_id) = action_route {
                return super::command_actions::CommandJob {
                    route_id,
                    revision: rule.revision,
                    source: event.scope,
                    event_id: event.event_id,
                }
                .message();
            }
            let destination_binding = self.target_binding(event, rule)?;
            let command = self
                .routes
                .read()
                .map_err(|_| PortError::Unavailable)?
                .routes
                .iter()
                .find(|r| {
                    r.revision == rule.revision
                        && r.targets.iter().any(|t| t.rule_id == rule.rule_id)
                })
                .and_then(|r| r.command.clone());
            if let Some(command) = command {
                let captured = command
                    .capture(&event.payload)
                    .ok_or(PortError::ScopeMismatch)?;
                if let Some((event_type, payload)) = command
                    .render(&event.payload, &captured)
                    .map_err(|_| PortError::InvalidInput {
                        code: "command_mapping_failed",
                    })?
                {
                    return Ok(Message {
                        destination_binding: destination_binding.clone(),
                        parts: vec![ContentPart::Json(serde_json::json!({
                            "event_id": event.event_id, "source_event_id": event.source_event_id,
                            "event_type": event_type, "payload": payload
                        }))],
                        reply_route_id: None,
                    });
                }
            }
            match &event.payload {
                EventPayload::Json(value) => Ok(Message {
                    destination_binding: destination_binding.clone(),
                    parts: vec![ContentPart::Json(
                        serde_json::json!({"event_id":event.event_id,"source_event_id":event.source_event_id,"event_type":event.event_type,"payload":value}),
                    )],
                    reply_route_id: None,
                }),
                EventPayload::Content(reference) => Ok(Message {
                    destination_binding: destination_binding.clone(),
                    parts: vec![ContentPart::StoredContent(reference.clone())],
                    reply_route_id: None,
                }),
                EventPayload::Message(message) => {
                    let mut message = message.clone();
                    message.destination_binding = destination_binding;
                    Ok(message)
                }
                _ => Err(PortError::Unsupported),
            }
        })
    }
}

impl Bindings {
    fn has_source(&self, scope: ConnectionScope) -> bool {
        self.sources.contains(&scope)
            || (scope.workspace_id == self.source.workspace_id
                && self.connections.read().is_ok_and(|s| {
                    s.connections
                        .iter()
                        .any(|c| c.receives() && c.id == scope.connection_id)
                }))
    }
    fn destination(&self, id: Uuid) -> Option<Destination> {
        self.destinations
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .or_else(|| {
                self.connections
                    .read()
                    .ok()?
                    .connections
                    .iter()
                    .find(|c| c.sends() && c.send_id() == id)
                    .map(|c| Destination {
                        id,
                        scope: ConnectionScope {
                            workspace_id: self.source.workspace_id,
                            connection_id: c.send_id(),
                        },
                        replay_safety: crate::domain::delivery::ReplaySafety::NonIdempotent,
                    })
            })
    }
}

use crate::{
    domain::{connection_routing::RouteBinding, managed_route::ManagedRoute},
    infrastructure::{
        connectors::routing::adapter, pipeline::message_connections::ConnectionSnapshot,
    },
};
/// Shared by route edits and connection edits, so changing a connector cannot silently widen routes.
pub(super) fn validate_routing(
    catalog: &ConnectionSnapshot,
    route: &ManagedRoute,
    default_source: Uuid,
) -> Result<(), PortError> {
    super::command_actions::validate(catalog, route, default_source)?;
    let source = route.source_id.unwrap_or(default_source);
    let options = route.routing.as_ref();
    let source_binding = options.and_then(|o| o.source.as_ref());
    if let Some(c) = catalog.connections.iter().find(|c| c.id == source) {
        adapter(&c.config).validate_source(source_binding)?;
    } else if source_binding.is_some() {
        return Err(PortError::InvalidInput {
            code: "unsupported_route_binding",
        });
    }
    if let Some(options) = options
        && (options.targets.len() > 32
            || options
                .targets
                .keys()
                .any(|id| !route.targets.iter().any(|t| t.destination_id == *id)))
    {
        return Err(PortError::InvalidInput {
            code: "unknown_route_binding_target",
        });
    }
    for target in &route.targets {
        let binding = options.and_then(|o| o.targets.get(&target.destination_id));
        if let Some(c) = catalog
            .connections
            .iter()
            .find(|c| c.send_id() == target.destination_id)
        {
            if !matches!(
                route.processor,
                Some(crate::domain::managed_route::RouteProcessor::Respond)
            ) && !matches!(&route.processor,Some(crate::domain::managed_route::RouteProcessor::InvokeCapability{tool_id,..}) if binding.is_none() && super::command_actions::definitions(&c.config).iter().any(|a|a.definition.id==*tool_id&&!a.target_required))
            {
                adapter(&c.config).validate_target(binding, c.id == source)?;
            }
        } else if binding.is_some() {
            return Err(PortError::InvalidInput {
                code: "unsupported_route_binding",
            });
        }
    }
    Ok(())
}
impl Bindings {
    fn source_matches(&self, r: &ManagedRoute, e: &EventEnvelope) -> bool {
        let Ok(catalog) = self.connections.read() else {
            return false;
        };
        let binding = r.routing.as_ref().and_then(|o| o.source.as_ref());
        match catalog
            .connections
            .iter()
            .find(|c| c.id == e.scope.connection_id)
        {
            Some(c) => c.enabled && adapter(&c.config).matches(binding, e.route_context.as_ref()),
            None => binding.is_none(),
        }
    }
    pub(super) fn target_binding(
        &self,
        e: &EventEnvelope,
        rule: &RouteRule,
    ) -> Result<Option<RouteBinding>, PortError> {
        let routes = self.routes.read().map_err(|_| PortError::Unavailable)?;
        let route = routes
            .routes
            .iter()
            .find(|r| {
                r.revision == rule.revision && r.targets.iter().any(|t| t.rule_id == rule.rule_id)
            })
            .ok_or(PortError::NotFound)?;
        let catalog = self
            .connections
            .read()
            .map_err(|_| PortError::Unavailable)?;
        let binding = route
            .routing
            .as_ref()
            .and_then(|o| o.targets.get(&rule.destination_id));
        match catalog
            .connections
            .iter()
            .find(|c| c.send_id() == rule.destination_id)
        {
            Some(c) => adapter(&c.config).resolve_target(binding, e, c.id == e.scope.connection_id),
            None => Ok(None),
        }
    }
}
