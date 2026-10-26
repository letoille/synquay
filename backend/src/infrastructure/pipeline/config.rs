use crate::application::ports::PortError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Configuration is operator-controlled. Secret values live only in environment variables.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    /// Only affects first initialization; existing managed routes are preserved.
    #[serde(default = "default_bootstrap_routes")]
    pub bootstrap_routes: bool,
    #[serde(default)]
    pub agent: Option<crate::domain::agent::AgentConfig>,
    #[serde(default)]
    pub webhook: super::webhook::WebhookConfig,
    pub retention: Option<crate::infrastructure::storage::maintenance::RetentionPolicy>,
    pub content: Option<crate::infrastructure::content::ContentConfig>,
    #[serde(default)]
    pub scheduler: super::scheduler::SchedulerConfig,
    #[serde(default)]
    pub additional_targets: Vec<TargetConfig>,
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub additional_sources: Vec<SourceConfig>,
    pub admin_token_env: Option<String>,
    pub operator_principal_id: Option<Uuid>,
    #[serde(default)]
    pub capacity: crate::infrastructure::storage::capacity::CapacityLimits,
    pub database: PathBuf,
    pub workspace_id: Uuid,
    pub source_connection_id: Uuid,
    pub target_connection_id: Uuid,
    pub destination_id: Uuid,
    pub principal_id: Uuid,
    pub rule_id: Uuid,
    pub webhook_token_env: String,
    pub destination_url: String,
    pub destination_token_env: Option<String>,
    #[serde(default)]
    pub allow_loopback_http: bool,
}
impl PipelineConfig {
    pub fn load(path: &Path) -> Result<Self, PortError> {
        use std::io::Read;
        let mut contents = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(1024 * 1024 + 1).read_to_end(&mut contents))
            .map_err(|_| PortError::Failed {
                code: "pipeline_config_read",
            })?;
        if contents.len() > 1024 * 1024 {
            return Err(PortError::PayloadTooLarge);
        }
        let mut config: Self =
            serde_json::from_slice(&contents).map_err(|_| PortError::InvalidInput {
                code: "pipeline_config_parse",
            })?;
        if config.database.is_relative() {
            config.database = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&config.database);
        }
        if let Some(content) = &mut config.content
            && content.directory.is_relative()
        {
            content.directory = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&content.directory);
        }
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), PortError> {
        self.webhook.validate()?;
        if self.agent.as_ref().is_some_and(|a| {
            !a.valid() || !self.additional_targets.is_empty() || self.content.is_some()
        }) {
            return Err(PortError::InvalidInput {
                code: "invalid_agent_pipeline",
            });
        }
        if let Some(content) = &self.content {
            content.validate()?;
        }
        if let Some(policy) = &self.retention {
            policy.validate()?;
        }
        self.capacity.validate()?;
        self.scheduler.validate()?;
        if self.additional_targets.len() > 31
            || self.admin_token_env.is_some() != self.operator_principal_id.is_some()
            || self
                .operator_principal_id
                .is_some_and(|x| x.is_nil() || x == self.principal_id)
        {
            return Err(PortError::InvalidInput {
                code: "pipeline_management_config",
            });
        }
        let mut connections =
            std::collections::HashSet::from([self.source_connection_id, self.target_connection_id]);
        let mut destinations = std::collections::HashSet::from([self.destination_id]);
        let mut rules = std::collections::HashSet::from([self.rule_id]);
        for target in &self.additional_targets {
            if target.rule_id.is_nil()
                || !rules.insert(target.rule_id)
                || target.connection_id.is_nil()
                || target.destination_id.is_nil()
                || !connections.insert(target.connection_id)
                || !destinations.insert(target.destination_id)
            {
                return Err(PortError::InvalidInput {
                    code: "duplicate_or_invalid_target",
                });
            }
        }
        if self.additional_sources.len() > 15
            || (self.agent.is_some() && !self.additional_sources.is_empty())
        {
            return Err(PortError::InvalidInput {
                code: "unsupported_sources",
            });
        }
        let valid_name =
            |s: &str| !s.trim().is_empty() && s.len() <= 120 && !s.chars().any(char::is_control);
        if self.source_name.as_deref().is_some_and(|s| !valid_name(s)) {
            return Err(PortError::InvalidInput {
                code: "source_name",
            });
        }
        for source in &self.additional_sources {
            if source.connection_id.is_nil()
                || !connections.insert(source.connection_id)
                || !valid_name(&source.name)
            {
                return Err(PortError::InvalidInput {
                    code: "duplicate_or_invalid_source",
                });
            }
        }
        if self.database.as_os_str().is_empty()
            || self.source_connection_id == self.target_connection_id
            || [
                self.workspace_id,
                self.source_connection_id,
                self.target_connection_id,
                self.destination_id,
                self.principal_id,
                self.rule_id,
            ]
            .iter()
            .any(Uuid::is_nil)
            || self.webhook_token_env.is_empty()
        {
            return Err(PortError::InvalidInput {
                code: "pipeline_config_invalid",
            });
        }
        Ok(())
    }
    pub fn secrets(&self) -> Result<(String, Option<String>), PortError> {
        let read = |name: &str| {
            std::env::var(name).map_err(|_| PortError::InvalidInput {
                code: "missing_pipeline_secret",
            })
        };
        let webhook = read(&self.webhook_token_env)?;
        if webhook.len() < 32
            || webhook.len() > 4096
            || !webhook.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(PortError::InvalidInput {
                code: "invalid_webhook_secret",
            });
        }
        let destination = self
            .destination_token_env
            .as_ref()
            .map(|name| read(name))
            .transpose()?;
        Ok((webhook, destination))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetConfig {
    pub rule_id: Uuid,
    pub connection_id: Uuid,
    pub destination_id: Uuid,
    pub url: String,
    pub token_env: Option<String>,
    #[serde(default)]
    pub allow_loopback_http: bool,
}

#[derive(Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub connection_id: Uuid,
    pub name: String,
}

fn default_bootstrap_routes() -> bool {
    true
}
