//! Space is the product name; Workspace is the sole tenant/isolation identity.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Active,
    Suspended,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberState {
    Active,
    Revoked,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRole {
    Owner,
    Administrator,
    Operator,
    Viewer,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub state: WorkspaceState,
    /// Incremented for every workspace or member change, including revocation.
    pub revision: u64,
}
impl Workspace {
    pub fn is_valid(&self) -> bool {
        !self.id.is_nil()
            && !self.name.trim().is_empty()
            && self.name.len() <= 128
            && !self.name.chars().any(char::is_control)
            && self.revision > 0
            && self.revision < i64::MAX as u64
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    pub workspace_id: Uuid,
    pub principal_id: Uuid,
    pub role: WorkspaceRole,
    pub state: MemberState,
}
impl Membership {
    pub fn is_valid(&self) -> bool {
        !self.workspace_id.is_nil() && !self.principal_id.is_nil()
    }
}
#[derive(Clone)]
pub struct WorkspaceAccess {
    pub workspace: Workspace,
    pub member: Membership,
}
impl WorkspaceAccess {
    pub fn active(&self) -> bool {
        self.workspace.id == self.member.workspace_id
            && self.workspace.state == WorkspaceState::Active
            && self.member.state == MemberState::Active
    }
    /// Recovery administration remains possible while the workspace is suspended.
    pub fn manages(&self) -> bool {
        self.workspace.id == self.member.workspace_id
            && self.member.state == MemberState::Active
            && matches!(
                self.member.role,
                WorkspaceRole::Owner | WorkspaceRole::Administrator
            )
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceChange {
    Rename {
        name: String,
    },
    SetState {
        state: WorkspaceState,
    },
    SetMember {
        principal_id: Uuid,
        role: WorkspaceRole,
        state: MemberState,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceUpdate {
    pub expected_revision: u64,
    pub change: WorkspaceChange,
}
