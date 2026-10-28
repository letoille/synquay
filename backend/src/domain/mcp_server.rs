//! An independently named MCP endpoint and its published tool scope.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServer {
    pub id: Uuid,
    #[serde(default)]
    pub builtin_scope: super::builtin_tools::BuiltinScope,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub public_url: Option<String>,
    #[serde(default)]
    pub content_scope: Option<ContentScope>,
    pub enabled: bool,
    pub deleted: bool,
    pub revision: u64,
    #[serde(default)]
    pub auth: ServerAuth,
    #[serde(default)]
    pub transport: ServerTransport,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin_scope: Option<super::builtin_tools::BuiltinScope>,
    #[serde(default)]
    pub expected_content_revision: Option<u64>,
    pub operation_id: Uuid,
    pub expected_revision: u64,
    pub expected_tool_revision: u64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub public_url: Option<String>,
    #[serde(default)]
    pub content_scope: Option<ContentScope>,
    pub enabled: bool,
    pub exposed_tools: Vec<String>,
    #[serde(default)]
    pub auth: Option<ServerAuth>,
    #[serde(default)]
    pub transport: ServerTransport,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerDeletion {
    pub operation_id: Uuid,
    pub expected_revision: u64,
}

/// Only public configuration and secret references are persisted or audited.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerAuth {
    #[default]
    Inherited,
    None,
    Basic {
        username: String,
        password_env: String,
    },
    Jwt {
        issuer: String,
        audience: String,
        subject: String,
        public_key_pem: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        issuer: String,
        resource: String,
        introspection_endpoint: String,
        introspection_client_id: String,
        introspection_secret_env: String,
        required_scope: String,
        subject: String,
        oauth_client_id: String,
        #[serde(default)]
        allow_loopback_http: bool,
    },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerTransport {
    #[default]
    StreamableHttp,
}

impl McpServer {
    pub fn path(&self, default_id: Uuid) -> String {
        self.public_url
            .as_ref()
            .and_then(|s| url::Url::parse(s).ok())
            .map(|u| u.path().to_owned())
            .unwrap_or_else(|| {
                if self.id == default_id {
                    "/mcp".into()
                } else {
                    format!("/mcp/{}", self.id)
                }
            })
    }
}

/// Explicit per-service exposure; absence preserves legacy default-service behavior.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentScope {
    pub resources: Vec<String>,
    pub prompts: Vec<String>,
}
impl ContentScope {
    pub fn is_valid(&self, catalog: &crate::domain::mcp_content::McpContentConfig) -> bool {
        let mut resources = std::collections::HashSet::new();
        let mut prompts = std::collections::HashSet::new();
        self.resources.len() <= 32
            && self.prompts.len() <= 32
            && self.resources.iter().all(|id| {
                resources.insert(id)
                    && (catalog.resources.iter().any(|r| &r.id == id)
                        || catalog.templates.iter().any(|r| &r.id == id))
            })
            && self
                .prompts
                .iter()
                .all(|id| prompts.insert(id) && catalog.prompts.iter().any(|p| &p.id == id))
    }
}
impl McpServer {
    pub fn effective_content_scope(
        &self,
        default_id: Uuid,
        catalog: &crate::domain::mcp_content::McpContentConfig,
    ) -> ContentScope {
        self.content_scope.clone().unwrap_or_else(|| {
            if self.id != default_id {
                return ContentScope::default();
            }
            ContentScope {
                resources: catalog
                    .resources
                    .iter()
                    .filter(|r| r.exposed)
                    .map(|r| r.id.clone())
                    .chain(
                        catalog
                            .templates
                            .iter()
                            .filter(|r| r.exposed)
                            .map(|r| r.id.clone()),
                    )
                    .collect(),
                prompts: catalog
                    .prompts
                    .iter()
                    .filter(|p| p.exposed)
                    .map(|p| p.id.clone())
                    .collect(),
            }
        })
    }
}
