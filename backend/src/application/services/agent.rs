use super::governance::Governance;
use crate::{
    application::ports::{PortError, agent::AgentJobStore, identity::AccessContext},
    domain::{
        agent::*,
        identity::{Action, Resource},
    },
};
use std::sync::Arc;
use uuid::Uuid;

pub struct AgentJobService {
    pub binding: AgentBinding,
    store: Arc<dyn AgentJobStore>,
    governance: Arc<Governance>,
}
impl AgentJobService {
    pub fn new(
        binding: AgentBinding,
        store: Arc<dyn AgentJobStore>,
        governance: Arc<Governance>,
    ) -> Self {
        Self {
            binding,
            store,
            governance,
        }
    }
    pub async fn authorize(&self, ctx: &AccessContext, write: bool) -> Result<(), PortError> {
        self.governance
            .authorize(
                ctx,
                if write {
                    Action::ManageAgentJobs
                } else {
                    Action::ReadAgentJobs
                },
                &Resource::AgentQueue {
                    workspace_id: self.binding.source.workspace_id,
                    profile_id: self.binding.config.profile_id,
                },
            )
            .await
    }
    pub async fn list(
        &self,
        ctx: &AccessContext,
        after: Option<Uuid>,
    ) -> Result<Vec<JobSummary>, PortError> {
        self.authorize(ctx, false).await?;
        self.store.list(ctx.clone(), after).await
    }
    pub async fn read(&self, ctx: &AccessContext, id: Uuid) -> Result<JobInput, PortError> {
        self.authorize(ctx, false).await?;
        self.store.read(ctx.clone(), id).await
    }
    pub async fn claim(
        &self,
        ctx: &AccessContext,
        id: Uuid,
    ) -> Result<Option<ClaimedJob>, PortError> {
        self.authorize(ctx, true).await?;
        if id.is_nil() {
            return Err(PortError::InvalidInput { code: "claim_uuid" });
        }
        self.store.claim(ctx.clone(), id).await
    }
    pub async fn renew(
        &self,
        ctx: &AccessContext,
        token: ClaimToken,
    ) -> Result<ClaimedJob, PortError> {
        self.authorize(ctx, true).await?;
        self.store.renew(ctx.clone(), token).await
    }
    pub async fn complete(
        &self,
        ctx: &AccessContext,
        token: ClaimToken,
        text: String,
    ) -> Result<CompletionReceipt, PortError> {
        self.authorize(ctx, true).await?;
        if text.trim().is_empty() || text.len() > 8192 || text.contains('\0') {
            return Err(PortError::InvalidInput {
                code: "reply_text_1_to_8192_bytes",
            });
        }
        self.store.complete(ctx.clone(), token, text).await
    }
    pub async fn fail(
        &self,
        ctx: &AccessContext,
        token: ClaimToken,
        retry: bool,
    ) -> Result<JobSummary, PortError> {
        self.authorize(ctx, true).await?;
        self.store.fail(ctx.clone(), token, retry).await
    }
}
