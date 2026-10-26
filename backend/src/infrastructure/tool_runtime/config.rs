use crate::{
    application::ports::PortError,
    domain::{
        connector::ConnectionScope,
        profile::{PublishedProfile, PublishedTool},
        tool::ToolDefinition,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpToolConfig {
    pub public_name: String,
    pub connection_id: Uuid,
    pub definition: ToolDefinition,
    pub url: String,
    pub bearer_env: Option<String>,
    pub exposed: bool,
    /// Local HTTP is opt-in; external endpoints must use HTTPS.
    #[serde(default)]
    pub allow_loopback_http: bool,
}
impl HttpToolConfig {
    /// Adapt the existing file format into the shared capability contract.
    pub fn capability(
        &self,
        workspace_id: Uuid,
        revision: u64,
    ) -> crate::domain::capability::binding::BoundCapability {
        use crate::domain::capability::{
            binding::{BoundCapability, ProtocolBinding},
            *,
        };
        BoundCapability {
            descriptor: CapabilityDescriptor {
                reference: CapabilityRef {
                    scope: ConnectionScope {
                        workspace_id,
                        connection_id: self.connection_id,
                    },
                    id: self.definition.id.clone(),
                    revision,
                },
                origin: DefinitionOrigin::Operator,
                resource: None,
                spec: CapabilitySpec::Action {
                    tool: self.definition.clone(),
                    execution: ExecutionSemantics {
                        mode: ExecutionMode::Inline,
                        completion: CompletionCondition::ResponseValidated,
                        idempotency: Idempotency::Unknown,
                        timeout_ms: 20_000,
                    },
                },
            },
            binding_revision: revision,
            binding: ProtocolBinding::HttpJsonPost {
                url: self.url.clone(),
                allow_loopback_http: self.allow_loopback_http,
            },
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    #[serde(default)]
    pub agent_jobs: bool,
    #[serde(default)]
    pub mcp_content: crate::domain::mcp_content::McpContentConfig,
    pub database: PathBuf,
    pub workspace_id: Uuid,
    pub profile_id: Uuid,
    pub principal_id: Uuid,
    pub token_env: String,
    pub revision: u64,
    pub tools: Vec<HttpToolConfig>,
}
impl ToolsConfig {
    pub fn load(path: &Path) -> Result<Self, PortError> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|_| PortError::Unavailable)?;
        let mut bytes = Vec::new();
        file.take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| PortError::Unavailable)?;
        if bytes.len() > 256 * 1024 {
            return Err(PortError::PayloadTooLarge);
        }
        serde_json::from_slice(&bytes).map_err(|_| PortError::InvalidInput {
            code: "invalid_tools_config",
        })
    }
    pub fn profile(&self) -> PublishedProfile {
        PublishedProfile {
            workspace_id: self.workspace_id,
            profile_id: self.profile_id,
            revision: self.revision,
            tools: self
                .tools
                .iter()
                .filter(|t| t.exposed)
                .map(|t| PublishedTool {
                    public_name: t.public_name.clone(),
                    scope: ConnectionScope {
                        workspace_id: self.workspace_id,
                        connection_id: t.connection_id,
                    },
                    definition: t.definition.clone(),
                })
                .collect(),
        }
    }
    pub fn validate(&self) -> Result<(), PortError> {
        let invalid = || PortError::InvalidInput {
            code: "invalid_tools_config",
        };
        if !self.mcp_content.is_valid()
            || self.revision == 0
            || self.revision >= i64::MAX as u64
            || self.tools.len() > 32
            || self.workspace_id.is_nil()
            || self.profile_id.is_nil()
            || self.principal_id.is_nil()
        {
            return Err(invalid());
        }
        let mut connections = HashSet::new();
        let mut names = HashSet::new();
        for t in &self.tools {
            if t.public_name.len() > 100
                || t.definition
                    .input_schema
                    .get("type")
                    .and_then(|v| v.as_str())
                    != Some("object")
                || t.connection_id.is_nil()
                || !connections.insert(t.connection_id)
                || !names.insert(&t.public_name)
            {
                return Err(invalid());
            }
            if !t.capability(self.workspace_id, self.revision).is_valid() {
                return Err(invalid());
            }
            // Validate hidden entries too so later publication cannot enable invalid definitions.
            let check = PublishedProfile {
                workspace_id: self.workspace_id,
                profile_id: self.profile_id,
                revision: self.revision,
                tools: vec![PublishedTool {
                    public_name: t.public_name.clone(),
                    scope: ConnectionScope {
                        workspace_id: self.workspace_id,
                        connection_id: t.connection_id,
                    },
                    definition: t.definition.clone(),
                }],
            };
            if !check.is_valid() {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn snapshot(&self) -> serde_json::Value {
        // Env references, never resolved credentials. Path relocation is operational.
        serde_json::json!({"workspace_id":self.workspace_id,"profile_id":self.profile_id,
            "principal_id":self.principal_id,"token_env":self.token_env,"tools":self.tools})
    }
}
pub fn secret(name: &str) -> Result<String, PortError> {
    secret_value(name, 32)
}
pub fn secret_value(name: &str, min_len: usize) -> Result<String, PortError> {
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(PortError::InvalidInput {
            code: "invalid_secret_reference",
        });
    }
    let value = std::env::var(name).map_err(|_| PortError::InvalidInput {
        code: "missing_tool_secret",
    })?;
    if value.len() < min_len
        || value.len() > 4096
        || !value.bytes().all(|b| (33..=126).contains(&b))
    {
        return Err(PortError::InvalidInput {
            code: "invalid_tool_secret",
        });
    }
    Ok(value)
}
