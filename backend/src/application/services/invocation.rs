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
    pub async fn invoke(
        &self,
        ctx: &AccessContext,
        request: InvocationRequest,
    ) -> Result<ToolInvocationReport, PortError> {
        let resource = Resource::Tool {
            scope: request.scope,
            tool_id: request.definition.id.clone(),
        };
        self.governance
            .authorize(ctx, Action::InvokeTool, &resource)
            .await?;
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
                Action::InvokeTool,
                &resource,
                AuditPhase::Started,
            )
            .await?;
        ctx.check()?;
        let mut operation = ctx.operation.clone();
        operation.scope = request.scope;
        // Recheck membership and current grants after admission/audit awaits, before dispatch.
        let mut outcome = match self
            .governance
            .authorize(ctx, Action::InvokeTool, &resource)
            .await
        {
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
                        action: Action::InvokeTool,
                        resource,
                        phase,
                        occurred_at: std::time::SystemTime::now(),
                    },
                )
                .await
                .is_ok()
        } else {
            self.governance
                .record(
                    ctx,
                    request.invocation_id,
                    Action::InvokeTool,
                    &resource,
                    phase,
                )
                .await
                .is_ok()
        };
        Ok(ToolInvocationReport {
            outcome,
            audit_recorded,
        })
    }
}
