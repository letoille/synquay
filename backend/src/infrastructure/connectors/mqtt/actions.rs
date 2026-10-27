//! MQTT actions and state queries share one connection's trusted configuration.
use super::*;
use crate::{
    application::ports::{
        connection_actions::{ConnectionAction, ConnectionActions},
        tools::ToolExecutor,
    },
    domain::{capability::mqtt::*, tool::*},
    infrastructure::storage::SqliteStore,
};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct MqttActions {
    pub scope: ConnectionScope,
    pub owner: Uuid,
    pub config: MqttConfig,
    pub store: Arc<SqliteStore>,
}
pub fn catalog(config: &MqttConfig) -> Vec<ConnectionAction> {
    config
        .capabilities
        .definitions()
        .into_iter()
        .map(|definition| {
            // Definition + bindings version prevents queued commands being reinterpreted after edits.
            let bytes = serde_json::to_vec(config).unwrap_or_default();
            let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
            let revision = (u64::from_be_bytes(digest.as_ref()[..8].try_into().unwrap())
                & ((1u64 << 52) - 1))
                .max(1);
            ConnectionAction {
                revision,
                definition,
                target_required: false,
                consumes_reply: false,
            }
        })
        .collect()
}
impl ConnectionActions for MqttActions {
    fn definitions(&self) -> Vec<ConnectionAction> {
        catalog(&self.config)
    }
    fn executor(
        &self,
        target: Option<crate::domain::connection_routing::RouteBinding>,
    ) -> Result<Arc<dyn ToolExecutor>, PortError> {
        if target.is_some() {
            return Err(PortError::InvalidInput {
                code: "mqtt_tool_target_fixed",
            });
        }
        Ok(Arc::new(self.clone()))
    }
}
impl ToolExecutor for MqttActions {
    fn discover(&self, ctx: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>> {
        Box::pin(async move {
            ctx.check_active()?;
            Ok(self.config.capabilities.definitions())
        })
    }
    fn invoke(&self, ctx: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult> {
        Box::pin(async move {
            let _guard = self.store.device_io.read().await;
            ctx.check_active()?;
            if ctx.scope != self.scope {
                return Err(PortError::ScopeMismatch);
            }
            let owner_scope = ConnectionScope {
                workspace_id: self.scope.workspace_id,
                connection_id: self.owner,
            };
            let current = self.store.device_connection(owner_scope).await?;
            if !current.enabled
                || !matches!(&current.config,ConnectionConfig::Mqtt(c) if serde_json::to_value(c).ok()==serde_json::to_value(&self.config).ok())
            {
                return Err(PortError::Conflict);
            }
            let definition = self
                .config
                .capabilities
                .definitions()
                .into_iter()
                .find(|d| d.id == call.tool_id)
                .ok_or(PortError::NotFound)?;
            use crate::application::ports::profiles::SchemaValidator;
            crate::infrastructure::tool_runtime::schema::Schemas::compile([definition
                .input_schema
                .clone()])?
            .validate(&definition.input_schema, &call.arguments)
            .await?;
            let value = if let Some(resource) = self
                .config
                .capabilities
                .resources
                .iter()
                .find(|r| r.tool_id() == call.tool_id)
            {
                let selected = call.arguments["group"].as_str();
                if selected.is_some_and(|id| !resource.groups.iter().any(|g| g.id == id)) {
                    return Err(invalid("unknown_state_group"));
                }
                if call.arguments["refresh"] == true {
                    let group = match selected {
                        Some(id) => resource.groups.iter().find(|g| g.id == id),
                        None if resource.groups.len() == 1 => resource.groups.first(),
                        _ => None,
                    }
                    .ok_or(invalid("refresh_requires_group"))?;
                    let tool = self
                        .config
                        .capabilities
                        .tools
                        .iter()
                        .find(|t| Some(&t.id) == group.refresh_tool.as_ref())
                        .ok_or(invalid("state_refresh_unavailable"))?;
                    let schema =
                        crate::infrastructure::tool_runtime::schema::Schemas::compile([tool
                            .input_schema
                            .clone()])?;
                    schema.validate(&tool.input_schema, &json!({})).await?;
                    let result = self
                        .execute(&ctx, tool, &json!({}), call.invocation_id)
                        .await?;
                    if result["status"] != "confirmed" {
                        return Ok(result_tool(result, true));
                    }
                }
                let mut states = self.store.resource_states(owner_scope).await?;
                let mut state = states[&resource.id].take();
                if let Some(group) = selected {
                    let v = state["groups"][group].take();
                    state["groups"] = json!({group:v});
                }
                state
            } else {
                let tool = self
                    .config
                    .capabilities
                    .tools
                    .iter()
                    .find(|t| t.id == call.tool_id)
                    .ok_or(PortError::NotFound)?;
                self.execute(&ctx, tool, &call.arguments, call.invocation_id)
                    .await?
            };
            let failed = value["status"] == "unconfirmed" || value["status"] == "device_rejected";
            Ok(result_tool(value, failed))
        })
    }
}
fn result_tool(value: Value, is_error: bool) -> ToolResult {
    ToolResult {
        content: vec![ContentPart::Text(value.to_string())],
        structured_content: Some(value),
        is_error,
    }
}
impl MqttActions {
    async fn execute(
        &self,
        ctx: &OperationContext,
        tool: &MqttTool,
        args: &Value,
        id: Uuid,
    ) -> Result<Value, PortError> {
        static SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
        let _permit = SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(8)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| PortError::RateLimited {
                retry_after: Duration::from_secs(1),
            })?;
        let payload = tool
            .payload
            .render(args, id)
            .ok_or(invalid("invalid_tool_payload"))?;
        let bytes = serde_json::to_vec(&payload).map_err(|_| invalid("invalid_tool_payload"))?;
        if bytes.len() > 65536 {
            return Err(PortError::PayloadTooLarge);
        }
        let connector = MqttConnector::new(self.scope, Uuid::new_v4(), &self.config)?;
        let mut options = connector.options;
        options.set_client_id(format!("sq-call-{}", id.simple()));
        options.set_clean_session(true);
        let (client, mut events) = AsyncClient::new(options, 8);
        events.network_options.set_connection_timeout(5);
        let ack = tool.acknowledgement.as_ref();
        let mut dispatched = false;
        let work = async {
            loop {
                match events.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(a)))
                        if a.code == rumqttc::ConnectReturnCode::Success =>
                    {
                        if let Some(a) = ack {
                            client
                                .try_subscribe(&a.topic, QoS::AtLeastOnce)
                                .map_err(|_| PortError::Unavailable)?;
                        } else {
                            client
                                .try_publish(&tool.topic, QoS::AtLeastOnce, false, bytes.clone())
                                .map_err(|_| PortError::Unavailable)?;
                            dispatched = true;
                        }
                    }
                    Ok(Event::Incoming(Packet::SubAck(a))) => {
                        if a.return_codes
                            .iter()
                            .any(|c| matches!(c, rumqttc::SubscribeReasonCode::Failure))
                        {
                            return Err(PortError::PermissionDenied);
                        }
                        client
                            .try_publish(&tool.topic, QoS::AtLeastOnce, false, bytes.clone())
                            .map_err(|_| PortError::Unavailable)?;
                        dispatched = true;
                    }
                    Ok(Event::Incoming(Packet::PubAck(_))) => {
                        self.store
                            .device_sent(self.scope.workspace_id, self.owner)
                            .await?;
                        if ack.is_none() {
                            return Ok(json!({"status":"submitted","message_id":id}));
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) if dispatched => {
                        let Some(a) = ack else { continue };
                        if p.retain || p.topic != a.topic {
                            continue;
                        }
                        let Ok(value) = serde_json::from_slice::<Value>(&p.payload) else {
                            continue;
                        };
                        if value.pointer(&a.correlation_pointer) != Some(&json!(id.to_string())) {
                            continue;
                        }
                        // Confirmed observations, never desired state, feed the same state projector.
                        self.store
                            .device_reply(
                                ConnectionScope {
                                    workspace_id: self.scope.workspace_id,
                                    connection_id: self.owner,
                                },
                                p.topic.clone(),
                                value.clone(),
                            )
                            .await?;
                        let success = value.pointer(&a.result_pointer) == Some(&a.expected);
                        return Ok(
                            json!({"status":if success{"confirmed"}else{"device_rejected"},"message_id":id,"response":value}),
                        );
                    }
                    Ok(_) => {}
                    Err(_) => return Err(PortError::OutcomeUnknown),
                }
            }
        };
        let timeout = Duration::from_millis(ack.map_or(15000, |a| a.timeout_ms));
        let outcome = tokio::select! {biased;_=ctx.stopped()=>Err(PortError::OutcomeUnknown),r=tokio::time::timeout(timeout,work)=>r.unwrap_or(Err(PortError::OutcomeUnknown))};
        tracing::info!(event="mqtt.tool_completed",tool=%tool.id,invocation_id=%id,dispatched,confirmed=outcome.as_ref().is_ok_and(|v|v["status"]=="confirmed"));
        match outcome {
            Err(PortError::OutcomeUnknown) if dispatched => {
                Ok(json!({"status":"unconfirmed","message_id":id}))
            }
            Err(PortError::OutcomeUnknown) => Err(PortError::Unavailable),
            other => other,
        }
    }
}
