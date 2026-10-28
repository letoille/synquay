//! Recent received messages. These values are a bounded cache, never a complete history.
use super::connector::ConnectionScope;
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConversationKey {
    pub scope: ConnectionScope,
    /// Adapter-verified upstream account; prevents history crossing account replacement.
    pub account: String,
    pub kind: String,
    pub id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecentMessage {
    pub sequence: u64,
    pub event_id: Uuid,
    pub source_message_id: String,
    pub sender: String,
    pub received_at_ms: u64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
}

#[derive(Serialize)]
pub struct RecentPage {
    /// Identifies this in-memory cache lifetime; changes after restart.
    pub generation: Uuid,
    pub messages: Vec<RecentMessage>,
    pub next_before: Option<u64>,
    /// A local cache can never establish completeness of the upstream conversation.
    pub history_complete: bool,
}
