use super::{connector::ConnectionScope, message::Message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub enum EventPayload {
    Content(super::content::ContentRef),
    Json(Value),
    Message(Message),
    Binary { media_type: String, bytes: Vec<u8> },
}

/// Untrusted content normalized by an adapter. Scope and receive time are server-owned.
#[derive(Clone)]
pub struct IncomingEvent {
    pub route_context: Option<super::connection_routing::RouteBinding>,
    pub event_type: String,
    pub schema_version: u32,
    pub occurred_at: Option<SystemTime>,
    /// Stable upstream identifier if available, scoped to workspace + connection.
    pub source_event_id: Option<String>,
    pub payload: EventPayload,
}

/// Internal model, not the public wire schema or storage representation.
#[derive(Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Set by trusted state mapping during durable ingestion, not by the sender.
    #[serde(default)]
    pub state_report: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_context: Option<super::connection_routing::RouteBinding>,
    pub event_id: Uuid,
    pub scope: ConnectionScope,
    pub event_type: String,
    pub schema_version: u32,
    pub occurred_at: Option<SystemTime>,
    pub received_at: SystemTime,
    pub source_event_id: Option<String>,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub route_trace: Vec<ConnectionScope>,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestDisposition {
    Stored,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestReceipt {
    /// Duplicate receipts MUST identify the original persisted event.
    pub event_id: Uuid,
    pub disposition: IngestDisposition,
}
