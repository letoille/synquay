//! Online exposure control over the trusted startup inventory. Publication waits
//! for admitted calls to finish; after it commits, no call can read an old profile.
use super::{ToolRuntime, config::ToolsConfig};
use crate::application::ports::{PortError, publications::*};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

pub fn adopt_snapshot(config: &mut ToolsConfig, record: &Value) -> Result<(), PortError> {
    fn inventory(mut value: Value) -> Value {
        if let Some(tools) = value.get_mut("tools").and_then(Value::as_array_mut) {
            for tool in tools {
                if let Some(tool) = tool.as_object_mut() {
                    tool.remove("exposed");
                }
            }
        }
        value
    }
    if inventory(config.snapshot()) != inventory(record["config"].clone()) {
        return Err(PortError::Conflict);
    }
    let profile: crate::domain::profile::PublishedProfile =
        serde_json::from_value(record["profile"].clone()).map_err(|_| PortError::Unavailable)?;
    config.revision = profile.revision;
    for tool in &mut config.tools {
        let entry = record["config"]["tools"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|t| t["public_name"].as_str() == Some(&tool.public_name))
            })
            .ok_or(PortError::Unavailable)?;
        tool.exposed = entry["exposed"].as_bool().ok_or(PortError::Unavailable)?;
    }
    config.validate()?;
    let mut http_profile = record["profile"].clone();
    if let Some(tools) = http_profile["tools"].as_array_mut() {
        tools.retain(|t| {
            !t["public_name"]
                .as_str()
                .is_some_and(|n| n.starts_with("mqtt_"))
                || config
                    .tools
                    .iter()
                    .any(|h| t["public_name"] == h.public_name)
        });
    }
    if serde_json::to_value(config.profile()).map_err(|_| PortError::Unavailable)? != http_profile {
        return Err(PortError::Conflict);
    }
    Ok(())
}
impl ToolRuntime {
    pub(super) async fn publication_scope(&self) -> Result<PublicationScope, PortError> {
        self.workspace_admin(false).await?;

        Ok(PublicationScope {
            workspace_id: self.config.workspace_id,
            profile_id: self.config.profile_id,
            actor_id: self.console_actor(),
        })
    }
    pub async fn control_view(&self) -> Result<Value, PortError> {
        let scope = self.publication_scope().await?;
        let _guard = self.publication_gate.read().await;
        let mut config = self.inventory_snapshot()?;
        let record = self
            .store
            .tools_snapshot(config.workspace_id, config.profile_id)
            .await?
            .ok_or(PortError::NotFound)?;
        adopt_snapshot(&mut config, &record)?;
        let draft = self.store.draft(scope.clone()).await?;
        let mut tool_views = config
            .tools
            .iter()
            .map(|t| serde_json::json!(t))
            .collect::<Vec<_>>();
        tool_views.extend(self.device_tool_views().await?);
        Ok(
            serde_json::json!({"workspace":self.workspace_admin(false).await?.workspace,"revision":config.revision,"profile_id":config.profile_id,"admin_id":scope.actor_id,"tools":tool_views,"draft":draft,
                "content": {
                    "revision":config.mcp_content.revision,
                    "resources":config.mcp_content.resources.iter().map(|r|serde_json::json!({"id":r.id,"description":r.description,"mime_type":r.mime_type,"kind":"resource","parameter":null}))
                        .chain(config.mcp_content.templates.iter().map(|r|serde_json::json!({"id":r.id,"description":r.description,"mime_type":r.mime_type,"kind":"template","parameter":r.parameter}))).collect::<Vec<_>>(),
                    "prompts":config.mcp_content.prompts.iter().map(|p|serde_json::json!({"id":p.id,"description":p.description,"arguments":p.arguments})).collect::<Vec<_>>()
                },
                "connection":{"resource":self.security.oauth.as_ref().map(|o| &o.resource),
                    "authentication":if self.security.oauth.is_some(){"oauth"}else{"bearer"},
                    "issuer":self.security.oauth.as_ref().map(|o| &o.issuer)}}),
        )
    }
    pub async fn publication_history(
        &self,
        before: Option<u64>,
        limit: u32,
    ) -> Result<PublicationHistory, PortError> {
        self.store
            .history(self.publication_scope().await?, before, limit)
            .await
    }
    /// Absence means unconfirmed, not failed: an admitted publisher may still run.
    /// No publication gate here, so reconciliation remains available during a wait.
    pub async fn publication_result(
        &self,
        request: PublishInput,
    ) -> Result<Option<PublicationReceipt>, PortError> {
        if request.operation_id.is_nil()
            || request.expected_revision == 0
            || request.expected_revision >= i64::MAX as u64
            || request.draft_generation == 0
            || request.draft_generation >= i64::MAX as u64
        {
            return Err(PortError::InvalidInput {
                code: "invalid_publication_query",
            });
        }
        self.store
            .published_operation(self.publication_scope().await?, request)
            .await
    }
    pub async fn save_exposure_draft(
        &self,
        mut input: DraftInput,
    ) -> Result<ExposureDraft, PortError> {
        self.workspace_admin(true).await?;
        let scope = self.publication_scope().await?;
        let inventory = self.inventory_snapshot()?;
        let devices = self.device_tools().await?;
        let mut names = HashSet::new();
        if input.exposed_tools.len() > 32
            || !input.exposed_tools.iter().all(|n| {
                names.insert(n.clone())
                    && (inventory.tools.iter().any(|t| &t.public_name == n)
                        || devices.iter().any(|t| &t.public_name == n))
            })
        {
            return Err(PortError::InvalidInput {
                code: "unknown_or_duplicate_tool",
            });
        }
        input.exposed_tools.sort();
        let _guard = self.publication_gate.read().await;
        self.store.save_draft(scope, input).await
    }
    pub async fn publish_exposure_draft(
        self: &Arc<Self>,
        request: PublishInput,
    ) -> Result<PublicationReceipt, PortError> {
        self.publication_scope().await?;
        if request.operation_id.is_nil()
            || request.expected_revision == 0
            || request.expected_revision >= i64::MAX as u64 - 1
            || request.draft_generation == 0
            || request.draft_generation >= i64::MAX as u64
        {
            return Err(PortError::InvalidInput {
                code: "invalid_operation_id",
            });
        }
        // Bound publishers before spawning. An HTTP disconnect must not drop the
        // exclusive gate while a spawn_blocking transaction is still committing.
        let permit = self
            .publish_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| PortError::RateLimited {
                retry_after: std::time::Duration::from_secs(1),
            })?;
        let runtime = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _guard = runtime.publication_gate.write().await;
            let _devices_guard = runtime.store.device_io.read().await;
            runtime.workspace_admin(true).await?;
            let scope = runtime.publication_scope().await?;
            if let Some(receipt) = runtime
                .store
                .published_operation(scope.clone(), request.clone())
                .await?
            {
                return Ok(receipt);
            }
            let draft = runtime
                .store
                .draft(scope.clone())
                .await?
                .ok_or(PortError::NotFound)?;
            if draft.generation != request.draft_generation
                || draft.base_revision != request.expected_revision
            {
                return Err(PortError::Conflict);
            }
            let record = runtime
                .store
                .tools_snapshot(scope.workspace_id, scope.profile_id)
                .await?
                .ok_or(PortError::NotFound)?;
            let mut config = runtime.inventory_snapshot()?;
            adopt_snapshot(&mut config, &record)?;
            if config.revision != request.expected_revision {
                return Err(PortError::Conflict);
            }
            config.revision = config.revision.checked_add(1).ok_or(PortError::Conflict)?;
            for tool in &mut config.tools {
                tool.exposed = draft.exposed_tools.contains(&tool.public_name);
            }
            config.validate()?;
            let mut profile = config.profile();
            profile.tools.extend(
                runtime
                    .device_tools()
                    .await?
                    .into_iter()
                    .filter(|t| draft.exposed_tools.contains(&t.public_name)),
            );
            runtime
                .store
                .publish_draft(PublicationPlan {
                    scope,
                    request,
                    profile,
                    snapshot: config.snapshot(),
                })
                .await
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)?
    }
}
