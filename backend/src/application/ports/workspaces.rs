use super::PortFuture;
use crate::domain::workspace::*;
use uuid::Uuid;

/// Internal persistence port. Bootstrapping is operator-only; reads do not grant authority.
pub trait WorkspaceRepository: Send + Sync {
    /// Creates only on first use. Existing records, including revoked members, are untouched.
    fn bootstrap(
        &self,
        workspace: Workspace,
        owner: Uuid,
        members: Vec<Membership>,
    ) -> PortFuture<'_, ()>;
    /// Read space and membership in the same database snapshot; missing membership is denied.
    fn access(&self, workspace: Uuid, principal: Uuid) -> PortFuture<'_, WorkspaceAccess>;
    /// Owner-authorized mutation, CAS revision and audit are committed atomically.
    fn update(
        &self,
        workspace: Uuid,
        actor: Uuid,
        request_id: Uuid,
        update: WorkspaceUpdate,
    ) -> PortFuture<'_, Workspace>;
    /// Keyset pagination, bounded to 100 records. Returns one page; resume after last principal.
    fn members(
        &self,
        workspace: Uuid,
        actor: Uuid,
        after: Option<Uuid>,
        limit: u32,
    ) -> PortFuture<'_, Vec<Membership>>;
}
