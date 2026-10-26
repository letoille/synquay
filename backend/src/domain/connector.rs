use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Both identifiers are assigned by the control plane, never taken from a message body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionScope {
    pub workspace_id: Uuid,
    pub connection_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorKind {
    Http,
    Cli,
    Mqtt,
    Mcp,
    Sdk,
    Custom(String),
}

#[derive(Clone)]
pub struct ConnectorDescriptor {
    pub scope: ConnectionScope,
    pub kind: ConnectorKind,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    DecodeInbound,
    ReceiveEvents,
    ExecuteTools,
    DeliverMessages,
    Lifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Ready,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct HealthReport {
    pub status: HealthStatus,
    /// Stable diagnostic code, not upstream response bodies or credentials.
    pub code: Option<&'static str>,
}
