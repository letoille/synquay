use super::ToolRuntime;
use crate::{
    application::ports::{PortError, workspaces::WorkspaceRepository},
    domain::workspace::*,
};
use uuid::Uuid;

impl ToolRuntime {
    /// Temporary console actor for audit and recovery scope, not an authenticated user.
    pub(super) fn console_actor(&self) -> Uuid {
        self.security
            .admin
            .as_ref()
            .map(|a| a.principal_id)
            .unwrap_or(self.config.principal_id)
    }

    pub(crate) async fn workspace_admin(
        &self,
        require_active: bool,
    ) -> Result<WorkspaceAccess, PortError> {
        let actor = self.console_actor();
        let access = self.store.access(self.config.workspace_id, actor).await?;
        if !access.manages() || (require_active && !access.active()) {
            return Err(PortError::PermissionDenied);
        }
        Ok(access)
    }
    /// Bootstrap-admin API. Space and actor are always bound by trusted runtime configuration.
    pub async fn workspace_view(&self) -> Result<Workspace, PortError> {
        Ok(self.workspace_admin(false).await?.workspace)
    }
    pub async fn workspace_members(
        &self,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<Membership>, PortError> {
        let access = self.workspace_admin(false).await?;
        self.store
            .members(
                access.workspace.id,
                access.member.principal_id,
                after,
                limit,
            )
            .await
    }
    pub async fn update_workspace(
        &self,
        request_id: Uuid,
        update: WorkspaceUpdate,
    ) -> Result<Workspace, PortError> {
        let access = self.workspace_admin(false).await?;
        if let WorkspaceChange::SetMember {
            principal_id, role, ..
        } = &update.change
        {
            // Until multi-admin authentication exists, retain the only recovery identity.
            if *principal_id == access.member.principal_id || *role == WorkspaceRole::Owner {
                return Err(PortError::PermissionDenied);
            }
            let known = *principal_id == self.config.principal_id
                || self
                    .security
                    .oauth
                    .as_ref()
                    .is_some_and(|o| o.grants.iter().any(|g| g.principal_id == *principal_id));
            if !known {
                return Err(PortError::InvalidInput {
                    code: "unknown_principal",
                });
            }
        }
        self.store
            .update(
                self.config.workspace_id,
                access.member.principal_id,
                request_id,
                update,
            )
            .await
    }
}
