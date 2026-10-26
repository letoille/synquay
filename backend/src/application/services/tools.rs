use super::governance::Governance;
use crate::{
    application::{
        connectors::ConnectorRegistry,
        ports::{
            PortError,
            identity::AccessContext,
            invocations::InvocationLedger,
            profiles::{ProfileRepository, SchemaValidator},
        },
    },
    domain::{
        identity::{Action, Resource},
        profile::PublishedProfile,
        tool::{ToolDefinition, ToolResult},
    },
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub struct PublishedToolCall {
    pub profile_id: Uuid,
    pub revision: u64,
    pub public_name: String,
    pub invocation_id: Uuid,
    pub arguments: Value,
}

pub struct PublishedToolCatalog {
    pub profile_id: Uuid,
    pub revision: u64,
    pub tools: Vec<ToolDefinition>,
}

/// Once dispatched, return execution outcome independently of completion-audit health.
/// A missing completion audit must never masquerade as an unexecuted/retryable call.
pub struct ToolInvocationReport {
    pub outcome: Result<ToolResult, PortError>,
    pub audit_recorded: bool,
}

pub struct ToolService {
    profiles: Arc<dyn ProfileRepository>,
    validator: Arc<dyn SchemaValidator>,
    connectors: Arc<ConnectorRegistry>,
    governance: Arc<Governance>,
    ledger: Option<Arc<dyn InvocationLedger>>,
    publication_gate: Option<Arc<tokio::sync::RwLock<()>>>,
}
impl ToolService {
    pub fn new(
        profiles: Arc<dyn ProfileRepository>,
        validator: Arc<dyn SchemaValidator>,
        connectors: Arc<ConnectorRegistry>,
        governance: Arc<Governance>,
    ) -> Self {
        Self {
            profiles,
            validator,
            connectors,
            governance,
            ledger: None,
            publication_gate: None,
        }
    }

    pub fn with_ledger(mut self, ledger: Arc<dyn InvocationLedger>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    pub fn with_publication_gate(mut self, gate: Arc<tokio::sync::RwLock<()>>) -> Self {
        self.publication_gate = Some(gate);
        self
    }

    async fn profile(
        &self,
        ctx: &AccessContext,
        profile_id: Uuid,
    ) -> Result<PublishedProfile, PortError> {
        self.governance
            .authorize(
                ctx,
                Action::UseProfile,
                &Resource::Profile {
                    workspace_id: ctx.identity.workspace_id,
                    profile_id,
                },
            )
            .await?;
        let profile = self
            .profiles
            .published(ctx.identity.workspace_id, profile_id)
            .await?;
        if profile.workspace_id != ctx.identity.workspace_id || profile.profile_id != profile_id {
            return Err(PortError::ScopeMismatch);
        }
        if !profile.is_valid() {
            return Err(PortError::InvalidInput {
                code: "invalid_published_profile",
            });
        }
        Ok(profile)
    }

    pub async fn list(
        &self,
        ctx: &AccessContext,
        profile_id: Uuid,
    ) -> Result<PublishedToolCatalog, PortError> {
        let _guard = match &self.publication_gate {
            Some(gate) => Some(gate.read().await),
            None => None,
        };
        let profile = self.profile(ctx, profile_id).await?;
        let mut tools = vec![];
        for binding in profile.tools {
            let resource = Resource::Tool {
                scope: binding.scope,
                tool_id: binding.definition.id.clone(),
            };
            match self
                .governance
                .authorize(ctx, Action::InvokeTool, &resource)
                .await
            {
                Ok(()) => {
                    let mut definition = binding.definition;
                    definition.id = binding.public_name;
                    tools.push(definition);
                }
                Err(PortError::PermissionDenied) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(PublishedToolCatalog {
            profile_id,
            revision: profile.revision,
            tools,
        })
    }

    #[tracing::instrument(skip_all, fields(request_id = %ctx.operation.request_id, invocation_id = %call.invocation_id))]
    pub async fn invoke(
        &self,
        ctx: &AccessContext,
        call: PublishedToolCall,
    ) -> Result<ToolInvocationReport, PortError> {
        let _guard = match &self.publication_gate {
            Some(gate) => Some(gate.read().await),
            None => None,
        };
        let profile = self.profile(ctx, call.profile_id).await?;
        if profile.revision != call.revision {
            return Err(PortError::Conflict);
        }
        let binding = profile
            .tools
            .into_iter()
            .find(|b| b.public_name == call.public_name)
            .ok_or(PortError::NotFound)?;
        let executor = self
            .connectors
            .get(binding.scope)?
            .tools
            .clone()
            .ok_or(PortError::Unsupported)?;
        let intent = serde_json::json!({
            "profile":call.profile_id,"revision":call.revision,
            "tool":call.public_name,"scope":binding.scope,"arguments":call.arguments
        });
        super::invocation::InvocationService {
            validator: self.validator.clone(),
            governance: self.governance.clone(),
            ledger: self.ledger.clone(),
        }
        .invoke(
            ctx,
            super::invocation::InvocationRequest {
                scope: binding.scope,
                definition: binding.definition,
                executor,
                invocation_id: call.invocation_id,
                arguments: call.arguments,
                intent,
            },
        )
        .await
    }
}
