//! Workspace membership is an additional ceiling, never a replacement for resource grants.
use crate::{
    application::ports::{
        PortError, PortFuture, identity::PolicyEngine, workspaces::WorkspaceRepository,
    },
    domain::{
        connector::ConnectionScope,
        identity::{Action, Identity, Resource},
        workspace::WorkspaceRole,
    },
};
use std::{
    collections::HashSet,
    sync::{Arc, RwLock},
    time::SystemTime,
};

pub struct WorkspacePolicy {
    pub repository: Arc<dyn WorkspaceRepository>,
    pub inner: Arc<dyn PolicyEngine>,
    /// Trusted classifications only. A read-only hint alone never grants access.
    pub read_only_tools: Arc<RwLock<HashSet<(ConnectionScope, String)>>>,
}
impl PolicyEngine for WorkspacePolicy {
    fn authorize(
        &self,
        identity: Identity,
        action: Action,
        resource: Resource,
    ) -> PortFuture<'_, ()> {
        Box::pin(async move {
            if identity.workspace_id != resource.workspace_id() {
                return Err(PortError::ScopeMismatch);
            }
            if identity.expires_at.is_some_and(|t| t <= SystemTime::now()) {
                return Err(PortError::PermissionDenied);
            }
            let access = self
                .repository
                .access(identity.workspace_id, identity.principal_id)
                .await?;
            if !access.active() {
                return Err(PortError::PermissionDenied);
            }
            if access.member.role == WorkspaceRole::Viewer {
                let read = matches!(
                    (&action, &resource),
                    (Action::UseProfile, Resource::Profile { .. })
                        | (Action::ReadMcpResource, Resource::McpResource { .. })
                        | (Action::ReadMcpBuiltin, Resource::McpBuiltin { .. })
                        | (Action::GetMcpPrompt, Resource::McpPrompt { .. })
                ) || match (&action, &resource) {
                    (Action::InvokeTool, Resource::Tool { scope, tool_id }) => self
                        .read_only_tools
                        .read()
                        .map_err(|_| PortError::Unavailable)?
                        .contains(&(*scope, tool_id.clone())),
                    _ => false,
                };
                if !read {
                    return Err(PortError::PermissionDenied);
                }
            }
            self.inner.authorize(identity, action, resource).await
        })
    }
}
