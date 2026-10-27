//! Durable connection catalog and runtime publication under the routing/I/O gates.
use super::*;
use crate::{
    application::ports::{
        PortFuture,
        events::{EventSource, SourceExit},
        messages::MessageSink,
    },
    domain::{event::IngestReceipt, message::*},
    infrastructure::connectors::{
        http_client::HttpClientSink,
        message_config::*,
        mqtt::MqttConnector,
        qq::{QqBotConfig, QqBotSink},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ConnectionSnapshot {
    pub revision: u64,
    pub connections: Vec<MessageConnection>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionInput {
    /// Write-only QQ credential; omitted or empty means retain the saved credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    pub operation_id: Uuid,
    pub expected_revision: u64,
    pub connection_id: Uuid,
    pub connection: Option<MessageConnection>,
}

pub(super) fn registration(
    workspace: Uuid,
    c: &MessageConnection,
    commands: Arc<super::command_actions::CommandRuntime>,
) -> Result<Option<ConnectorRegistration>, PortError> {
    c.validate()?;
    // Validate receive-only MQTT TLS and credential references too.
    if let ConnectionConfig::Mqtt(config) = &c.config {
        MqttConnector::new(
            ConnectionScope {
                workspace_id: workspace,
                connection_id: c.id,
            },
            c.send_id(),
            config,
        )?;
    }
    if !c.sends() {
        return Ok(None);
    }
    let scope = ConnectionScope {
        workspace_id: workspace,
        connection_id: c.send_id(),
    };
    let sink: Arc<dyn MessageSink> = match &c.config {
        ConnectionConfig::HttpClient(config) => {
            Arc::new(HttpClientSink::new(scope, c.send_id(), config)?)
        }
        ConnectionConfig::Mqtt(config) => {
            let actions: Arc<dyn crate::application::ports::connection_actions::ConnectionActions> =
                Arc::new(
                    crate::infrastructure::connectors::mqtt::actions::MqttActions {
                        scope,
                        owner: c.id,
                        config: config.clone(),
                        store: commands.store.clone(),
                    },
                );
            let validator = Arc::new(
                crate::infrastructure::tool_runtime::schema::Schemas::compile(
                    actions
                        .definitions()
                        .into_iter()
                        .map(|d| d.definition.input_schema),
                )?,
            );
            Arc::new(super::command_actions::ActionSink {
                runtime: commands,
                inner: Arc::new(MqttConnector::new(scope, c.send_id(), config)?),
                actions,
                scope,
                validator,
            })
        }
        ConnectionConfig::QqBot(config) => {
            let sink = Arc::new(QqBotSink::new(scope, c.send_id(), config)?);
            let actions: Arc<dyn crate::application::ports::connection_actions::ConnectionActions> =
                Arc::new(crate::infrastructure::connectors::qq::actions::QqActions {
                    sink: sink.clone(),
                    destination_id: c.send_id(),
                });
            let validator = Arc::new(
                crate::infrastructure::tool_runtime::schema::Schemas::compile(
                    actions.definitions().into_iter().flat_map(|d| {
                        std::iter::once(d.definition.input_schema).chain(d.definition.output_schema)
                    }),
                )?,
            );
            Arc::new(super::command_actions::ActionSink {
                runtime: commands,
                inner: sink,
                actions,
                scope,
                validator,
            })
        }
        _ => return Err(PortError::Unsupported),
    };
    let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
        scope,
        kind: if matches!(c.config, ConnectionConfig::QqBot(_)) {
            ConnectorKind::Custom("qq".into())
        } else if matches!(c.config, ConnectionConfig::Mqtt(_)) {
            ConnectorKind::Mqtt
        } else {
            ConnectorKind::Http
        },
        display_name: c.name.clone(),
    });
    registration.sink = Some(Arc::new(EnabledSink {
        enabled: c.enabled,
        inner: sink,
    }));
    Ok(Some(registration))
}
struct EnabledSink {
    enabled: bool,
    inner: Arc<dyn MessageSink>,
}
impl MessageSink for EnabledSink {
    fn validate(&self, m: &Message) -> Result<(), PortError> {
        if !self.enabled {
            return Err(PortError::Unavailable);
        }
        self.inner.validate(m)
    }
    fn deliver(&self, c: OperationContext, r: DeliveryRequest) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            if !self.enabled {
                return Err(PortError::Unavailable);
            }
            self.inner.deliver(c, r).await
        })
    }
}
impl Pipeline {
    pub fn qq_settings(&self, id: Uuid) -> Result<QqBotConfig, PortError> {
        let snapshot = self.connection_snapshot()?;
        match snapshot
            .connections
            .into_iter()
            .find(|c| c.id == id && c.enabled)
            .map(|c| c.config)
        {
            Some(ConnectionConfig::QqBot(c)) => Ok(c),
            _ => Err(PortError::NotFound),
        }
    }

