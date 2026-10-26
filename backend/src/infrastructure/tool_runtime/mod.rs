//! Single-process tool composition. Trusted file inventory is extended by durable
//! admin-managed connections; exposure remains a separate explicit publication.
pub mod config;
pub mod connections;
mod control;
pub mod endpoint_auth;
mod http;
pub mod oauth;
pub(crate) mod schema;
pub mod security;
mod servers;
mod workspace;

use crate::{
    application::{
        connectors::{ConnectorRegistration, ConnectorRegistry},
        ports::{
            PortError, PortFuture,
            identity::{AccessContext, PolicyEngine},
            profiles::SchemaValidator,
            workspaces::WorkspaceRepository,
        },
        services::{governance::Governance, tools::ToolService},
    },
    domain::{
        connector::{ConnectionScope, ConnectorDescriptor, ConnectorKind},
        identity::{Action, Identity, Resource},
    },
    infrastructure::storage::SqliteStore,
};
use config::ToolsConfig;
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

pub struct ToolRuntime {
    endpoint_auth_cache: std::sync::Mutex<
        std::collections::HashMap<uuid::Uuid, (u64, Arc<endpoint_auth::PreparedAuth>)>,
    >,
    pub agent: Option<crate::application::services::agent::AgentJobService>,
    pub service: ToolService,
    pub content: crate::application::services::mcp_content::McpContentService,
    inventory: Arc<RwLock<ToolsConfig>>,
    registry: Arc<ConnectorRegistry>,
    schemas: Arc<schema::Schemas>,
    read_only_tools: Arc<RwLock<std::collections::HashSet<(ConnectionScope, String)>>>,
    http_client: reqwest::Client,
    execution_slots: Arc<Semaphore>,
    pub config: ToolsConfig,
    token: String,
    pub security: security::SecurityConfig,
    pub oauth: Option<oauth::OAuthAuthenticator>,
    store: Arc<SqliteStore>,
    publication_gate: Arc<tokio::sync::RwLock<()>>,
    publish_slots: Arc<Semaphore>,
    pub requests: Arc<Semaphore>,
    pub control_requests: Arc<Semaphore>,
    // Hold exclusive OS lease for file and online managed registry state.
    _lease: std::fs::File,
}
struct ToolPolicy {
    store: Arc<SqliteStore>,
    config: Arc<RwLock<ToolsConfig>>,
    oauth: Option<security::OAuthConfig>,
}
impl PolicyEngine for ToolPolicy {
    fn authorize(
        &self,
        identity: Identity,
        action: Action,
        resource: Resource,
    ) -> PortFuture<'_, ()> {
        Box::pin(async move {
            let endpoint = if let Some((id, revision)) = identity.mcp_endpoint {
                let server = self.store.mcp_server(identity.workspace_id, id).await?;
                if !server.enabled || server.deleted || server.revision != revision {
                    return Err(PortError::PermissionDenied);
                }
                let profile = crate::application::ports::profiles::ProfileRepository::published(
                    self.store.as_ref(),
                    identity.workspace_id,
                    id,
                )
                .await?;
                let within = match &resource {
                    Resource::Profile { profile_id, .. }
                    | Resource::AgentQueue { profile_id, .. }
                    | Resource::McpResource { profile_id, .. }
                    | Resource::McpPrompt { profile_id, .. } => *profile_id == id,
                    Resource::Tool { scope, tool_id } => profile.tools.iter().any(|t| {
                        t.scope.connection_id == scope.connection_id && t.definition.id == *tool_id
                    }),
                    _ => false,
                };
                if !within {
                    return Err(PortError::PermissionDenied);
                }
                Some(server)
            } else {
                None
            };
            let independent = endpoint.as_ref().is_some_and(|s| {
                !matches!(s.auth, crate::domain::mcp_server::ServerAuth::Inherited)
            });
            let active_profile = if let Resource::Profile {
                workspace_id,
                profile_id,
            } = &resource
            {
                self.store
                    .mcp_server(*workspace_id, *profile_id)
                    .await
                    .ok()
                    .is_some_and(|s| s.enabled && !s.deleted)
            } else {
                false
            };
            let content_server = match &resource {
                Resource::McpResource {
                    workspace_id,
                    profile_id,
                    ..
                }
                | Resource::McpPrompt {
                    workspace_id,
                    profile_id,
                    ..
                } => {
                    let server = self.store.mcp_server(*workspace_id, *profile_id).await?;
                    if !server.enabled || server.deleted {
                        return Err(PortError::PermissionDenied);
                    }
                    Some(server)
                }
                _ => None,
            };
            let c = self.config.read().map_err(|_| PortError::Unavailable)?;
            let content_scope = content_server
                .as_ref()
                .map(|s| s.effective_content_scope(c.profile_id, &c.mcp_content));
            let grant = self.oauth.as_ref().filter(|_| !independent).and_then(|o| {
                o.grants.iter().find(|g| {
                    g.principal_id == identity.principal_id
                        && Some(g.client_id) == identity.client_id
                })
            });
            let valid_subject = if independent {
                endpoint.as_ref().is_some_and(|s| {
                    identity.principal_id == c.principal_id
                        && identity.client_id == Some(s.id)
                        && match &s.auth {
                            crate::domain::mcp_server::ServerAuth::OAuth {
                                required_scope, ..
                            } => identity.scopes.contains(required_scope),
                            _ => true,
                        }
                })
            } else {
                match &self.oauth {
                    Some(o) => grant.is_some() && identity.scopes.contains(&o.required_scope),
                    None => identity.principal_id == c.principal_id && identity.client_id.is_none(),
                }
            };
            if identity.workspace_id != c.workspace_id || !valid_subject {
                return Err(PortError::PermissionDenied);
            }
            let allowed = match (action, resource) {
                (
                    Action::ReadAgentJobs | Action::ManageAgentJobs,
                    Resource::AgentQueue {
                        workspace_id,
                        profile_id,
                    },
                ) => {
                    c.agent_jobs
                        && workspace_id == c.workspace_id
                        && profile_id == c.profile_id
                        && grant.is_none_or(|g| g.agent_jobs)
                }
                (
                    Action::UseProfile,
                    Resource::Profile {
                        workspace_id,
                        profile_id,
                    },
                ) => workspace_id == c.workspace_id && active_profile && !profile_id.is_nil(),
                (Action::InvokeTool, Resource::Tool { scope, tool_id }) => {
                    scope.workspace_id == c.workspace_id
                        && c.tools.iter().any(|t| {
                            grant.is_none_or(|g| g.tools.contains(&t.public_name))
                                && t.connection_id == scope.connection_id
                                && t.definition.id == tool_id
                        })
                }
                (
                    Action::ReadMcpResource,
                    Resource::McpResource {
                        workspace_id, id, ..
                    },
                ) => {
                    workspace_id == c.workspace_id
                        && grant.is_none_or(|g| g.resources.contains(&id))
                        && content_scope
                            .as_ref()
                            .is_some_and(|s| s.resources.contains(&id))
                        && (c.mcp_content.resources.iter().any(|r| r.id == id)
                            || c.mcp_content.templates.iter().any(|r| r.id == id))
                }
                (
                    Action::GetMcpPrompt,
                    Resource::McpPrompt {
                        workspace_id, id, ..
                    },
                ) => {
                    workspace_id == c.workspace_id
                        && grant.is_none_or(|g| g.prompts.contains(&id))
                        && content_scope
                            .as_ref()
                            .is_some_and(|s| s.prompts.contains(&id))
                        && c.mcp_content.prompts.iter().any(|p| p.id == id)
                }
                _ => false,
            };
            if allowed {
                Ok(())
            } else {
                Err(PortError::PermissionDenied)
            }
        })
    }
}
impl ToolRuntime {
    pub async fn build(config: ToolsConfig) -> Result<Arc<Self>, PortError> {
        Self::build_secured(config, security::SecurityConfig::default()).await
    }
    pub async fn build_secured(
        config: ToolsConfig,
        security: security::SecurityConfig,
    ) -> Result<Arc<Self>, PortError> {
        let token = if security.oauth.is_some() {
            String::new()
        } else {
            config::secret(&config.token_env)?
        };
        let introspection = security
            .oauth
            .as_ref()
            .map(|o| config::secret_value(&o.introspection_secret_env, 1))
            .transpose()?;
        let secrets = config
            .tools
            .iter()
            .map(|t| t.bearer_env.as_deref().map(config::secret).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        Self::build_configured(config, token, secrets, security, introspection).await
    }
    pub async fn build_with_secrets(
        config: ToolsConfig,
        token: String,
        secrets: Vec<Option<String>>,
    ) -> Result<Arc<Self>, PortError> {
        Self::build_configured(
            config,
            token,
            secrets,
            security::SecurityConfig::default(),
            None,
        )
        .await
    }
    /// Explicit injection for local embedders and integration tests; secrets never enter snapshots.
    pub async fn build_configured(
        mut config: ToolsConfig,
        token: String,
        mut secrets: Vec<Option<String>>,
        security: security::SecurityConfig,
        introspection_secret: Option<String>,
    ) -> Result<Arc<Self>, PortError> {
        if security.oauth.is_some() != introspection_secret.is_some() {
            return Err(PortError::InvalidInput {
                code: "missing_security_secret",
            });
        }
        config.validate()?;
        if (security.oauth.is_none()
            && (token.len() < 32
                || token.len() > 4096
                || !token.bytes().all(|b| (33..=126).contains(&b))))
            || secrets.len() != config.tools.len()
        {
            return Err(PortError::InvalidInput {
                code: "invalid_tool_secret",
            });
        }
        // Canonical parent + fixed extension avoids duplicate writers through relative paths.
        let parent = config
            .database
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let filename = config.database.file_name().ok_or(PortError::InvalidInput {
            code: "invalid_database_path",
        })?;
        let path = parent
            .canonicalize()
            .map_err(|_| PortError::Unavailable)?
            .join(filename);
        if path.is_symlink() {
            return Err(PortError::InvalidInput {
                code: "symlink_database",
            });
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lease = options
            .open(path.with_extension("tools.lock"))
            .map_err(|_| PortError::Unavailable)?;
        lease.try_lock().map_err(|_| PortError::Conflict)?;
        let store = Arc::new(SqliteStore::open(&path).await?);
        for record in store
            .managed_tools(config.workspace_id, config.profile_id)
            .await?
        {
            let tool: config::HttpToolConfig =
                serde_json::from_value(record).map_err(|_| PortError::Unavailable)?;
            if tool.bearer_env.is_some() || tool.exposed {
                return Err(PortError::Unavailable);
            }
            config.tools.push(tool);
            secrets.push(None);
        }
        // Retain declared names solely for validating old, deny-only OAuth grants.
        let mut security_inventory = config.clone();
        for (id, record) in store
            .connection_overrides(
                config.workspace_id,
                config.profile_id,
                config.tools.iter().map(|t| t.connection_id).collect(),
            )
            .await?
        {
            let index = config.tools.iter().position(|t| t.connection_id == id);
            match (index, record) {
                (Some(index), Some(record)) => {
                    let tool: config::HttpToolConfig =
                        serde_json::from_value(record).map_err(|_| PortError::Unavailable)?;
                    if tool.connection_id != id || tool.bearer_env != config.tools[index].bearer_env
                    {
                        return Err(PortError::Conflict);
                    }
                    config.tools[index] = tool;
                }
                (Some(index), None) => {
                    config.tools.remove(index);
                    secrets.remove(index);
                }
                (None, None) => {}
                (None, Some(_)) => return Err(PortError::Conflict),
            }
        }
        config.validate()?;
        for tool in &config.tools {
            if !security_inventory
                .tools
                .iter()
                .any(|t| t.public_name == tool.public_name)
            {
                security_inventory.tools.push(tool.clone());
            }
        }
        if let Some(oauth) = &security.oauth {
            let missing = oauth
                .grants
                .iter()
                .flat_map(|g| g.tools.iter())
                .filter(|name| {
                    !security_inventory
                        .tools
                        .iter()
                        .any(|t| &t.public_name == *name)
                })
                .cloned()
                .collect::<std::collections::HashSet<_>>();
            if !missing.is_empty() {
                for record in store
                    .retired_grant_tools(
                        config.workspace_id,
                        config.profile_id,
                        missing.into_iter().collect(),
                    )
                    .await?
                {
                    security_inventory
                        .tools
                        .push(serde_json::from_value(record).map_err(|_| PortError::Unavailable)?);
                }
            }
        }
        security.validate(&security_inventory)?;
        let oauth = security
            .oauth
            .clone()
            .zip(introspection_secret)
            .map(|(o, s)| oauth::OAuthAuthenticator::new(o, config.workspace_id, s))
            .transpose()?;
        let schemas = Arc::new(schema::Schemas::compile(config.tools.iter().flat_map(
            |t| {
                std::iter::once(t.definition.input_schema.clone())
                    .chain(t.definition.output_schema.clone())
            },
        ))?);
        for tool in &config.tools {
            if let Some(documentation) = &tool.definition.documentation {
                for example in &documentation.examples {
                    schemas
                        .validate(&tool.definition.input_schema, &example.arguments)
                        .await?;
                }
            }
        }
        let capabilities = Arc::new(
            crate::infrastructure::capabilities::StaticCapabilityCatalog::build(
                config
                    .tools
                    .iter()
                    .map(|t| t.capability(config.workspace_id, config.revision))
                    .collect(),
                vec![],
            )?,
        );
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| PortError::Unavailable)?;
        let slots = Arc::new(Semaphore::new(8));
        let mut registry = ConnectorRegistry::default();
        for (t, credential) in config.tools.iter().zip(secrets) {
            if t.bearer_env.is_some() != credential.is_some() {
                return Err(PortError::InvalidInput {
                    code: "missing_tool_secret",
                });
            }
            if let Some(v) = &credential
                && reqwest::header::HeaderValue::from_str(&format!("Bearer {v}")).is_err()
            {
                return Err(PortError::InvalidInput {
                    code: "invalid_tool_secret",
                });
            }
            let scope = ConnectionScope {
                workspace_id: config.workspace_id,
                connection_id: t.connection_id,
            };
            let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
                scope,
                kind: ConnectorKind::Http,
                display_name: t.public_name.clone(),
            });
            registration.tools = Some(Arc::new(http::HttpExecutor {
                catalog: capabilities.clone(),
                reference: t
                    .capability(config.workspace_id, config.revision)
                    .descriptor
                    .reference,
                scope,
                credential,
                client: client.clone(),
                slots: slots.clone(),
            }));
            registry.register(registration)?;
        }
        if (security.admin.is_some()
            || store
                .has_online_tool_changes(config.workspace_id, config.profile_id)
                .await?)
            && let Some(record) = store
                .tools_snapshot(config.workspace_id, config.profile_id)
                .await?
        {
            let current = record["profile"]["revision"]
                .as_u64()
                .ok_or(PortError::Unavailable)?;
            if config.revision <= current {
                control::adopt_snapshot(&mut config, &record)?;
            }
        }
        store
            .publish_tools(config.profile(), config.snapshot(), config.principal_id)
            .await?;
        let owner = security
            .admin
            .as_ref()
            .map(|a| a.principal_id)
            .unwrap_or(config.principal_id);
        let mut principals = std::collections::HashSet::new();
        if config.principal_id != owner {
            principals.insert(config.principal_id);
        }
        if let Some(oauth) = &security.oauth {
            principals.extend(oauth.grants.iter().map(|g| g.principal_id));
        }
        use crate::domain::workspace::*;
        store
            .bootstrap(
                Workspace {
                    id: config.workspace_id,
                    name: "默认空间".into(),
                    state: WorkspaceState::Active,
                    revision: 1,
                },
                owner,
                principals
                    .into_iter()
                    .map(|principal_id| Membership {
                        workspace_id: config.workspace_id,
                        principal_id,
                        role: WorkspaceRole::Operator,
                        state: MemberState::Active,
                    })
                    .collect(),
            )
            .await?;
        store
            .bind_mcp_catalog(
                config.workspace_id,
                config.profile_id,
                owner,
                config.mcp_content.clone(),
            )
            .await?;
        let inventory = Arc::new(RwLock::new(config.clone()));
        let read_only_tools = Arc::new(RwLock::new(
            config
                .tools
                .iter()
                .filter(|t| {
                    matches!(
                        t.definition.effect,
                        crate::domain::tool::ToolEffect::ReadOnly
                    )
                })
                .map(|t| {
                    (
                        ConnectionScope {
                            workspace_id: config.workspace_id,
                            connection_id: t.connection_id,
                        },
                        t.definition.id.clone(),
                    )
                })
                .collect(),
        ));
        store
            .bootstrap_mcp_server(config.workspace_id, config.profile_id)
            .await?;
        let inner = Arc::new(ToolPolicy {
            store: store.clone(),
            config: inventory.clone(),
            oauth: security.oauth.clone(),
        });
        let governance = Arc::new(Governance::new(
            Arc::new(crate::application::services::workspaces::WorkspacePolicy {
                repository: store.clone(),
                inner,
                read_only_tools: read_only_tools.clone(),
            }),
            store.clone(),
        ));
        let registry = Arc::new(registry);
        let content = crate::application::services::mcp_content::McpContentService::new(
            config.workspace_id,
            config.profile_id,
            Arc::new(crate::infrastructure::mcp_content::StaticMcpContent::new(
                config.workspace_id,
                config.mcp_content.clone(),
            )?),
            governance.clone(),
        );
        let agent = if config.agent_jobs {
            let binding = store
                .agent_binding()
                .await?
                .ok_or(PortError::InvalidInput {
                    code: "agent_pipeline_required",
                })?;
            if binding.source.workspace_id != config.workspace_id
                || binding.config.profile_id != config.profile_id
                || binding.executor != config.principal_id
            {
                return Err(PortError::ScopeMismatch);
            }
            Some(crate::application::services::agent::AgentJobService::new(
                binding,
                store.clone(),
                governance.clone(),
            ))
        } else {
            None
        };
        let publication_gate = Arc::new(tokio::sync::RwLock::new(()));
        let service =
            ToolService::new(store.clone(), schemas.clone(), registry.clone(), governance)
                .with_ledger(store.clone())
                .with_publication_gate(publication_gate.clone());
        Ok(Arc::new(Self {
            agent,
            content,
            service,
            inventory,
            registry,
            schemas,
            read_only_tools,
            http_client: client,
            execution_slots: slots,
            config,
            token,
            endpoint_auth_cache: Default::default(),
            security,
            oauth,
            store,
            publication_gate,
            publish_slots: Arc::new(Semaphore::new(1)),
            requests: Arc::new(Semaphore::new(32)),
            control_requests: Arc::new(Semaphore::new(4)),
            _lease: lease,
        }))
    }
    fn authenticate_static(&self, bearer: &str) -> bool {
        // Hash first to make timing independent of secret length.
        let a = ring::digest::digest(&ring::digest::SHA256, bearer.as_bytes());
        let b = ring::digest::digest(&ring::digest::SHA256, self.token.as_bytes());
        bool::from(a.as_ref().ct_eq(b.as_ref()))
    }
    pub async fn authenticate_identity(&self, bearer: &str) -> Result<Identity, PortError> {
        use crate::application::ports::identity::{Authenticator, Credentials};
        match &self.oauth {
            Some(auth) => {
                auth.authenticate(Credentials {
                    bearer_token: bearer.into(),
                })
                .await
            }
            None if self.authenticate_static(bearer) => Ok(self
                .context(tokio_util::sync::CancellationToken::new())
                .identity),
            None => Err(PortError::PermissionDenied),
        }
    }
    pub fn context(&self, cancellation: tokio_util::sync::CancellationToken) -> AccessContext {
        use crate::application::ports::OperationContext;
        let request_id = uuid::Uuid::new_v4();
        AccessContext {
            identity: Identity {
                mcp_endpoint: None,
                workspace_id: self.config.workspace_id,
                principal_id: self.config.principal_id,
                client_id: None,
                scopes: Default::default(),
                expires_at: None,
            },
            operation: OperationContext {
                scope: ConnectionScope {
                    workspace_id: self.config.workspace_id,
                    connection_id: self.config.profile_id,
                },
                principal_id: self.config.principal_id,
                request_id,
                correlation_id: request_id,
                causation_id: None,
                route_trace: vec![],
                deadline: Some(std::time::Instant::now() + Duration::from_secs(25)),
                cancellation,
            },
        }
    }
}
