use super::connector::ConnectionScope;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::SystemTime};
use uuid::Uuid;

/// Produced by an authenticator after validating credentials, issuer, audience and expiry.
#[derive(Clone)]
pub struct Identity {
    /// Bound by the HTTP authenticator, never taken from caller claims.
    pub mcp_endpoint: Option<(Uuid, u64)>,
    pub workspace_id: Uuid,
    pub principal_id: Uuid,
    pub client_id: Option<Uuid>,
    pub scopes: BTreeSet<String>,
    pub expires_at: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    ManageWorkspace,
    ReadAgentJobs,
    ManageAgentJobs,
    UseProfile,
    InvokeTool,
    ReadMcpResource,
    GetMcpPrompt,
    RouteEvent,
    DeliverMessage,
    Reply,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    AgentQueue {
        workspace_id: Uuid,
        profile_id: Uuid,
    },
    Workspace {
        workspace_id: Uuid,
    },
    Profile {
        workspace_id: Uuid,
        profile_id: Uuid,
    },
    McpResource {
        workspace_id: Uuid,
        profile_id: Uuid,
        id: String,
    },
    McpPrompt {
        workspace_id: Uuid,
        profile_id: Uuid,
        id: String,
    },
    Tool {
        scope: ConnectionScope,
        tool_id: String,
    },
    Event {
        scope: ConnectionScope,
        event_id: Uuid,
    },
    Destination {
        scope: ConnectionScope,
        destination_id: Uuid,
    },
    ReplyRoute {
        workspace_id: Uuid,
        route_id: Uuid,
    },
}
impl Resource {
    pub fn workspace_id(&self) -> Uuid {
        match self {
            Self::AgentQueue { workspace_id, .. }
            | Self::Workspace { workspace_id }
            | Self::Profile { workspace_id, .. }
            | Self::McpResource { workspace_id, .. }
            | Self::McpPrompt { workspace_id, .. }
            | Self::ReplyRoute { workspace_id, .. } => *workspace_id,
            Self::Tool { scope, .. }
            | Self::Event { scope, .. }
            | Self::Destination { scope, .. } => scope.workspace_id,
        }
    }
}
