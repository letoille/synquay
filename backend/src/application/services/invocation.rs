//! Shared execution boundary for MCP publication and trusted command routes.
use super::{governance::Governance, tools::ToolInvocationReport};
use crate::{
    application::ports::{
        PortError,
        identity::AccessContext,
        invocations::{Admission, InvocationKey, InvocationLedger, SavedOutcome},
        profiles::SchemaValidator,
        tools::ToolExecutor,
    },
    domain::{
        audit::{AuditPhase, AuditRecord},
        connector::ConnectionScope,
        identity::{Action, Resource},
        tool::{ToolCall, ToolDefinition},
    },
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub struct InvocationRequest {
    pub scope: ConnectionScope,
    pub definition: ToolDefinition,
    pub executor: Arc<dyn ToolExecutor>,
    pub invocation_id: Uuid,
    pub arguments: Value,
    /// Trusted publication/route identity; part of the durable deduplication fingerprint.
    pub intent: Value,
}
pub struct InvocationService {
    pub validator: Arc<dyn SchemaValidator>,
    pub governance: Arc<Governance>,
    pub ledger: Option<Arc<dyn InvocationLedger>>,
}
impl InvocationService {
    /// Read sensitive, volatile data without placing the result in the durable ledger.
    /// Intended only for trusted, explicitly selected built-in read-only operations.
    pub async fn invoke_ephemeral_read(
        &self,
        ctx: &AccessContext,
        request: InvocationRequest,
    ) -> Result<ToolInvocationReport, PortError> {
        let resource = Resource::Tool {
            scope: request.scope,
            tool_id: request.definition.id.clone(),
        };
        self.invoke_ephemeral(ctx, request, Action::InvokeTool, resource)
            .await
    }

    /// Built-in queries use a distinct policy resource and never persist results.
    pub async fn invoke_ephemeral_builtin(
        &self,
        ctx: &AccessContext,
        request: InvocationRequest,
        profile_id: Uuid,
        tool: crate::domain::builtin_tools::BuiltinTool,
    ) -> Result<ToolInvocationReport, PortError> {
        if request.definition.id != tool.name() {
            return Err(PortError::PermissionDenied);
        }
        let resource = Resource::McpBuiltin {
            workspace_id: request.scope.workspace_id,
            profile_id,
            tool,
        };
        self.invoke_ephemeral(ctx, request, Action::ReadMcpBuiltin, resource)
            .await
    }

    async fn invoke_ephemeral(
        &self,
        ctx: &AccessContext,
        request: InvocationRequest,
        action: Action,
        resource: Resource,
    ) -> Result<ToolInvocationReport, PortError> {
        use crate::domain::tool::ToolEffect;
        if request.definition.effect != ToolEffect::ReadOnly {
            return Err(PortError::PermissionDenied);
        }
        self.governance.authorize(ctx, action, &resource).await?;
        self.validator
            .validate(&request.definition.input_schema, &request.arguments)
            .await?;
        self.governance
            .record(
                ctx,
                request.invocation_id,
                action,
                &resource,
                AuditPhase::Started,
            )
            .await?;
        // Permissions can change while validation and audit storage are pending.
        self.governance.authorize(ctx, action, &resource).await?;
        ctx.check()?;
        let mut operation = ctx.operation.clone();
        operation.scope = request.scope;
        let mut outcome = request
            .executor
            .invoke(
                operation,
                ToolCall {
                    invocation_id: request.invocation_id,
                    tool_id: request.definition.id,
                    arguments: request.arguments,
                },
            )
            .await;
        if let (Ok(result), Some(schema)) = (&outcome, &request.definition.output_schema)
            && match &result.structured_content {
                Some(value) => self.validator.validate(schema, value).await.is_err(),
                None => true,
            }
        {
            outcome = Err(PortError::Failed {
                code: "invalid_tool_output",
            });
        }
        // Never return a result after membership or access has been revoked.
        self.governance.authorize(ctx, action, &resource).await?;
        let phase = if outcome.as_ref().is_ok_and(|result| !result.is_error) {
            AuditPhase::Succeeded
        } else {
            AuditPhase::Failed
        };
        // Fail closed: sensitive output is not released if its audit cannot be saved.
        self.governance
            .record(ctx, request.invocation_id, action, &resource, phase)
            .await?;
        Ok(ToolInvocationReport {
            outcome,
            audit_recorded: true,
        })
    }

    pub async fn invoke(
        &self,
        ctx: &AccessContext,
        request: InvocationRequest,
    ) -> Result<ToolInvocationReport, PortError> {
        let resource = Resource::Tool {
            scope: request.scope,
            tool_id: request.definition.id.clone(),
        };
        self.invoke_with_resource(ctx, request, Action::InvokeTool, resource)
            .await
    }
    pub async fn invoke_builtin_action(
        &self,
        ctx: &AccessContext,
        request: InvocationRequest,
        profile_id: Uuid,
        tool: crate::domain::builtin_tools::BuiltinTool,
    ) -> Result<ToolInvocationReport, PortError> {
        if request.definition.id != tool.name()
            || tool != crate::domain::builtin_tools::BuiltinTool::QqSendText
        {
            return Err(PortError::PermissionDenied);
        }
        let resource = Resource::McpBuiltin {
            workspace_id: request.scope.workspace_id,
            profile_id,
            tool,
        };
        self.invoke_with_resource(ctx, request, Action::InvokeMcpBuiltin, resource)
            .await
    }
    async fn invoke_with_resource(
        &self,
        ctx: &AccessContext,
        request: InvocationRequest,
        action: Action,
        resource: Resource,
    ) -> Result<ToolInvocationReport, PortError> {
        self.governance.authorize(ctx, action, &resource).await?;
        self.validator
            .validate(&request.definition.input_schema, &request.arguments)
            .await?;
        let key = InvocationKey {
            workspace_id: ctx.identity.workspace_id,
            principal_id: ctx.identity.principal_id,
            invocation_id: request.invocation_id,
        };
        if let Some(ledger) = &self.ledger {
            let intent =
                serde_json::to_vec(&request.intent).map_err(|_| PortError::InvalidInput {
                    code: "invalid_arguments",
                })?;
            let hash = ring::digest::digest(&ring::digest::SHA256, &intent);
            let fingerprint = hash.as_ref().iter().map(|b| format!("{b:02x}")).collect();
            if let Admission::Cached(outcome) = ledger.begin(key.clone(), fingerprint).await? {
                let audit_recorded = !matches!(outcome, SavedOutcome::Unknown);
                return Ok(ToolInvocationReport {
                    outcome: outcome.replay(),
                    audit_recorded,
                });
            }
        }
        self.governance
            .record(
                ctx,
                request.invocation_id,
                action,
                &resource,
                AuditPhase::Started,
            )
            .await?;
        ctx.check()?;
        let mut operation = ctx.operation.clone();
        operation.scope = request.scope;
        // Recheck membership and current grants after admission/audit awaits, before dispatch.
        let mut outcome = match self.governance.authorize(ctx, action, &resource).await {
            Ok(()) => {
                request
                    .executor
                    .invoke(
                        operation,
                        ToolCall {
                            invocation_id: request.invocation_id,
                            tool_id: request.definition.id,
                            arguments: request.arguments,
                        },
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        if let (Ok(result), Some(schema)) = (&outcome, &request.definition.output_schema) {
            let valid = match &result.structured_content {
                Some(value) => self.validator.validate(schema, value).await.is_ok(),
                None => false,
            };
            if !valid {
                outcome = Err(PortError::Failed {
                    code: "invalid_tool_output",
                });
            }
        }
        let succeeded = outcome.as_ref().is_ok_and(|result| !result.is_error);
        let phase = if succeeded {
            AuditPhase::Succeeded
        } else {
            AuditPhase::Failed
        };
        let audit_recorded = if let Some(ledger) = &self.ledger {
            ledger
                .finish(
                    key,
                    SavedOutcome::capture(&outcome),
                    AuditRecord {
                        record_id: Uuid::new_v4(),
                        operation_id: request.invocation_id,
                        request_id: ctx.operation.request_id,
                        workspace_id: ctx.identity.workspace_id,
                        principal_id: ctx.identity.principal_id,
                        action: action,
                        resource,
                        phase,
                        occurred_at: std::time::SystemTime::now(),
                    },
                )
                .await
                .is_ok()
        } else {
            self.governance
                .record(ctx, request.invocation_id, action, &resource, phase)
                .await
                .is_ok()
        };
        Ok(ToolInvocationReport {
            outcome,
            audit_recorded,
        })
    }
}
