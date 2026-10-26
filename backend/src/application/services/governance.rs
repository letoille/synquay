use crate::{
    application::ports::{
        PortError,
        audit::AuditLog,
        identity::{AccessContext, PolicyEngine},
    },
    domain::{
        audit::{AuditPhase, AuditRecord},
        identity::{Action, Resource},
    },
};
use std::{sync::Arc, time::SystemTime};
use uuid::Uuid;

pub struct Governance {
    policy: Arc<dyn PolicyEngine>,
    audit: Arc<dyn AuditLog>,
}
impl Governance {
    pub fn new(policy: Arc<dyn PolicyEngine>, audit: Arc<dyn AuditLog>) -> Self {
        Self { policy, audit }
    }

    pub async fn authorize(
        &self,
        ctx: &AccessContext,
        action: Action,
        resource: &Resource,
    ) -> Result<(), PortError> {
        ctx.check()?;
        if resource.workspace_id() != ctx.identity.workspace_id {
            return Err(PortError::ScopeMismatch);
        }
        let result = self
            .policy
            .authorize(ctx.identity.clone(), action, resource.clone())
            .await;
        if matches!(result, Err(PortError::PermissionDenied)) {
            // Never turn a denial into execution, even if the denial audit store fails.
            if self
                .record(
                    ctx,
                    ctx.operation.request_id,
                    action,
                    resource,
                    AuditPhase::Denied,
                )
                .await
                .is_err()
            {
                tracing::error!(event = "audit.denial_failed", request_id = %ctx.operation.request_id, "Denied action audit could not be persisted");
            }
        }
        result
    }

    pub async fn record(
        &self,
        ctx: &AccessContext,
        operation_id: Uuid,
        action: Action,
        resource: &Resource,
        phase: AuditPhase,
    ) -> Result<(), PortError> {
        self.audit
            .append(AuditRecord {
                record_id: Uuid::new_v4(),
                operation_id,
                request_id: ctx.operation.request_id,
                workspace_id: ctx.identity.workspace_id,
                principal_id: ctx.identity.principal_id,
                action,
                resource: resource.clone(),
                phase,
                occurred_at: SystemTime::now(),
            })
            .await
    }
}
