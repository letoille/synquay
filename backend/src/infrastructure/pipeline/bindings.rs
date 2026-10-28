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
                        && (if matches!(
                            r.processor,
                            Some(crate::domain::managed_route::RouteProcessor::CommandSet { .. })
                        ) {
                            selected_command(r, event).is_some()
                        } else {
                            r.command
                                .as_ref()
                                .is_some_and(|c| c.capture(&event.payload).is_some())
                        })
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
                        && if matches!(
                            r.processor,
                            Some(crate::domain::managed_route::RouteProcessor::CommandSet { .. })
                        ) {
                            selected_command(r, event).is_some()
                        } else {
                            r.command
                                .as_ref()
                                .is_none_or(|c| c.capture(&event.payload).is_some())
                        }
                        && r.event_type
                            .as_ref()
                            .is_none_or(|kind| *kind == event.event_type)
                })
                .flat_map(|r| {
                    let selected = selected_command(r, event).map(|c| c.id);
                    r.targets
                        .iter()
                        .filter(move |t| match &r.processor {
                            Some(crate::domain::managed_route::RouteProcessor::CommandSet {
                                ..
                            }) if t.branch == crate::domain::managed_route::RouteBranch::Action => {
                                selected.is_some_and(|id| {
                                    t.rule_id
                                        == super::command_actions::command_rule(r.id, id, false)
                                })
                            }
                            Some(crate::domain::managed_route::RouteProcessor::CommandSet {
                                ..
                            }) if t.branch == crate::domain::managed_route::RouteBranch::Reply => {
                                selected.is_some_and(|id| {
                                    t.rule_id
                                        == super::command_actions::command_rule(r.id, id, true)
                                })
                            }
                            _ => true,
                        })
                        .map(|t| RouteRule {
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
fn selected_command<'a>(
    route: &'a crate::domain::managed_route::ManagedRoute,
    event: &EventEnvelope,
) -> Option<&'a crate::domain::managed_route::RouteCommand> {
    match &route.processor {
        Some(crate::domain::managed_route::RouteProcessor::CommandSet { commands, .. }) => commands
            .iter()
            .find(|c| c.mapping.capture(&event.payload).is_some()),
        _ => None,
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
                        && (matches!(
                            r.processor,
                            Some(crate::domain::managed_route::RouteProcessor::Respond)
                        ) || r.targets.iter().any(|t| {
                            t.rule_id == rule.rule_id
                                && t.branch == crate::domain::managed_route::RouteBranch::Reply
                        }))
                })
                .cloned();
            if let Some(r) = response_route {
                let selected = selected_command(&r, event);
                let mapping = selected
                    .map(|c| &c.mapping)
                    .or(r.command.as_ref())
                    .ok_or(PortError::PermissionDenied)?;
                let captured = mapping
                    .capture(&event.payload)
                    .ok_or(PortError::PermissionDenied)?;
                let text = if let Some(crate::domain::managed_route::RouteReply::Fixed { text }) =
                    selected.and_then(|c| c.reply.as_ref())
                {
                    text.clone()
                } else if let Some(crate::domain::managed_route::RouteProcessor::Combined {
                    reply: Some(crate::domain::managed_route::RouteReply::Fixed { text }),
                    ..
                }) = &r.processor
                {
                    text.clone()
                } else {
                    let (_, value) = mapping
                        .render(&event.payload, &captured)
                        .map_err(|_| PortError::InvalidInput {
                            code: "command_mapping_failed",
                        })?
                        .ok_or(PortError::Unsupported)?;
                    value["content"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty() && s.len() <= 4000)
                        .ok_or(PortError::InvalidInput {
                            code: "invalid_direct_response",
                        })?
                        .to_owned()
                };
                let catalog = self
                    .connections
                    .read()
                    .map_err(|_| PortError::Unavailable)?;
                let source = catalog
                    .connections
                    .iter()
                    .find(|c| c.id == event.scope.connection_id && c.enabled)
                    .ok_or(PortError::PermissionDenied)?;
                let selected_action = selected.map(|c| &c.action);
                let combined_action =
                    if let Some(crate::domain::managed_route::RouteProcessor::Combined {
                        action: Some(action),
                        ..
                    }) = &r.processor
                    {
                        Some(action.as_ref())
                    } else {
                        None
                    };
                let consumed = if let Some(action) = selected_action.or(combined_action) {
                    if action.destination_id == source.send_id()
                        && super::command_actions::definitions(&source.config)
                            .iter()
                            .any(|a| a.definition.id == action.tool_id && a.consumes_reply)
                    {
                        adapter(&source.config).resolve_target(
                            r.routing
                                .as_ref()
                                .and_then(|o| o.targets.get(&action.destination_id)),
                            event,
                            true,
                        )?
                    } else {
                        None
                    }
                } else {
                    None
                };
                return adapter(&source.config).command_reply(event, text, consumed.as_ref());
            }
            let action_route=self.routes.read().map_err(|_|PortError::Unavailable)?.routes.iter().find(|r|
                r.revision==rule.revision && r.targets.iter().any(|t|t.rule_id==rule.rule_id)
                && (matches!(r.processor,Some(crate::domain::managed_route::RouteProcessor::InvokeCapability{..})) || r.targets.iter().any(|t|t.rule_id==rule.rule_id && t.branch==crate::domain::managed_route::RouteBranch::Action))).map(|r|(r.id,matches!(r.processor,Some(crate::domain::managed_route::RouteProcessor::CommandSet{..}))));
            if let Some((route_id, is_command_set)) = action_route {
                return super::command_actions::CommandJob {
                    route_id,
                    revision: rule.revision,
                    source: event.scope,
                    event_id: event.event_id,
                    rule_id: is_command_set.then_some(rule.rule_id),
                }
                .message();
            }
            let destination_binding = self.target_binding(event, rule)?;
            // Reserve sequence 3 for a forward back to the same QQ conversation when
            // this route may also issue an action and a reply (sequences 1 and 2).
            let sequence = destination_binding.as_ref().is_some_and(|b| b.adapter == "qq" && b.config.get("msg_id").is_some()) && self.routes.read().map_err(|_| PortError::Unavailable)?.routes.iter().any(|r| {
                r.revision == rule.revision && r.targets.iter().any(|t| t.rule_id == rule.rule_id && t.branch == crate::domain::managed_route::RouteBranch::Forward)
                && (matches!(&r.processor, Some(crate::domain::managed_route::RouteProcessor::Combined { action, reply, .. }) if action.is_some() || reply.is_some()) || matches!(r.processor,Some(crate::domain::managed_route::RouteProcessor::CommandSet{..})))
            });
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
                .and_then(|r| {
                    selected_command(r, event)
                        .map(|c| c.mapping.clone())
                        .or_else(|| r.command.clone())
                });
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
                    let mut message = Message {
                        destination_binding: destination_binding.clone(),
                        parts: vec![ContentPart::Json(serde_json::json!({
                            "event_id": event.event_id, "source_event_id": event.source_event_id,
                            "event_type": event_type, "payload": payload
                        }))],
                        reply_route_id: None,
                    };
                    set_forward_sequence(&mut message, sequence);
                    return Ok(message);
                }
            }
            let mut message = match &event.payload {
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
            }?;
            set_forward_sequence(&mut message, sequence);
            Ok(message)
        })
    }
}

