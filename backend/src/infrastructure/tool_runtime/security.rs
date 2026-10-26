//! Resource-server credentials and trusted identity bindings. No token issuance.
use super::config::ToolsConfig;
use crate::application::ports::PortError;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthGrant {
    #[serde(default)]
    pub agent_jobs: bool,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub prompts: Vec<String>,
    pub subject: String,
    pub oauth_client_id: String,
    pub principal_id: Uuid,
    pub client_id: Uuid,
    pub tools: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthConfig {
    pub issuer: String,
    pub resource: String,
    pub introspection_endpoint: String,
    pub introspection_client_id: String,
    pub introspection_secret_env: String,
    pub required_scope: String,
    pub grants: Vec<OAuthGrant>,
    #[serde(default)]
    pub allow_loopback_http: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConfig {
    pub principal_id: Uuid,
    /// Legacy configuration key, accepted for compatibility but never read.
    #[serde(default, skip_serializing)]
    pub token_env: String,
    /// Exact serialized origins; empty preserves native-client-only access.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    pub oauth: Option<OAuthConfig>,
    pub admin: Option<AdminConfig>,
}
impl SecurityConfig {
    pub fn load(path: &Path) -> Result<Self, PortError> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|_| PortError::Unavailable)?;
        let mut bytes = Vec::new();
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| PortError::Unavailable)?;
        if bytes.len() > 65536 {
            return Err(PortError::PayloadTooLarge);
        }
        serde_json::from_slice(&bytes).map_err(|_| PortError::InvalidInput {
            code: "invalid_security_config",
        })
    }
    pub fn validate(&self, tools: &ToolsConfig) -> Result<(), PortError> {
        let invalid = || PortError::InvalidInput {
            code: "invalid_security_config",
        };
        if let Some(admin) = &self.admin
            && (admin.principal_id.is_nil() || admin.principal_id == tools.principal_id)
        {
            return Err(invalid());
        }
        if let Some(admin) = &self.admin {
            let mut seen = HashSet::new();
            if admin.allowed_origins.len() > 8 {
                return Err(invalid());
            }
            for origin in &admin.allowed_origins {
                let u = reqwest::Url::parse(origin).map_err(|_| invalid())?;
                let loopback = matches!(u.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
                if origin.len() > 256
                    || !seen.insert(origin)
                    || !(u.scheme() == "https" || (u.scheme() == "http" && loopback))
                    || u.origin().ascii_serialization() != *origin
                    || !u.username().is_empty()
                    || u.password().is_some()
                    || u.path() != "/"
                    || u.query().is_some()
                    || u.fragment().is_some()
                {
                    return Err(invalid());
                }
            }
        }
        if let Some(o) = &self.oauth {
            for url in [&o.issuer, &o.resource, &o.introspection_endpoint] {
                let u = reqwest::Url::parse(url).map_err(|_| invalid())?;
                if !((u.scheme() == "https")
                    || (o.allow_loopback_http
                        && u.scheme() == "http"
                        && matches!(u.host_str(), Some("127.0.0.1" | "[::1]" | "::1"))))
                    || !u.username().is_empty()
                    || u.password().is_some()
                    || u.query().is_some()
                    || u.fragment().is_some()
                    || u.host_str().is_none()
                    || u.as_str() != url
                {
                    return Err(invalid());
                }
            }
            if reqwest::Url::parse(&o.resource)
                .map_err(|_| invalid())?
                .path()
                != "/mcp"
            {
                return Err(invalid());
            }
            if o.required_scope.is_empty()
                || o.required_scope.len() > 128
                || !o
                    .required_scope
                    .bytes()
                    .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b))
            {
                return Err(invalid());
            }
            if o.introspection_client_id.is_empty()
                || o.introspection_client_id.len() > 256
                || !o
                    .introspection_client_id
                    .bytes()
                    .all(|b| (33..=126).contains(&b))
                || o.grants.len() > 128
            {
                return Err(invalid());
            }
            let mut bindings = HashSet::new();
            let mut principals = HashSet::new();
            for g in &o.grants {
                if g.subject.is_empty()
                    || g.subject.len() > 256
                    || g.oauth_client_id.is_empty()
                    || g.oauth_client_id.len() > 256
                    || g.principal_id.is_nil()
                    || g.client_id.is_nil()
                    || g.principal_id == tools.principal_id
                    || self
                        .admin
                        .as_ref()
                        .is_some_and(|a| a.principal_id == g.principal_id)
                    || !bindings.insert((&g.subject, &g.oauth_client_id))
                    || !principals.insert(g.principal_id)
                    || g.tools.len() > 32
                {
                    return Err(invalid());
                }
                let mut resources = HashSet::new();
                let mut prompts = HashSet::new();
                if g.resources.len() > 32
                    || g.prompts.len() > 32
                    || !g.resources.iter().all(|id| {
                        resources.insert(id)
                            && (tools.mcp_content.resources.iter().any(|r| &r.id == id)
                                || tools.mcp_content.templates.iter().any(|t| &t.id == id))
                    })
                    || !g.prompts.iter().all(|id| {
                        prompts.insert(id) && tools.mcp_content.prompts.iter().any(|p| &p.id == id)
                    })
                {
                    return Err(invalid());
                }
                let mut names = HashSet::new();
                if !g
                    .tools
                    .iter()
                    .all(|n| names.insert(n) && tools.tools.iter().any(|t| &t.public_name == n))
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}
