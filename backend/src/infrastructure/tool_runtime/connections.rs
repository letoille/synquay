use super::{
    ToolRuntime,
    config::{HttpToolConfig, ToolsConfig},
    control::adopt_snapshot,
    http::HttpExecutor,
    schema::Schemas,
};
use crate::{
    application::{
        connectors::ConnectorRegistration,
        ports::{PortError, profiles::SchemaValidator, publications::PublicationReceipt},
    },
    domain::{
        connector::{ConnectionScope, ConnectorDescriptor, ConnectorKind},
        tool::ToolEffect,
    },
    infrastructure::{capabilities::StaticCapabilityCatalog, storage::connections::ConnectionPlan},
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddConnectionInput {
    pub operation_id: Uuid,
    pub expected_revision: u64,
    pub tool: HttpToolConfig,
}
impl ToolRuntime {
    pub(super) fn inventory_snapshot(&self) -> Result<ToolsConfig, PortError> {
        self.inventory
            .read()
            .map(|v| v.clone())
            .map_err(|_| PortError::Unavailable)
    }
    /// Read-only recovery; returns only a receipt, never credentials or full draft input.
    pub async fn added_connection(
        &self,
        id: Uuid,
    ) -> Result<Option<PublicationReceipt>, PortError> {
        let scope = self.publication_scope().await?;
        self.store
            .connection_receipt(scope, id)
            .await
            .map(|v| v.map(|(_, r)| r))
    }
    /// Additive online configuration. Does not call the upstream or grant Agent access.
    pub async fn add_connection(
        self: &Arc<Self>,
        input: AddConnectionInput,
    ) -> Result<PublicationReceipt, PortError> {
        self.workspace_admin(true).await?;
        if input.operation_id.is_nil()
            || input.tool.connection_id != input.operation_id
            || input.expected_revision == 0
            || input.expected_revision >= i64::MAX as u64 - 1
            || input.tool.exposed
            || input.tool.bearer_env.is_some()
        {
            return Err(PortError::InvalidInput {
                code: "invalid_connection_input",
            });
        }
        let permit = self
            .publish_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| PortError::RateLimited {
                retry_after: Duration::from_secs(1),
            })?;
        let runtime = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _guard = runtime.publication_gate.write().await;
            runtime.workspace_admin(true).await?;
            let scope = runtime.publication_scope().await?;
            let request = serde_json::to_value(&input).map_err(|_| PortError::Unavailable)?;
            if let Some((old, receipt)) = runtime
                .store
                .connection_receipt(scope.clone(), input.operation_id)
                .await?
            {
                if old != request {
                    return Err(PortError::Conflict);
                }
                return Ok(receipt);
            }
            let mut config = runtime.inventory_snapshot()?;
            let record = runtime
                .store
                .tools_snapshot(config.workspace_id, config.profile_id)
                .await?
                .ok_or(PortError::NotFound)?;
            adopt_snapshot(&mut config, &record)?;
            if config.revision != input.expected_revision {
                return Err(PortError::Conflict);
            }
            config.revision += 1;
            config.tools.push(input.tool.clone());
            config.validate()?;
            let schemas = Schemas::compile(
                std::iter::once(input.tool.definition.input_schema.clone())
                    .chain(input.tool.definition.output_schema.clone()),
            )?;
            if let Some(doc) = &input.tool.definition.documentation {
                for example in &doc.examples {
                    schemas
                        .validate(&input.tool.definition.input_schema, &example.arguments)
                        .await?;
                }
            }
            let capability = input.tool.capability(config.workspace_id, config.revision);
            let reference = capability.descriptor.reference.clone();
            let catalog = Arc::new(StaticCapabilityCatalog::build(vec![capability], vec![])?);
            let connection_scope = ConnectionScope {
                workspace_id: config.workspace_id,
                connection_id: input.tool.connection_id,
            };
            let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
                scope: connection_scope,
                kind: ConnectorKind::Http,
                display_name: input.tool.public_name.clone(),
            });
            registration.tools = Some(Arc::new(HttpExecutor {
                catalog,
                reference,
                scope: connection_scope,
                credential: None,
                client: runtime.http_client.clone(),
                slots: runtime.execution_slots.clone(),
            }));
            let receipt = runtime
                .store
                .create_connection(ConnectionPlan {
                    scope,
                    operation_id: input.operation_id,
                    expected_revision: input.expected_revision,
                    request,
                    tool: serde_json::to_value(&input.tool).map_err(|_| PortError::Unavailable)?,
                    profile: config.profile(),
                    snapshot: config.snapshot(),
                })
                .await?;
            // Prevalidated additive activation while readers are excluded by the publication gate.
            runtime
                .schemas
                .extend(schemas)
                .map_err(|_| PortError::OutcomeUnknown)?;
            runtime
                .registry
                .register_shared(registration)
                .map_err(|_| PortError::OutcomeUnknown)?;
            if matches!(input.tool.definition.effect, ToolEffect::ReadOnly) {
                runtime
                    .read_only_tools
                    .write()
                    .map_err(|_| PortError::OutcomeUnknown)?
                    .insert((connection_scope, input.tool.definition.id.clone()));
            }
            *runtime
                .inventory
                .write()
                .map_err(|_| PortError::OutcomeUnknown)? = config;
            Ok(receipt)
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)?
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeConnectionInput {
    pub operation_id: Uuid,
    pub expected_revision: u64,
    pub change: ConnectionChange,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionChange {
    Update {
        public_name: String,
        description: String,
        url: String,
        effect: ToolEffect,
        input_schema: serde_json::Value,
        allow_loopback_http: bool,
    },
    Delete,
}
impl ToolRuntime {
    pub async fn change_connection(
        self: &Arc<Self>,
        id: Uuid,
        input: ChangeConnectionInput,
    ) -> Result<PublicationReceipt, PortError> {
        self.workspace_admin(true).await?;
        if id.is_nil()
            || input.operation_id.is_nil()
            || input.expected_revision == 0
            || input.expected_revision >= i64::MAX as u64 - 1
        {
            return Err(PortError::InvalidInput {
                code: "invalid_connection_input",
            });
        }
        let permit = self
            .publish_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| PortError::RateLimited {
                retry_after: Duration::from_secs(1),
            })?;
        let runtime = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _guard = runtime.publication_gate.write().await;
            runtime.workspace_admin(true).await?;
            let scope = runtime.publication_scope().await?;
            let request = serde_json::json!({"connection_id":id,"input":input});
            if let Some((old, receipt)) = runtime
                .store
                .connection_receipt(scope.clone(), input.operation_id)
                .await?
            {
                if old != request {
                    return Err(PortError::Conflict);
                }
                return Ok(receipt);
            }
            let mut config = runtime.inventory_snapshot()?;
            let record = runtime
                .store
                .tools_snapshot(config.workspace_id, config.profile_id)
                .await?
                .ok_or(PortError::NotFound)?;
            adopt_snapshot(&mut config, &record)?;
            if config.revision != input.expected_revision {
                return Err(PortError::Conflict);
            }
            let index = config
                .tools
                .iter()
                .position(|t| t.connection_id == id)
                .ok_or(PortError::NotFound)?;
            let previous = config.tools[index].clone();
            let replacement = match input.change {
                ConnectionChange::Delete => {
                    config.tools.remove(index);
                    None
                }
                ConnectionChange::Update {
                    public_name,
                    description,
                    url,
                    effect,
                    input_schema,
                    allow_loopback_http,
                } => {
                    // OAuth grants are names: do not silently transfer a named grant on rename.
                    if public_name != previous.public_name
                        && runtime.security.oauth.as_ref().is_some_and(|o| {
                            o.grants
                                .iter()
                                .any(|g| g.tools.contains(&previous.public_name))
                        })
                    {
                        return Err(PortError::InvalidInput {
                            code: "connection_grant_in_use",
                        });
                    }
                    let tool = &mut config.tools[index];
                    tool.public_name = public_name;
                    tool.definition.description = description;
                    tool.url = url;
                    tool.definition.effect = effect;
                    tool.definition.input_schema = input_schema;
                    tool.allow_loopback_http = allow_loopback_http;
                    Some(tool.clone())
                }
            };
            config.revision += 1;
            config.validate()?;
            let schemas = Schemas::compile(config.tools.iter().flat_map(|t| {
                std::iter::once(t.definition.input_schema.clone())
                    .chain(t.definition.output_schema.clone())
            }))?;
            for tool in &config.tools {
                if let Some(doc) = &tool.definition.documentation {
                    for example in &doc.examples {
                        schemas
                            .validate(&tool.definition.input_schema, &example.arguments)
                            .await?;
                    }
                }
            }
            let connection_scope = ConnectionScope {
                workspace_id: config.workspace_id,
                connection_id: id,
            };
            let registration = if let Some(tool) = &replacement {
                let capability = tool.capability(config.workspace_id, config.revision);
                let reference = capability.descriptor.reference.clone();
                let catalog = Arc::new(StaticCapabilityCatalog::build(vec![capability], vec![])?);
                let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
                    scope: connection_scope,
                    kind: ConnectorKind::Http,
                    display_name: tool.public_name.clone(),
                });
                registration.tools = Some(Arc::new(HttpExecutor {
                    catalog,
                    reference,
                    scope: connection_scope,
                    credential: tool
                        .bearer_env
                        .as_deref()
                        .map(super::config::secret)
                        .transpose()?,
                    client: runtime.http_client.clone(),
                    slots: runtime.execution_slots.clone(),
                }));
                Some(registration)
            } else {
                None
            };
            let binding = replacement
                .as_ref()
                .map(|t| crate::domain::profile::PublishedTool {
                    public_name: t.public_name.clone(),
                    scope: connection_scope,
                    definition: t.definition.clone(),
                });
            let receipt = runtime
                .store
                .change_connection(
                    ConnectionPlan {
                        scope,
                        operation_id: input.operation_id,
                        expected_revision: input.expected_revision,
                        request,
                        tool: serde_json::to_value(&replacement)
                            .map_err(|_| PortError::Unavailable)?,
                        profile: config.profile(),
                        snapshot: config.snapshot(),
                    },
                    id,
                    binding,
                )
                .await?;
            runtime
                .schemas
                .replace(schemas)
                .map_err(|_| PortError::OutcomeUnknown)?;
            runtime
                .registry
                .replace_shared(connection_scope, registration)
                .map_err(|_| PortError::OutcomeUnknown)?;
            let mut read_only = runtime
                .read_only_tools
                .write()
                .map_err(|_| PortError::OutcomeUnknown)?;
            read_only.retain(|(scope, _)| *scope != connection_scope);
            if let Some(tool) = replacement
                && matches!(tool.definition.effect, ToolEffect::ReadOnly)
            {
                read_only.insert((connection_scope, tool.definition.id));
            }
            *runtime
                .inventory
                .write()
                .map_err(|_| PortError::OutcomeUnknown)? = config;
            Ok(receipt)
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)?
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyInput {
    pub mcp_token: String,
    pub invocation_id: Uuid,
    pub action: VerifyAction,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VerifyAction {
    Inspect,
    Execute {
        revision: u64,
        public_name: String,
        arguments: serde_json::Value,
        confirmed: bool,
    },
}
impl ToolRuntime {
    pub async fn verify_connection(
        &self,
        input: VerifyInput,
    ) -> Result<serde_json::Value, PortError> {
        self.workspace_admin(true).await?;
        if input.invocation_id.is_nil() || input.mcp_token.len() > 4096 {
            return Err(PortError::InvalidInput {
                code: "invalid_verification",
            });
        }
        let endpoint = self.endpoint(self.config.profile_id).await?;
        let identity = self
            .authenticate_endpoint(&endpoint, Some(&format!("Bearer {}", input.mcp_token)))
            .await?;
        let mut ctx = self.context(tokio_util::sync::CancellationToken::new());
        ctx.operation.principal_id = identity.principal_id;
        ctx.identity = identity;
        // Reuse the normal Profile, membership and tool authorization path.
        self.service.list(&ctx, self.config.profile_id).await?;
        match input.action {
            VerifyAction::Inspect => Ok(
                serde_json::json!({"invocation_id":input.invocation_id,"status":self.store.invocation_status(self.config.workspace_id,ctx.identity.principal_id,input.invocation_id).await?}),
            ),
            VerifyAction::Execute {
                revision,
                public_name,
                arguments,
                confirmed,
            } => {
                if !confirmed {
                    return Err(PortError::InvalidInput {
                        code: "confirmation_required",
                    });
                }
                let report = self
                    .service
                    .invoke(
                        &ctx,
                        crate::application::services::tools::PublishedToolCall {
                            profile_id: self.config.profile_id,
                            revision,
                            public_name,
                            invocation_id: input.invocation_id,
                            arguments,
                        },
                    )
                    .await?;
                let status = if !report.audit_recorded {
                    "unknown"
                } else {
                    match report.outcome {
                        Ok(result) if !result.is_error => "succeeded",
                        Ok(_) => "failed",
                        Err(
                            PortError::OutcomeUnknown
                            | PortError::DeadlineExceeded
                            | PortError::Cancelled,
                        ) => "unknown",
                        Err(_) => "failed",
                    }
                };
                Ok(serde_json::json!({"invocation_id":input.invocation_id,"status":status}))
            }
        }
    }
}