fn set_forward_sequence(message: &mut Message, enabled: bool) {
    if !enabled {
        return;
    }
    if let [ContentPart::Json(value)] = message.parts.as_mut_slice() {
        let payload = if value.get("payload").is_some() {
            &mut value["payload"]
        } else {
            value
        };
        if let Some(fields) = payload.as_object_mut() {
            fields.insert("msg_seq".into(), serde_json::json!(3));
        }
    } else if let [ContentPart::Text(text)] = message.parts.as_slice() {
        message.parts = vec![ContentPart::Json(
            serde_json::json!({"content": text, "msg_seq": 3}),
        )];
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
            let combined_special = match &route.processor {
                Some(crate::domain::managed_route::RouteProcessor::Combined { action, .. }) => {
                    match target.branch {
                        crate::domain::managed_route::RouteBranch::Reply => true,
                        crate::domain::managed_route::RouteBranch::Action => {
                            binding.is_none()
                                && action.as_ref().is_some_and(|action| {
                                    super::command_actions::definitions(&c.config)
                                        .iter()
                                        .any(|a| {
                                            a.definition.id == action.tool_id && !a.target_required
                                        })
                                })
                        }
                        _ => false,
                    }
                }
                Some(crate::domain::managed_route::RouteProcessor::CommandSet {
                    commands, ..
                }) => match target.branch {
                    crate::domain::managed_route::RouteBranch::Reply => true,
                    crate::domain::managed_route::RouteBranch::Action => {
                        binding.is_none()
                            && commands
                                .iter()
                                .find(|cmd| {
                                    super::command_actions::command_rule(route.id, cmd.id, false)
                                        == target.rule_id
                                })
                                .is_some_and(|cmd| {
                                    super::command_actions::definitions(&c.config)
                                        .iter()
                                        .any(|a| {
                                            a.definition.id == cmd.action.tool_id
                                                && !a.target_required
                                        })
                                })
                    }
                    _ => false,
                },
                _ => false,
            };
            if !combined_special
                && !matches!(
                    route.processor,
                    Some(crate::domain::managed_route::RouteProcessor::Respond)
                )
                && !matches!(&route.processor,Some(crate::domain::managed_route::RouteProcessor::InvokeCapability{tool_id,..}) if binding.is_none() && super::command_actions::definitions(&c.config).iter().any(|a|a.definition.id==*tool_id&&!a.target_required))
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