    pub fn connection_snapshot(&self) -> Result<ConnectionSnapshot, PortError> {
        self.connections
            .read()
            .map(|s| s.clone())
            .map_err(|_| PortError::Unavailable)
    }
    pub(crate) fn all_sources(&self) -> Vec<routes::RouteSource> {
        let mut sources = self.sources.clone();
        if let Ok(s) = self.connections.read() {
            sources.extend(s.connections.iter().filter(|c| c.receives()).map(|c| {
                routes::RouteSource {
                    routing: Some(
                        crate::infrastructure::connectors::routing::adapter(&c.config)
                            .source_descriptor(),
                    ),
                    connection_id: c.id,
                    name: c.name.clone(),
                    address: match &c.config {
                        ConnectionConfig::Mqtt(m) => format!(
                            "{}/{}",
                            c.address(),
                            m.subscription
                                .as_ref()
                                .map(|s| s.topic.as_str())
                                .unwrap_or("")
                        ),
                        _ => c.address(),
                    },
                    protocol: c.protocol().into(),
                }
            }));
        }
        sources
    }
    pub(crate) fn all_targets(&self) -> Vec<routes::RouteEndpoint> {
        let mut targets = self.route_endpoints.clone();
        if let Ok(s) = self.connections.read() {
            targets.extend(s.connections.iter().filter(|c| c.sends()).map(|c| {
                routes::RouteEndpoint {
                    actions: super::command_actions::definitions(&c.config),
                    owner_connection_id: Some(c.id),
                    routing: Some(
                        crate::infrastructure::connectors::routing::adapter(&c.config)
                            .target_descriptor(),
                    ),
                    connection_id: c.send_id(),
                    destination_id: c.send_id(),
                    address: match &c.config {
                        ConnectionConfig::Mqtt(m) => format!(
                            "{}/{}",
                            c.address(),
                            m.publish.as_ref().map(|p| p.topic.as_str()).unwrap_or("")
                        ),
                        _ => c.address(),
                    },
                    protocol: c.protocol().into(),
                    name: c.name.clone(),
                }
            }));
        }
        targets
    }
    pub(crate) fn has_source(&self, id: Uuid) -> bool {
        self.ingresses.contains_key(&id)
            || self
                .connections
                .read()
                .is_ok_and(|s| s.connections.iter().any(|c| c.id == id && c.receives()))
    }
    pub fn webhook_settings(
        &self,
        id: Uuid,
    ) -> Result<(webhook::WebhookConfig, String), PortError> {
        if self.ingresses.contains_key(&id) {
            return Ok((self.webhook.clone(), self.token.clone()));
        }
        let s = self
            .connections
            .read()
            .map_err(|_| PortError::Unavailable)?;
        let c = s
            .connections
            .iter()
            .find(|c| c.id == id && c.enabled)
            .ok_or(PortError::NotFound)?;
        match &c.config {
            ConnectionConfig::Webhook { secret_env, policy } => {
                Ok((policy.clone(), secret(secret_env)?))
            }
            _ => Err(PortError::NotFound),
        }
    }
    pub async fn change_connection(
        self: &Arc<Self>,
        actor: Uuid,
        input: ConnectionInput,
    ) -> Result<u64, PortError> {
        if self.agent.is_some() {
            return Err(PortError::Unsupported);
        }
        if actor.is_nil() || input.operation_id.is_nil() || input.connection_id.is_nil() {
            return Err(invalid("invalid_connection_id"));
        }
        let permit = self.try_admit_admin()?;
        let p = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _routing = p.routing_lane.lock().await;
            let _io = p.connection_io.write().await;
            let _devices = p.store.device_io.write().await;
            if let Some(revision) = p
                .store
                .connection_operation(p.source, actor, input.clone())
                .await?
            {
                return Ok(revision);
            }
            let mut next = p.connection_snapshot()?;
            if input.expected_revision != next.revision {
                return Err(PortError::Conflict);
            }
            let old = next
                .connections
                .iter()
                .find(|c| c.id == input.connection_id)
                .cloned();
            let mut effective = input.connection.clone();
            if let Some(c) = effective.as_mut() {
                if let ConnectionConfig::QqBot(qq) = &mut c.config {
                    if let Some(value) = input.secret.as_ref().filter(|v| !v.is_empty()) {
                        if value.len() > 4096 || !value.bytes().all(|b| b.is_ascii_graphic()) {
                            return Err(invalid("invalid_qq_secret"));
                        }
                        qq.resolved_secret = Some(Arc::from(value.as_str()));
                    } else if let Some(ConnectionConfig::QqBot(prior)) = old.as_ref().map(|o| &o.config) {
                        if prior.app_id != qq.app_id { return Err(invalid("qq_secret_required")); }
                        qq.resolved_secret = prior.resolved_secret.clone();
                    } else {
                        return Err(invalid("qq_secret_required"));
                    }
                } else if input.secret.is_some() { return Err(invalid("unsupported_connection_secret")); }
            } else if input.secret.is_some() { return Err(invalid("unsupported_connection_secret")); }
            let prepared = if let Some(c) = &effective {
                if c.id != input.connection_id {
                    return Err(PortError::ScopeMismatch);
                }
                if p.sources
                    .iter()
                    .any(|s| [c.id, c.send_id()].contains(&s.connection_id))
                    || p.route_endpoints.iter().any(|t| {
                        [c.id, c.send_id()].contains(&t.connection_id)
                            || [c.id, c.send_id()].contains(&t.destination_id)
                    })
                    || next
                        .connections
                        .iter()
                        .any(|o| o.id != c.id && o.send_id() == c.id)
                {
                    return Err(PortError::Conflict);
                }
                if let ConnectionConfig::QqBot(qq) = &c.config
                    && next.connections.iter().any(|other| other.id != c.id
                        && matches!(&other.config,ConnectionConfig::QqBot(existing) if existing.app_id == qq.app_id))
                { return Err(invalid("duplicate_qq_app")); }
                let prepared = registration(p.source.workspace_id, c, p.commands.clone())?;
                if let Some(index) = next.connections.iter().position(|v| v.id == c.id) {
                    next.connections[index] = c.clone();
                } else {
                    next.connections.push(c.clone());
                }
                prepared
            } else {
                if old.is_none() {
                    return Err(PortError::NotFound);
                }
                next.connections.retain(|c| c.id != input.connection_id);
                None
            };
            if next.connections.len() > 32 {
                return Err(invalid("connection_limit"));
            }
            let mut routes = p
                .bindings
                .routes
                .read()
                .map_err(|_| PortError::Unavailable)?
                .clone();
            if let Some(old) = &old {
                let lost_receive = input.connection.as_ref().is_none_or(|c| !c.receives());
                let lost_send = input.connection.as_ref().is_none_or(|c| !c.sends());
                if lost_receive || lost_send {
                    routes.routes.retain_mut(|r| {
                        if lost_receive && r.source_id == Some(old.id) {
                            return false;
                        }
                        if lost_send {
                            r.targets.retain(|t| t.destination_id != old.send_id());
                            if let Some(binding) = &mut r.routing { binding.targets.remove(&old.send_id()); }
                        }
                        if r.targets.is_empty() {
                            return false;
                        }
                        true
                    });
                    routes.revision += 1;
                    for r in &mut routes.routes {
                        r.revision = routes.revision;
                    }
                }
            }
            if let Some(c) = &input.connection
                && !matches!(c.config, ConnectionConfig::HttpClient(_))
                && routes.routes.iter().any(|r| {
                    matches!(
                        r.processor,
                        Some(crate::domain::managed_route::RouteProcessor::AgentHttp { .. })
                    ) && r.targets.iter().any(|t| t.destination_id == c.send_id())
                })
            {
                return Err(invalid("invalid_agent_http_target"));
            }
            if let Some(old)=&old {
                let actions=input.connection.as_ref().map(|c|super::command_actions::definitions(&c.config)).unwrap_or_default();
                let mut touched=false;
                routes.routes.retain_mut(|r| {
                    if !r.targets.iter().any(|t|t.destination_id==old.send_id()){return true}
                    if let Some(crate::domain::managed_route::RouteProcessor::InvokeCapability{tool_id,revision,..})=&mut r.processor {
                        if let Some(action)=actions.iter().find(|a|a.definition.id==*tool_id){
                            if *revision!=action.revision{*revision=action.revision;r.enabled=false;touched=true;}
                        }else{touched=true;return false}
                    }
                    true
                });
                if touched {routes.revision+=1;for r in &mut routes.routes{r.revision=routes.revision;}}
            }
            for route in &routes.routes {
                super::bindings::validate_routing(&next, route, p.source.connection_id)?;
            }
            next.revision += 1;
            p.store
                .save_message_connections(
                    p.source,
                    actor,
                    input.clone(),
                    next.clone(),
                    routes.clone(),
                    old.as_ref()
                        .filter(|old| {
                            input.connection.as_ref().is_none_or(|next| {
                                serde_json::to_value(&old.config).ok()
                                    != serde_json::to_value(&next.config).ok()
                            })
                        })
                        .map(|c| (c.id, c.send_id())),
                )
                .await?;
            let scope = ConnectionScope {
                workspace_id: p.source.workspace_id,
                connection_id: input
                    .connection
                    .as_ref()
                    .map(|c| c.send_id())
                    .or_else(|| old.as_ref().map(|c| c.send_id()))
                    .ok_or(PortError::OutcomeUnknown)?,
            };
            if p.registry.get(scope).is_ok() {
                p.registry.replace_shared(scope, prepared)?;
            } else if let Some(r) = prepared {
                p.registry.register_shared(r)?;
            }
            *p.connections
                .write()
                .map_err(|_| PortError::OutcomeUnknown)? = next.clone();
            *p.bindings
                .routes
                .write()
                .map_err(|_| PortError::OutcomeUnknown)? = routes;
            p.wake.notify_waiters();
            Ok(next.revision)
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)?
    }
    pub(super) async fn run_connections(self: Arc<Self>, stop: CancellationToken) {
        let mut sources = std::collections::HashMap::<
            Uuid,
            (
                String,
                CancellationToken,
                tokio::task::JoinHandle<Result<SourceExit, PortError>>,
            ),
        >::new();
        let mut deliveries = std::collections::HashMap::<
            Uuid,
            (CancellationToken, tokio::task::JoinHandle<()>),
        >::new();
        while !stop.is_cancelled() {
            let snapshot = match self.connection_snapshot() {
                Ok(s) => s,
                Err(_) => break,
            };
            if let Ok(mut statuses) = self.connection_status.write() {
                statuses.retain(|id, _| snapshot.connections.iter().any(|c| c.id == *id));
            }
            let desired: Vec<_> = snapshot
                .connections
                .iter()
                .filter(|c| c.enabled && matches!(&c.config, ConnectionConfig::Mqtt(_)))
                .collect();
            let remove: Vec<_> = sources
                .iter()
                .filter(|(id, (encoded, _, task))| {
                    task.is_finished()
                        || !desired.iter().any(|c| {
                            c.id == **id && serde_json::to_string(c).ok().as_ref() == Some(encoded)
                        })
                })
                .map(|(id, _)| *id)
                .collect();
            for id in remove {
                if let Some((_, token, task)) = sources.remove(&id) {
                    token.cancel();
                    let _ = task.await;
                }
            }
            for c in desired {
                if sources.contains_key(&c.id) {
                    continue;
                }
                let ConnectionConfig::Mqtt(config) = &c.config else {
                    continue;
                };
                let scope = ConnectionScope {
                    workspace_id: self.source.workspace_id,
                    connection_id: c.id,
                };
                match MqttConnector::new(scope, c.send_id(), config) {
                    Ok(driver) => {
                        if let Ok(mut statuses) = self.connection_status.write() {
                            statuses.insert(
                                c.id,
                                (
                                    serde_json::to_string(c).unwrap_or_default(),
                                    driver.runtime_status(),
                                ),
                            );
                        }
                        let token = stop.child_token();
                        let mut context = self.context(Uuid::new_v4(), token.clone()).operation;
                        context.scope = scope;
                        context.deadline = None;
                        let ingress: Arc<dyn EventIngress> = Arc::new(ManagedIngress {
                            pipeline: Arc::downgrade(&self),
                            id: c.id,
                            configuration: serde_json::to_string(c).unwrap_or_default(),
                        });
                        let task = tokio::spawn(async move { driver.run(context, ingress).await });
                        sources.insert(
                            c.id,
                            (serde_json::to_string(c).unwrap_or_default(), token, task),
                        );
                    }
                    Err(e) => {
                        use crate::infrastructure::connectors::runtime_status::{
                            ConnectionState, RuntimeStatus,
                        };
                        let status = RuntimeStatus::default();
                        status.update(
                            c.id,
                            ConnectionState::Error,
                            Some("mqtt_configuration_unavailable"),
                        );
                        if let Ok(mut statuses) = self.connection_status.write() {
                            statuses.insert(
                                c.id,
                                (serde_json::to_string(c).unwrap_or_default(), status),
                            );
                        }
                        tracing::warn!(event="connection.start_failed",connection=%c.id,error=%e)
                    }
                }
            }
            let ids: Vec<_> = snapshot
                .connections
                .iter()
                .filter(|c| c.enabled && c.sends())
                .map(|c| c.send_id())
                .collect();
            let removed: Vec<_> = deliveries
                .keys()
                .filter(|id| !ids.contains(id))
                .copied()
                .collect();
            for id in removed {
                if let Some((token, task)) = deliveries.remove(&id) {
                    token.cancel();
                    let _ = task.await;
                }
            }
            for id in ids {
                if deliveries.contains_key(&id) {
                    continue;
                }
                let p = self.clone();
                let token = stop.child_token();
                let task_token = token.clone();
                let task = tokio::spawn(async move {
                    let lane = scheduler::TargetLane::new(id);
                    while !task_token.is_cancelled() {
                        if let Err(e) = p.deliver_target(&lane, &task_token).await {
                            tracing::warn!(event="connection.delivery_failed",error=%e);
                        }
                        tokio::select! {biased;_=task_token.cancelled()=>break,_=p.wake.notified()=>{},_=tokio::time::sleep(Duration::from_secs(1))=>{}}
                    }
                });
                deliveries.insert(id, (token, task));
            }
            tokio::select! {biased;_=stop.cancelled()=>break,_=self.wake.notified()=>{},_=tokio::time::sleep(Duration::from_secs(1))=>{}}
        }
        for (_, (token, task)) in deliveries {
            token.cancel();
            let _ = task.await;
        }
        for (_, (_, token, task)) in sources {
            token.cancel();
            let _ = task.await;
        }
    }
}
struct ManagedIngress {
    pipeline: std::sync::Weak<Pipeline>,
    id: Uuid,
    configuration: String,
}
impl EventIngress for ManagedIngress {
    fn accept(&self, ctx: OperationContext, input: IncomingEvent) -> PortFuture<'_, IngestReceipt> {
        Box::pin(async move {
            ctx.check_active()?;
            let p = self.pipeline.upgrade().ok_or(PortError::Unavailable)?;
            let id = self.id;
            let configuration = self.configuration.clone();
            // Preserve the publication guard through durable acceptance even if the
            // subscriber or HTTP caller disconnects while SQLite is committing.
            tokio::spawn(async move {
                let _gate = p.connection_io.read().await;
                let c = p
                    .connection_snapshot()?
                    .connections
                    .into_iter()
                    .find(|c| c.id == id && c.enabled)
                    .ok_or(PortError::Unavailable)?;
                if serde_json::to_string(&c).ok().as_ref() != Some(&configuration) {
                    return Err(PortError::Conflict);
                }
                p.accept_from(id, ctx.request_id, input).await
            })
            .await
            .map_err(|_| PortError::OutcomeUnknown)?
        })
    }
}

