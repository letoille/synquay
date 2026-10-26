//! Reviewed content exposure and per-principal authorization, independent of MCP wire types.
use super::governance::Governance;
use crate::{
    application::ports::{PortError, identity::AccessContext, mcp_content::McpContentProvider},
    domain::{
        audit::AuditPhase,
        identity::{Action, Resource},
        mcp_content::*,
    },
};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

#[derive(Clone)]
pub struct McpContentService {
    exposure: Option<Arc<crate::domain::mcp_server::ContentScope>>,
    workspace: Uuid,
    profile: Uuid,
    provider: Arc<dyn McpContentProvider>,
    governance: Arc<Governance>,
}
pub struct ResourceRead {
    pub uri: String,
    pub mime_type: String,
    pub text: Arc<str>,
}
impl McpContentService {
    pub fn new(
        workspace: Uuid,
        profile: Uuid,
        provider: Arc<dyn McpContentProvider>,
        governance: Arc<Governance>,
    ) -> Self {
        Self {
            exposure: None,
            workspace,
            profile,
            provider,
            governance,
        }
    }
    pub fn scoped(&self, profile: Uuid, scope: crate::domain::mcp_server::ContentScope) -> Self {
        Self {
            profile,
            exposure: Some(Arc::new(scope)),
            ..self.clone()
        }
    }
    fn resource_exposed(&self, id: &str, legacy: bool) -> bool {
        self.exposure
            .as_ref()
            .map_or(legacy, |s| s.resources.iter().any(|r| r == id))
    }
    fn prompt_exposed(&self, id: &str, legacy: bool) -> bool {
        self.exposure
            .as_ref()
            .map_or(legacy, |s| s.prompts.iter().any(|p| p == id))
    }
    pub fn capabilities(&self) -> (bool, bool) {
        let c = self.provider.catalog();
        (
            c.resources
                .iter()
                .any(|r| self.resource_exposed(&r.id, r.exposed))
                || c.templates
                    .iter()
                    .any(|r| self.resource_exposed(&r.id, r.exposed)),
            c.prompts
                .iter()
                .any(|p| self.prompt_exposed(&p.id, p.exposed)),
        )
    }
    fn base(&self) -> String {
        format!(
            "synquay://{}/{}/c{}",
            self.workspace,
            self.profile,
            self.provider.catalog().revision
        )
    }
    pub fn resource_uri(&self, id: &str) -> String {
        format!("{}/resources/{id}", self.base())
    }
    pub fn template_uri(&self, r: &ResourceTemplateDefinition) -> String {
        format!("{}/templates/{}/{{{}}}", self.base(), r.id, r.parameter)
    }
    pub fn prompt_name(&self, id: &str) -> String {
        format!("{id}__c{}", self.provider.catalog().revision)
    }
    async fn profile_access(&self, ctx: &AccessContext) -> Result<(), PortError> {
        self.governance
            .authorize(
                ctx,
                Action::UseProfile,
                &Resource::Profile {
                    workspace_id: self.workspace,
                    profile_id: self.profile,
                },
            )
            .await
    }
    fn resource(&self, id: &str, prompt: bool) -> (Action, Resource) {
        if prompt {
            (
                Action::GetMcpPrompt,
                Resource::McpPrompt {
                    workspace_id: self.workspace,
                    profile_id: self.profile,
                    id: id.into(),
                },
            )
        } else {
            (
                Action::ReadMcpResource,
                Resource::McpResource {
                    workspace_id: self.workspace,
                    profile_id: self.profile,
                    id: id.into(),
                },
            )
        }
    }
    async fn allowed(
        &self,
        ctx: &AccessContext,
        id: &str,
        prompt: bool,
    ) -> Result<bool, PortError> {
        let (action, r) = self.resource(id, prompt);
        match self.governance.authorize(ctx, action, &r).await {
            Ok(()) => Ok(true),
            Err(PortError::PermissionDenied) => Ok(false),
            Err(e) => Err(e),
        }
    }
    async fn require(
        &self,
        ctx: &AccessContext,
        id: &str,
        prompt: bool,
    ) -> Result<(Action, Resource), PortError> {
        self.profile_access(ctx).await?;
        let (a, r) = self.resource(id, prompt);
        self.governance.authorize(ctx, a, &r).await?;
        Ok((a, r))
    }
    pub async fn resources(
        &self,
        ctx: &AccessContext,
    ) -> Result<Vec<ResourceDefinition>, PortError> {
        self.profile_access(ctx).await?;
        let mut result = vec![];
        for r in &self.provider.catalog().resources {
            if self.resource_exposed(&r.id, r.exposed) && self.allowed(ctx, &r.id, false).await? {
                result.push(r.clone())
            }
        }
        Ok(result)
    }
    pub async fn templates(
        &self,
        ctx: &AccessContext,
    ) -> Result<Vec<ResourceTemplateDefinition>, PortError> {
        self.profile_access(ctx).await?;
        let mut result = vec![];
        for r in &self.provider.catalog().templates {
            if self.resource_exposed(&r.id, r.exposed) && self.allowed(ctx, &r.id, false).await? {
                result.push(r.clone())
            }
        }
        Ok(result)
    }
    pub async fn prompts(&self, ctx: &AccessContext) -> Result<Vec<PromptDefinition>, PortError> {
        self.profile_access(ctx).await?;
        let mut result = vec![];
        for p in &self.provider.catalog().prompts {
            if self.prompt_exposed(&p.id, p.exposed) && self.allowed(ctx, &p.id, true).await? {
                result.push(p.clone())
            }
        }
        Ok(result)
    }
    #[tracing::instrument(skip_all,fields(request_id=%ctx.operation.request_id))]
    pub async fn read(&self, ctx: &AccessContext, uri: &str) -> Result<ResourceRead, PortError> {
        self.profile_access(ctx).await?;
        let config = self.provider.catalog();
        let matched = config
            .resources
            .iter()
            .find(|r| self.resource_exposed(&r.id, r.exposed) && self.resource_uri(&r.id) == uri)
            .map(|r| (r.id.as_str(), None, r.mime_type.as_str()))
            .or_else(|| {
                config
                    .templates
                    .iter()
                    .filter(|r| self.resource_exposed(&r.id, r.exposed))
                    .find_map(|r| {
                        let prefix = format!("{}/templates/{}/", self.base(), r.id);
                        let key = uri.strip_prefix(&prefix)?;
                        r.entries.contains_key(key).then_some((
                            r.id.as_str(),
                            Some(key),
                            r.mime_type.as_str(),
                        ))
                    })
            })
            .ok_or(PortError::NotFound)?;
        let (id, key, mime) = matched;
        let (action, resource) = self.require(ctx, id, false).await?;
        let text = self.provider.read(ctx.operation.clone(), id, key).await?;
        self.governance.authorize(ctx, action, &resource).await?;
        self.governance
            .record(
                ctx,
                ctx.operation.request_id,
                action,
                &resource,
                AuditPhase::Succeeded,
            )
            .await?;
        Ok(ResourceRead {
            uri: uri.into(),
            mime_type: mime.into(),
            text,
        })
    }
    #[tracing::instrument(skip_all,fields(request_id=%ctx.operation.request_id))]
    pub async fn prompt(
        &self,
        ctx: &AccessContext,
        name: &str,
        args: &BTreeMap<String, String>,
    ) -> Result<(String, Vec<PromptText>), PortError> {
        self.profile_access(ctx).await?;
        let config = self.provider.catalog();
        let p = config
            .prompts
            .iter()
            .find(|p| self.prompt_exposed(&p.id, p.exposed) && self.prompt_name(&p.id) == name)
            .ok_or(PortError::NotFound)?;
        let (action, resource) = self.require(ctx, &p.id, true).await?;
        let result = self
            .provider
            .render(ctx.operation.clone(), &p.id, args)
            .await?;
        self.governance.authorize(ctx, action, &resource).await?;
        self.governance
            .record(
                ctx,
                ctx.operation.request_id,
                action,
                &resource,
                AuditPhase::Succeeded,
            )
            .await?;
        Ok((p.description.clone(), result))
    }
    pub async fn complete(
        &self,
        ctx: &AccessContext,
        prompt: bool,
        reference: &str,
        argument: &str,
        prefix: &str,
    ) -> Result<(Vec<String>, usize), PortError> {
        self.profile_access(ctx).await?;
        if prefix.len() > 4096 || argument.len() > 64 {
            return Err(PortError::InvalidInput {
                code: "invalid_completion",
            });
        }
        let config = self.provider.catalog();
        let (id, choices) = if prompt {
            let p = config
                .prompts
                .iter()
                .find(|p| {
                    self.prompt_exposed(&p.id, p.exposed) && self.prompt_name(&p.id) == reference
                })
                .ok_or(PortError::NotFound)?;
            let a =
                p.arguments
                    .iter()
                    .find(|a| a.name == argument)
                    .ok_or(PortError::InvalidInput {
                        code: "unknown_argument",
                    })?;
            (&p.id, a.choices.clone())
        } else {
            let t = config
                .templates
                .iter()
                .find(|t| {
                    self.resource_exposed(&t.id, t.exposed) && self.template_uri(t) == reference
                })
                .ok_or(PortError::NotFound)?;
            if t.parameter != argument {
                return Err(PortError::InvalidInput {
                    code: "unknown_argument",
                });
            }
            (&t.id, t.entries.keys().cloned().collect())
        };
        let (action, resource) = self.require(ctx, id, prompt).await?;
        let mut candidates: Vec<_> = choices
            .into_iter()
            .filter(|v| v.starts_with(prefix))
            .collect();
        candidates.sort();
        candidates.dedup();
        let total = candidates.len();
        candidates.truncate(50);
        self.governance
            .record(
                ctx,
                ctx.operation.request_id,
                action,
                &resource,
                AuditPhase::Succeeded,
            )
            .await?;
        Ok((candidates, total))
    }
}
