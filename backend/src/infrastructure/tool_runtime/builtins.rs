//! Built-in MCP queries and explicitly authorized actions.
use super::{ToolRuntime, schema::Schemas};
use crate::{
    application::{
        ports::{
            OperationContext, PortError, PortFuture, identity::AccessContext, tools::ToolExecutor,
        },
        services::invocation::{InvocationRequest, InvocationService},
    },
    domain::{
        builtin_tools::{BuiltinScope, BuiltinTool},
        connector::ConnectionScope,
        identity::{Action, Resource},
        tool::{ToolCall, ToolDefinition, ToolEffect, ToolResult},
    },
    infrastructure::pipeline::Pipeline,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

fn definition(tool: BuiltinTool) -> ToolDefinition {
    let (description, schema) = match tool {
        BuiltinTool::ConnectionsList => (
            "List connections this MCP service is allowed to view.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        BuiltinTool::ConnectionStatus => (
            "Read the current observed status of an allowed connection.",
            json!({"type":"object","properties":{"connection_id":{"type":"string","format":"uuid"}},"required":["connection_id"],"additionalProperties":false}),
        ),
        BuiltinTool::ConnectionStatistics => (
            "Read accepted receive and send counts for an allowed connection. A send is not proof of delivery or execution.",
            json!({"type":"object","properties":{"connection_id":{"type":"string","format":"uuid"}},"required":["connection_id"],"additionalProperties":false}),
        ),
        BuiltinTool::QqSendText => (
            "Send a text message to an explicitly authorized QQ group or direct conversation. Requires proactive sending permission.",
            json!({"type":"object","properties":{"connection_id":{"type":"string","format":"uuid"},"kind":{"enum":["group","direct"]},"conversation_id":{"type":"string","minLength":1,"maxLength":128},"content":{"type":"string","minLength":1,"maxLength":4000}},"required":["connection_id","kind","conversation_id","content"],"additionalProperties":false}),
        ),
        BuiltinTool::QqBotInfo => (
            "Read information about an explicitly allowed QQ bot connection.",
            json!({"type":"object","properties":{"connection_id":{"type":"string","format":"uuid"}},"required":["connection_id"],"additionalProperties":false}),
        ),
        BuiltinTool::RecentMessages => (
            "Read a bounded, incomplete cache from an explicitly allowed QQ conversation or connection stream. MQTT and webhook text contains JSON with event type, optional topic and payload.",
            json!({"type":"object","properties":{"connection_id":{"type":"string","format":"uuid"},"kind":{"enum":["group","direct","stream"]},"conversation_id":{"type":"string","minLength":1,"maxLength":128},"cursor":{"type":"string","maxLength":2048},"limit":{"type":"integer","minimum":1,"maximum":50}},"required":["connection_id","kind","conversation_id"],"additionalProperties":false}),
        ),
    };
    ToolDefinition {
        id: tool.name().into(),
        description: description.into(),
        input_schema: schema,
        output_schema: None,
        effect: if tool == BuiltinTool::QqSendText {
            ToolEffect::SideEffecting
        } else {
            ToolEffect::ReadOnly
        },
        documentation: None,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationArgs {
    connection_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    connection_id: Uuid,
    kind: String,
    conversation_id: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageArgs {
    connection_id: Uuid,
    kind: String,
    conversation_id: String,
    cursor: Option<String>,
    limit: Option<usize>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedCursor {
    profile: Uuid,
    revision: u64,
    cursor: String,
}
struct Executor {
    profile: Uuid,
    revision: u64,
    pipeline: Arc<Pipeline>,
    scope: BuiltinScope,
    tool: BuiltinTool,
}
impl ToolExecutor for Executor {
    fn discover(&self, _: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn invoke(&self, ctx: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult> {
        Box::pin(async move {
            let value = match self.tool {
                BuiltinTool::ConnectionsList => {
                    self.pipeline
                        .builtin_connections_locked(&self.scope)
                        .await?
                }
                BuiltinTool::ConnectionStatus | BuiltinTool::ConnectionStatistics => {
                    let args: ObservationArgs =
                        serde_json::from_value(call.arguments).map_err(|_| {
                            PortError::InvalidInput {
                                code: "invalid_arguments",
                            }
                        })?;
                    self.pipeline
                        .builtin_observation_locked(&self.scope, self.tool, args.connection_id)
                        .await?
                }
                BuiltinTool::QqSendText => {
                    use crate::application::ports::connection_actions::ConnectionActions;
                    use crate::domain::{
                        builtin_tools::ConversationGrant, connection_routing::RouteBinding,
                    };
                    use crate::infrastructure::connectors::{
                        message_config::ConnectionConfig,
                        qq::{QqBotSink, actions::QqActions},
                    };
                    let args: SendArgs = serde_json::from_value(call.arguments).map_err(|_| {
                        PortError::InvalidInput {
                            code: "invalid_arguments",
                        }
                    })?;
                    if !self
                        .scope
                        .permits_connection(BuiltinTool::QqSendText, args.connection_id)
                    {
                        return Err(PortError::PermissionDenied);
                    }
                    let connection = self
                        .pipeline
                        .connection_snapshot()?
                        .connections
                        .into_iter()
                        .find(|c| c.id == args.connection_id && c.enabled)
                        .ok_or(PortError::PermissionDenied)?;
                    let ConnectionConfig::QqBot(config) = &connection.config else {
                        return Err(PortError::PermissionDenied);
                    };
                    let grant = ConversationGrant {
                        connection_id: args.connection_id,
                        account: config.app_id.clone(),
                        kind: args.kind.clone(),
                        conversation_id: args.conversation_id.clone(),
                    };
                    if !config.allow_proactive || !self.scope.qq_send_conversations.contains(&grant)
                    {
                        return Err(PortError::PermissionDenied);
                    }
                    let scope = ConnectionScope {
                        workspace_id: ctx.scope.workspace_id,
                        connection_id: connection.send_id(),
                    };
                    let actions = QqActions {
                        sink: Arc::new(QqBotSink::new(scope, connection.send_id(), config)?),
                        destination_id: connection.send_id(),
                    };
                    let target = RouteBinding::new(
                        "qq",
                        json!({"app_id":config.app_id,"kind":args.kind,"id":args.conversation_id}),
                    );
                    let mut operation = ctx;
                    operation.scope = scope;
                    return actions
                        .executor(Some(target))?
                        .invoke(
                            operation,
                            ToolCall {
                                invocation_id: call.invocation_id,
                                tool_id: "qq.send_text".into(),
                                arguments: json!({"content":args.content}),
                            },
                        )
                        .await;
                }
                BuiltinTool::QqBotInfo => {
                    use crate::application::ports::connection_actions::ConnectionActions;
                    use crate::infrastructure::connectors::{
                        message_config::ConnectionConfig,
                        qq::{QqBotSink, actions::QqActions},
                    };
                    let args: ObservationArgs =
                        serde_json::from_value(call.arguments).map_err(|_| {
                            PortError::InvalidInput {
                                code: "invalid_arguments",
                            }
                        })?;
                    if !self
                        .scope
                        .permits_connection(BuiltinTool::QqBotInfo, args.connection_id)
                    {
                        return Err(PortError::PermissionDenied);
                    }
                    let connection = self
                        .pipeline
                        .connection_snapshot()?
                        .connections
                        .into_iter()
                        .find(|c| c.id == args.connection_id && c.enabled)
                        .ok_or(PortError::PermissionDenied)?;
                    let ConnectionConfig::QqBot(config) = &connection.config else {
                        return Err(PortError::PermissionDenied);
                    };
                    let scope = ConnectionScope {
                        workspace_id: ctx.scope.workspace_id,
                        connection_id: connection.send_id(),
                    };
                    let actions = QqActions {
                        sink: Arc::new(QqBotSink::new(scope, connection.send_id(), config)?),
                        destination_id: connection.send_id(),
                    };
                    let mut operation = ctx;
                    operation.scope = scope;
                    return actions
                        .executor(None)?
                        .invoke(
                            operation,
                            ToolCall {
                                invocation_id: call.invocation_id,
                                tool_id: "qq.bot_info".into(),
                                arguments: json!({}),
                            },
                        )
                        .await;
                }
                BuiltinTool::RecentMessages => {
                    let args: MessageArgs =
                        serde_json::from_value(call.arguments).map_err(|_| {
                            PortError::InvalidInput {
                                code: "invalid_arguments",
                            }
                        })?;
                    let mut matches = self.scope.conversations.iter().filter(|grant| {
                        grant.connection_id == args.connection_id
                            && grant.kind == args.kind
                            && grant.conversation_id == args.conversation_id
                    });
                    let grant = matches.next().cloned().ok_or(PortError::PermissionDenied)?;
                    if matches.next().is_some() {
                        return Err(PortError::PermissionDenied);
                    }
                    let inner =
                        args.cursor
                            .map(|value| {
                                let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| {
                                    PortError::InvalidInput {
                                        code: "invalid_recent_cursor",
                                    }
                                })?;
                                let wrapped: ScopedCursor = serde_json::from_slice(&bytes)
                                    .map_err(|_| PortError::InvalidInput {
                                        code: "invalid_recent_cursor",
                                    })?;
                                if wrapped.profile != self.profile
                                    || wrapped.revision != self.revision
                                {
                                    return Err(PortError::InvalidInput {
                                        code: "invalid_recent_cursor",
                                    });
                                }
                                Ok(wrapped.cursor)
                            })
                            .transpose()?;
                    let mut value = self
                        .pipeline
                        .builtin_recent_messages_locked(
                            &self.scope,
                            grant,
                            inner.as_deref(),
                            args.limit.unwrap_or(20),
                        )
                        .await?;
                    if let Some(cursor) = value["next_cursor"].as_str() {
                        let wrapped = ScopedCursor {
                            profile: self.profile,
                            revision: self.revision,
                            cursor: cursor.into(),
                        };
                        value["next_cursor"] = Value::String(URL_SAFE_NO_PAD.encode(
                            serde_json::to_vec(&wrapped).map_err(|_| PortError::Unavailable)?,
                        ));
                    }
                    value
                }
            };
            Ok(ToolResult {
                content: vec![],
                structured_content: Some(value),
                is_error: false,
            })
        })
    }
}
impl ToolRuntime {
    fn matching_pipeline(&self, pipeline: &Pipeline) -> bool {
        pipeline.source.workspace_id == self.config.workspace_id
            && self
                .config
                .database
                .canonicalize()
                .ok()
                .zip(pipeline.database_path().canonicalize().ok())
                .is_some_and(|(a, b)| a == b)
    }
    pub async fn list_builtins(
        &self,
        ctx: &AccessContext,
        profile: Uuid,
        pipeline: &Pipeline,
    ) -> Result<(u64, Vec<ToolDefinition>), PortError> {
        if !self.matching_pipeline(pipeline) {
            return Ok((0, vec![]));
        }
        let _publication = self.publication_gate.read().await;
        let server = self.endpoint(profile).await?;
        let mut output = Vec::new();
        for tool in BuiltinTool::ALL {
            if !server.builtin_scope.tools.contains(&tool) {
                continue;
            }
            let resource = Resource::McpBuiltin {
                workspace_id: self.config.workspace_id,
                profile_id: profile,
                tool,
            };
            match self
                .governance
                .authorize(
                    ctx,
                    if tool == BuiltinTool::QqSendText {
                        Action::InvokeMcpBuiltin
                    } else {
                        Action::ReadMcpBuiltin
                    },
                    &resource,
                )
                .await
            {
                Ok(()) => output.push(definition(tool)),
                Err(PortError::PermissionDenied) => (),
                Err(e) => return Err(e),
            }
        }
        Ok((server.revision, output))
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn call_builtin(
        &self,
        ctx: &AccessContext,
        profile: Uuid,
        pipeline: Arc<Pipeline>,
        name: &str,
        revision: u64,
        invocation_id: Uuid,
        arguments: Value,
    ) -> Result<crate::application::services::tools::ToolInvocationReport, PortError> {
        if !self.matching_pipeline(&pipeline) {
            return Err(PortError::Unavailable);
        }
        let tool = BuiltinTool::ALL
            .into_iter()
            .find(|tool| tool.name() == name)
            .ok_or(PortError::PermissionDenied)?;
        let _publication = self.publication_gate.read().await;
        let _connections = pipeline.connection_io.read().await;
        let server = self.endpoint(profile).await?;
        if server.revision != revision {
            return Err(PortError::Conflict);
        }
        let def = definition(tool);
        static SCHEMAS: OnceLock<Arc<Schemas>> = OnceLock::new();
        let validator = if let Some(schemas) = SCHEMAS.get() {
            schemas.clone()
        } else {
            let schemas = Arc::new(Schemas::compile(
                BuiltinTool::ALL
                    .into_iter()
                    .map(|tool| definition(tool).input_schema),
            )?);
            let _ = SCHEMAS.set(schemas.clone());
            SCHEMAS.get().cloned().unwrap_or(schemas)
        };
        let executor = Arc::new(Executor {
            pipeline: pipeline.clone(),
            profile,
            revision,
            scope: server.builtin_scope,
            tool,
        });
        let service = InvocationService {
            validator,
            governance: self.governance.clone(),
            ledger: Some(self.store.clone()),
        };
        let request = InvocationRequest {
            scope: ConnectionScope {
                workspace_id: self.config.workspace_id,
                connection_id: profile,
            },
            definition: def,
            executor,
            invocation_id,
            intent: json!({"profile":profile,"revision":revision,"tool":name,"arguments":arguments}),
            arguments,
        };
        if tool == BuiltinTool::QqSendText {
            service
                .invoke_builtin_action(ctx, request, profile, tool)
                .await
        } else {
            service
                .invoke_ephemeral_builtin(ctx, request, profile, tool)
                .await
        }
    }
}