impl Pipeline {
    pub async fn connection_conversations(
        &self,
        id: Uuid,
        q: crate::infrastructure::storage::conversations::ConversationQuery,
    ) -> Result<crate::infrastructure::storage::conversations::ConversationPage, PortError> {
        let c = self
            .connection_snapshot()?
            .connections
            .into_iter()
            .find(|c| c.id == id)
            .ok_or(PortError::NotFound)?;
        let account = crate::infrastructure::connectors::routing::adapter(&c.config)
            .discovery_account()
            .ok_or(PortError::Unsupported)?;
        self.store
            .conversations(self.source.workspace_id, id, account, q)
            .await
    }
}

impl Pipeline {
    pub async fn connection_observations(
        &self,
        snapshot: &ConnectionSnapshot,
    ) -> Result<serde_json::Value, PortError> {
        let received = self
            .store
            .message_totals(
                self.source.workspace_id,
                "received",
                snapshot.connections.iter().map(|c| c.id).collect(),
            )
            .await?;
        let sent = self
            .store
            .message_totals(
                self.source.workspace_id,
                "sent",
                snapshot.connections.iter().map(|c| c.send_id()).collect(),
            )
            .await?;
        let statuses = self
            .connection_status
            .read()
            .map_err(|_| PortError::Unavailable)?;
        let observations=snapshot.connections.iter().map(|c| {
            let status=if !c.enabled {serde_json::json!({"state":"stopped"})}
            else {match &c.config {
                ConnectionConfig::Mqtt(_)=>statuses.get(&c.id).filter(|(config,_)|serde_json::to_string(c).ok().as_ref()==Some(config)).map(|(_,s)|serde_json::json!(s.snapshot())).unwrap_or(serde_json::json!({"state":"connecting"})),
                ConnectionConfig::HttpClient(_)=>serde_json::json!({"state":"on_demand"}),
                _=>serde_json::json!({"state":"listening"}),
            }};
            (c.id.to_string(),serde_json::json!({"status":status,"received":received.get(&c.id).copied().unwrap_or(0),"sent":sent.get(&c.send_id()).copied().unwrap_or(0)}))
        }).collect::<serde_json::Map<_,_>>();
        Ok(serde_json::Value::Object(observations))
    }
}
