//! Durable command execution: queue references, reload trusted state, invoke, then queue a reply.
//! Delivery holds connection_io.read; edits take its write side through publication.
use super::{bindings::Bindings, message_connections::ConnectionSnapshot};
use crate::{
    application::{
        ports::{
            OperationContext, PortError, PortFuture,
            connection_actions::{ConnectionAction, ConnectionActions},
            delivery::{DeliveryOutbox, DestinationRepository},
            identity::{AccessContext, PolicyEngine},
            messages::MessageSink,
            routing::{EventReader, RoutingRules},
        },
        services::{
            governance::Governance,
            invocation::{InvocationRequest, InvocationService},
        },
    },
    domain::{
        connection_routing::RouteBinding,
        connector::ConnectionScope,
        delivery::*,
        identity::{Action, Identity, Resource},
        managed_route::{ManagedRoute, RouteProcessor, RouteResponse},
        message::*,
        routing::RouteRule,
    },
    infrastructure::{
        connectors::{
            message_config::{ConnectionConfig, invalid},
            routing::adapter,
        },
        storage::SqliteStore,
        tool_runtime::schema::Schemas,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CommandJob {
    pub route_id: Uuid,
    pub revision: u64,
    pub source: ConnectionScope,
    pub event_id: Uuid,
}
impl CommandJob {
    pub fn message(&self) -> Result<Message, PortError> {
        Ok(Message {
            destination_binding: Some(RouteBinding::new(
                "synquay.command",
                serde_json::to_value(self).map_err(|_| PortError::Unavailable)?,
            )),
            parts: vec![],
            reply_route_id: None,
        })
    }
    fn decode(m: &Message) -> Result<Option<Self>, PortError> {
        let Some(b) = m
            .destination_binding
            .as_ref()
            .filter(|b| b.adapter == "synquay.command")
        else {
            return Ok(None);
        };
        if b.version != 1 || !m.parts.is_empty() || m.reply_route_id.is_some() {
            return Err(PortError::ScopeMismatch);
        }
        serde_json::from_value(b.config.clone())
            .map(Some)
            .map_err(|_| PortError::ScopeMismatch)
    }
}
pub(super) fn definitions(config: &ConnectionConfig) -> Vec<ConnectionAction> {
    match config {
        ConnectionConfig::QqBot(_) => crate::infrastructure::connectors::qq::actions::catalog(),
        _ => vec![],
    }
}
pub(super) fn validate(
    catalog: &ConnectionSnapshot,
    r: &ManagedRoute,
    default_source: Uuid,
) -> Result<(), PortError> {
    if matches!(r.processor, Some(RouteProcessor::Respond)) {
        let source = catalog
            .connections
            .iter()
            .find(|c| c.id == r.source_id.unwrap_or(default_source))
            .ok_or(invalid("route_reply_unavailable"))?;
        if !adapter(&source.config).supports_reply()
            || r.response != Some(RouteResponse::Source)
            || r.targets.len() != 1
            || r.targets[0].destination_id != source.send_id()
            || !matches!(
                r.command.as_ref().map(|c| &c.output),
                Some(crate::domain::command::CommandOutput::Json { .. })
            )
        {
            return Err(invalid("invalid_direct_response"));
        }
        return Ok(());
    }
    if r.response == Some(RouteResponse::Source)
        && !matches!(r.processor, Some(RouteProcessor::InvokeCapability { .. }))
    {
        return Err(invalid("route_reply_unavailable"));
    }
    let Some(RouteProcessor::InvokeCapability {
        tool_id,
        revision,
        allowed_senders,
    }) = &r.processor
    else {
        return Ok(());
    };
    if r.targets.len() != 1
        || !matches!(
            r.command.as_ref().map(|c| &c.output),
            Some(crate::domain::command::CommandOutput::Json { .. })
        )
    {
        return Err(invalid("invalid_command_action"));
    }
    let source = catalog
        .connections
        .iter()
        .find(|c| c.id == r.source_id.unwrap_or(default_source))
        .ok_or(invalid("command_sender_unavailable"))?;
    adapter(&source.config)
        .validate_command_senders(allowed_senders)
        .map_err(|_| invalid("command_sender_unavailable"))?;
    let target = catalog
        .connections
        .iter()
        .find(|c| c.send_id() == r.targets[0].destination_id)
        .ok_or(invalid("unknown_connection_action"))?;
    if !definitions(&target.config)
        .iter()
        .any(|a| a.revision == *revision && a.definition.id == *tool_id)
    {
        return Err(invalid("unknown_connection_action"));
    }
    if definitions(&target.config)
        .iter()
        .any(|a| a.definition.id == *tool_id && a.target_required)
        && r.routing
            .as_ref()
            .and_then(|o| o.targets.get(&target.send_id()))
            .is_none()
    {
        return Err(invalid("route_target_selection_required"));
    }
    if r.response == Some(RouteResponse::Source) && source.id != target.id {
        return Err(invalid("command_reply_requires_same_connection"));
    }
    Ok(())
}
pub(super) struct CommandRuntime {
    pub bindings: Arc<Bindings>,
    pub store: Arc<SqliteStore>,
}
pub(super) struct ActionSink {
    pub runtime: Arc<CommandRuntime>,
    pub inner: Arc<dyn MessageSink>,
    pub actions: Arc<dyn ConnectionActions>,
    pub scope: ConnectionScope,
    pub validator: Arc<Schemas>,
}
impl ActionSink {
    fn route(&self, j: &CommandJob) -> Result<ManagedRoute, PortError> {
        if j.source.workspace_id != self.scope.workspace_id {
            return Err(PortError::ScopeMismatch);
        }
        self.runtime
            .bindings
            .routes
            .read()
            .map_err(|_| PortError::Unavailable)?
            .routes
            .iter()
            .find(|r| {
                r.id == j.route_id
                    && r.enabled
                    && r.revision == j.revision
                    && r.source_id
                        .unwrap_or(self.runtime.bindings.source.connection_id)
                        == j.source.connection_id
                    && r.targets.len() == 1
                    && r.targets[0].destination_id == self.scope.connection_id
                    && matches!(r.processor, Some(RouteProcessor::InvokeCapability { .. }))
            })
            .cloned()
            .ok_or(PortError::PermissionDenied)
    }
    async fn execute(
        &self,
        mut operation: OperationContext,
        request: DeliveryRequest,
        j: CommandJob,
    ) -> Result<DeliveryReceipt, PortError> {
        let route = self.route(&j)?;
        let event = self.runtime.store.load(j.source, j.event_id).await?;
        let rule = RouteRule {
            workspace_id: self.scope.workspace_id,
            rule_id: route.targets[0].rule_id,
            revision: route.revision,
            destination_id: self.scope.connection_id,
        };
        if !self
            .runtime
            .bindings
            .matching(&event)
            .await?
            .iter()
            .any(|r| r.rule_id == rule.rule_id && r.revision == rule.revision)
        {
            return Err(PortError::PermissionDenied);
        }
        let RouteProcessor::InvokeCapability {
            tool_id,
            revision,
            allowed_senders,
        } = route
            .processor
            .as_ref()
            .ok_or(PortError::PermissionDenied)?
        else {
            return Err(PortError::PermissionDenied);
        };
        let source = self
            .runtime
            .bindings
            .connections
            .read()
            .map_err(|_| PortError::Unavailable)?
            .connections
            .iter()
            .find(|c| c.id == event.scope.connection_id && c.enabled)
            .cloned()
            .ok_or(PortError::PermissionDenied)?;
        let sender = adapter(&source.config).authenticated_sender(event.route_context.as_ref())?;
        let principal = stable_id(&json!([
            "connection-sender",
            event.scope,
            sender.namespace,
            sender.id
        ]));
        let allow = sender.allowed && allowed_senders.contains(&sender.id);
        operation.principal_id = principal;
        let access = AccessContext {
            identity: Identity {
                mcp_endpoint: None,
                workspace_id: self.scope.workspace_id,
                principal_id: principal,
                client_id: None,
                scopes: Default::default(),
                expires_at: None,
            },
            operation,
        };
        let governance = Arc::new(Governance::new(
            Arc::new(CommandGrant {
                principal,
                scope: self.scope,
                tool: tool_id.clone(),
                allow,
            }),
            self.runtime.store.clone(),
        ));
        // Authorize before mapping or constructing an executor; never trust payload.sender_openid.
        governance
            .authorize(
                &access,
                Action::InvokeTool,
                &Resource::Tool {
                    scope: self.scope,
                    tool_id: tool_id.clone(),
                },
            )
            .await?;
        let mapping = route.command.as_ref().ok_or(PortError::PermissionDenied)?;
        let captured = mapping
            .capture(&event.payload)
            .ok_or(PortError::PermissionDenied)?;
        let (_, arguments) = mapping
            .render(&event.payload, &captured)
            .map_err(|_| invalid("command_mapping_failed"))?
            .ok_or(invalid("invalid_command_action"))?;
        let action = self
            .actions
            .definitions()
            .into_iter()
            .find(|a| a.definition.id == *tool_id && a.revision == *revision)
            .ok_or(PortError::NotFound)?;
        let target = if !action.target_required {
            None
        } else {
            self.runtime.bindings.target_binding(&event, &rule)?
        };
        let invocation_id = stable_id(&json!([
            "command-invocation",
            event.scope,
            event.event_id,
            route.id
        ]));
        let report=InvocationService{validator:self.validator.clone(),governance,ledger:Some(self.runtime.store.clone())}.invoke(&access,InvocationRequest{
            scope:self.scope,definition:action.definition,executor:self.actions.executor(target.clone())?,invocation_id,arguments:arguments.clone(),
            intent:json!({"route":route.id,"revision":route.revision,"event":event.event_id,"scope":self.scope,"tool":tool_id,"capability_revision":revision,"target":target,"arguments":arguments}),
        }).await?;
        if !report.audit_recorded {
            return Err(PortError::OutcomeUnknown);
        }
        if route.response == Some(RouteResponse::Source) {
            let text = match &report.outcome {
                Ok(result) => result
                    .content
                    .iter()
                    .filter_map(|p| {
                        if let ContentPart::Text(t) = p {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(PortError::OutcomeUnknown) => "执行结果不确定，请核实后再操作。".into(),
                Err(_) => "操作未完成，请查看执行记录。".into(),
            };
            let message = adapter(&source.config).command_reply(
                &event,
                reply_text(&text),
                if action.consumes_reply {
                    target.as_ref()
                } else {
                    None
                },
            )?;
            let reply_id = stable_id(&json!(["command-result-reply", invocation_id]));
            let destination = self
                .runtime
                .bindings
                .resolve(self.scope.workspace_id, source.send_id())
                .await?;
            self.runtime
                .store
                .enqueue(DispatchPlan {
                    workspace_id: self.scope.workspace_id,
                    key: DispatchKey::Reply {
                        workspace_id: self.scope.workspace_id,
                        reply_id,
                    },
                    deliveries: vec![PendingDelivery {
                        delivery_id: reply_id,
                        route_rule: Some((rule.rule_id, rule.revision)),
                        destination,
                        message,
                        correlation_id: event.correlation_id,
                        causation_id: event.event_id,
                        route_trace: event.route_trace.clone(),
                    }],
                })
                .await
                .map_err(|_| PortError::OutcomeUnknown)?;
        }
        let result = report.outcome?;
        if result.is_error {
            return Err(PortError::Failed {
                code: "command_tool_failed",
            });
        }
        Ok(DeliveryReceipt {
            delivery_id: request.delivery_id,
            external_message_id: Some(invocation_id.to_string()),
            status: DeliveryStatus::Accepted,
        })
    }
}
impl MessageSink for ActionSink {
    fn validate(&self, m: &Message) -> Result<(), PortError> {
        match CommandJob::decode(m)? {
            Some(j) => self.route(&j).map(|_| ()),
            None => self.inner.validate(m),
        }
    }
    fn deliver(
        &self,
        ctx: OperationContext,
        r: DeliveryRequest,
    ) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != self.scope || r.destination_id != self.scope.connection_id {
                return Err(PortError::ScopeMismatch);
            }
            match CommandJob::decode(&r.message)? {
                Some(j) => self.execute(ctx, r, j).await,
                None => self.inner.deliver(ctx, r).await,
            }
        })
    }
}
struct CommandGrant {
    principal: Uuid,
    scope: ConnectionScope,
    tool: String,
    allow: bool,
}
impl PolicyEngine for CommandGrant {
    fn authorize(&self, i: Identity, a: Action, r: Resource) -> PortFuture<'_, ()> {
        Box::pin(async move {
            if self.allow
                && i.workspace_id == self.scope.workspace_id
                && i.principal_id == self.principal
                && a == Action::InvokeTool
                && r == (Resource::Tool {
                    scope: self.scope,
                    tool_id: self.tool.clone(),
                })
            {
                Ok(())
            } else {
                Err(PortError::PermissionDenied)
            }
        })
    }
}
fn stable_id(value: &serde_json::Value) -> Uuid {
    let hash = ring::digest::digest(&ring::digest::SHA256, value.to_string().as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash.as_ref()[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}
fn reply_text(s: &str) -> String {
    if s.trim().is_empty() {
        return "操作已完成。".into();
    }
    if s.len() <= 4000 {
        return s.into();
    }
    let mut end = 3997;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests;
