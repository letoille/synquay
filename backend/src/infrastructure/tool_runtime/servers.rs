//! Persistent MCP endpoint management; all endpoints reuse the trusted connector inventory.
use super::ToolRuntime;
use crate::{
    application::ports::{PortError, profiles::ProfileRepository},
    domain::mcp_server::*,
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

impl ToolRuntime {
    pub fn endpoint_content(
        &self,
        server: &McpServer,
    ) -> crate::application::services::mcp_content::McpContentService {
        self.content.scoped(
            server.id,
            server.effective_content_scope(self.config.profile_id, &self.config.mcp_content),
        )
    }
    pub async fn endpoint(&self, id: Uuid) -> Result<McpServer, PortError> {
        let server = self.store.mcp_server(self.config.workspace_id, id).await?;
        if server.deleted || !server.enabled {
            return Err(PortError::NotFound);
        }
        Ok(server)
    }
    pub async fn endpoint_by_path(&self, path: &str) -> Result<McpServer, PortError> {
        let path = path
            .strip_prefix("/.well-known/oauth-protected-resource")
            .unwrap_or(path);
        let path = if path.is_empty() {
            "/mcp"
        } else {
            path.trim_end_matches('/')
        };
        let server = self
            .store
            .mcp_server_by_path(self.config.workspace_id, path.into())
            .await?;
        if !server.enabled {
            return Err(PortError::NotFound);
        }
        Ok(server)
    }
    pub async fn server_view(&self, id: Uuid) -> Result<Value, PortError> {
        self.workspace_admin(false).await?;
        let _gate = self.publication_gate.read().await;
        let server = self.store.mcp_server(self.config.workspace_id, id).await?;
        self.server_json(server).await
    }
    async fn server_json(&self, server: McpServer) -> Result<Value, PortError> {
        let profile = self
            .store
            .published(self.config.workspace_id, server.id)
            .await?;
        let content_scope =
            server.effective_content_scope(self.config.profile_id, &self.config.mcp_content);
        let default = server.id == self.config.profile_id;
        let path = server.path(self.config.profile_id);
        let address = if server.public_url.is_some() {
            server.public_url.clone()
        } else if let ServerAuth::OAuth { resource, .. } = &server.auth {
            Some(resource.clone())
        } else {
            self.security.oauth.as_ref().map(|o| {
                if default {
                    o.resource.clone()
                } else {
                    format!("{}/{}", o.resource.trim_end_matches('/'), server.id)
                }
            })
        };
        let authentication = match &server.auth {
            ServerAuth::Inherited => {
                if self.oauth.is_some() {
                    "oauth"
                } else {
                    "bearer"
                }
            }
            ServerAuth::None => "none",
            ServerAuth::Basic { .. } => "basic",
            ServerAuth::Jwt { .. } => "jwt",
            ServerAuth::OAuth { .. } => "oauth",
        };
        Ok(
            json!({"id":server.id,"name":server.name,"description":server.description,"enabled":server.enabled,"deleted":server.deleted,
            "public_url":server.public_url,"revision":server.revision,"tool_revision":profile.revision,"path":path,"address":address,
            "content_scope":content_scope,"content_revision":self.config.mcp_content.revision,"builtin_scope":server.builtin_scope,
            "exposed_tools":profile.tools.into_iter().map(|t|t.public_name).collect::<Vec<_>>(),
            "is_default":default,"authentication":authentication,"auth":server.auth,"transport":server.transport}),
        )
    }
    pub async fn list_servers(&self) -> Result<Value, PortError> {
        self.workspace_admin(false).await?;
        let _gate = self.publication_gate.read().await;
        let mut items = Vec::new();
        for server in self.store.mcp_servers(self.config.workspace_id).await? {
            items.push(self.server_json(server).await?);
        }
        Ok(json!({"items":items}))
    }
    pub async fn save_server(
        self: &Arc<Self>,
        id: Uuid,
        input: ServerChange,
    ) -> Result<Value, PortError> {
        self.workspace_admin(true).await?;
        self.refresh_device_tools().await?;
        if id.is_nil()
            || input.operation_id.is_nil()
            || input.expected_revision >= i64::MAX as u64 - 1
            || input.expected_tool_revision >= i64::MAX as u64 - 1
            || input.name.trim().is_empty()
            || input.name.len() > 120
            || input.name != input.name.trim()
            || input
                .description
                .as_ref()
                .is_some_and(|s| s.len() > 600 || s.chars().any(char::is_control))
            || input.name.chars().any(char::is_control)
        {
            return Err(PortError::InvalidInput {
                code: "invalid_mcp_server",
            });
        }
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
            let _gate = runtime.publication_gate.write().await;
            runtime.workspace_admin(true).await?;
            let _devices_guard = runtime.store.device_io.read().await;
            let mut config = runtime.inventory_snapshot()?;
            let devices = runtime.device_tools().await?;
            let mut names = HashSet::new();
            if input.exposed_tools.len() > 32
                || !input.exposed_tools.iter().all(|name| {
                    names.insert(name)
                        && (config.tools.iter().any(|t| &t.public_name == name)
                            || devices.iter().any(|t| &t.public_name == name))
                })
            {
                return Err(PortError::InvalidInput {
                    code: "unknown_or_duplicate_tool",
                });
            }
            let content_scope = match &input.content_scope {
                Some(scope) => {
                    if input.expected_content_revision != Some(config.mcp_content.revision) {
                        return Err(PortError::Conflict);
                    }
                    if !scope.is_valid(&config.mcp_content) {
                        return Err(PortError::InvalidInput {
                            code: "unknown_or_duplicate_content",
                        });
                    }
                    Some(scope.clone())
                }
                None if input.expected_revision == 0 => Some(ContentScope::default()),
                None => {
                    runtime
                        .store
                        .mcp_server(runtime.config.workspace_id, id)
                        .await?
                        .content_scope
                }
            };
            config.profile_id = id;
            config.revision = input.expected_tool_revision + 1;
            for tool in &mut config.tools {
                tool.exposed = input.exposed_tools.contains(&tool.public_name);
            }
            let auth = match &input.auth {
                Some(auth) => auth.clone(),
                None if input.expected_revision == 0 => ServerAuth::Inherited,
                None => {
                    runtime
                        .store
                        .mcp_server(runtime.config.workspace_id, id)
                        .await?
                        .auth
                }
            };
            let public_url = match &input.public_url {
                Some(value) => {
                    validate_public_url(value)?;
                    Some(value.clone())
                }
                None if input.expected_revision == 0 => None,
                None => {
                    runtime
                        .store
                        .mcp_server(runtime.config.workspace_id, id)
                        .await?
                        .public_url
                }
            };
            let builtin_scope = match &input.builtin_scope {
                Some(scope) => {
                    runtime
                        .store
                        .validate_builtin_scope(runtime.config.workspace_id, scope.clone())
                        .await?;
                    scope.clone()
                }
                None if input.expected_revision == 0 => Default::default(),
                None => {
                    runtime
                        .store
                        .mcp_server(runtime.config.workspace_id, id)
                        .await?
                        .builtin_scope
                }
            };
            let server = McpServer {
                id,
                builtin_scope,
                name: input.name.clone(),
                public_url,
                content_scope,
                description: match &input.description {
                    Some(v) => v.trim().into(),
                    None if input.expected_revision == 0 => String::new(),
                    None => {
                        runtime
                            .store
                            .mcp_server(runtime.config.workspace_id, id)
                            .await?
                            .description
                    }
                },
                enabled: input.enabled,
                deleted: false,
                revision: input.expected_revision + 1,
                auth,
                transport: input.transport.clone(),
            };
            let route_path = server.path(runtime.config.profile_id);
            runtime.prepare_endpoint_auth(id, &server.auth, &route_path)?;
            if let ServerAuth::OAuth { resource, .. } = &server.auth
                && server.public_url.as_ref().is_some_and(|u| u != resource)
            {
                return Err(PortError::InvalidInput {
                    code: "invalid_server_auth",
                });
            }
            // Inherited OAuth uses deployment-wide resource/audience configuration.
            if matches!(server.auth, ServerAuth::Inherited)
                && runtime.oauth.is_some()
                && server.public_url.as_ref().is_some_and(|u| {
                    runtime.security.oauth.as_ref().is_some_and(|o| {
                        let expected = if id == runtime.config.profile_id {
                            o.resource.clone()
                        } else {
                            format!("{}/{}", o.resource.trim_end_matches('/'), id)
                        };
                        u != &expected
                    })
                })
            {
                return Err(PortError::InvalidInput {
                    code: "invalid_server_auth",
                });
            }
            let mut profile = config.profile();
            profile.tools.extend(
                devices
                    .into_iter()
                    .filter(|t| input.exposed_tools.contains(&t.public_name)),
            );
            runtime
                .store
                .commit_mcp_server(
                    runtime.config.workspace_id,
                    id,
                    runtime.console_actor(),
                    input.operation_id,
                    input.expected_revision,
                    input.expected_tool_revision,
                    json!(input),
                    server,
                    route_path,
                    Some((profile, config.snapshot())),
                )
                .await
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)??;
        self.server_view(id).await
    }
    pub async fn delete_server(
        self: &Arc<Self>,
        id: Uuid,
        input: ServerDeletion,
    ) -> Result<Value, PortError> {
        self.workspace_admin(true).await?;
        self.refresh_device_tools().await?;
        if id.is_nil()
            || input.operation_id.is_nil()
            || input.expected_revision == 0
            || input.expected_revision >= i64::MAX as u64 - 1
        {
            return Err(PortError::InvalidInput {
                code: "invalid_mcp_server",
            });
        }
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
            let _gate = runtime.publication_gate.write().await;
            runtime.workspace_admin(true).await?;
            let previous = runtime
                .store
                .mcp_server(runtime.config.workspace_id, id)
                .await?;
            let next = McpServer {
                enabled: false,
                deleted: true,
                revision: input.expected_revision + 1,
                ..previous
            };
            let route_path = next.path(runtime.config.profile_id);
            runtime
                .store
                .commit_mcp_server(
                    runtime.config.workspace_id,
                    id,
                    runtime.console_actor(),
                    input.operation_id,
                    input.expected_revision,
                    0,
                    json!(input),
                    next,
                    route_path,
                    None,
                )
                .await
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)??;
        self.server_view(id).await
    }
}

fn validate_public_url(value: &str) -> Result<(), PortError> {
    let invalid = || PortError::InvalidInput {
        code: "invalid_mcp_url",
    };
    if value.len() > 2048 || value.chars().any(char::is_whitespace) || value.contains('\\') {
        return Err(invalid());
    }
    let url = url::Url::parse(value).map_err(|_| invalid())?;
    let loopback = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h == "[::1]"
            || h.parse::<std::net::IpAddr>().is_ok_and(|a| a.is_loopback())
    });
    let path = url.path();
    let valid_path = path == "/mcp"
        || path.strip_prefix("/mcp/").is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
    if url.host_str().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !valid_path
        || url.as_str() != value
    {
        return Err(invalid());
    }
    Ok(())
}
