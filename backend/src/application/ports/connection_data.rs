//! Connection-scoped reads. These ports do not grant access; callers must authorize
//! the current workspace, connection and resource/conversation before using them.
use super::PortFuture;
use crate::domain::{
    connector::ConnectionScope,
    recent_messages::{ConversationKey, RecentPage},
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct StateQuery {
    pub scope: ConnectionScope,
    pub resource: Option<String>,
    pub group: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    Stale,
    Unknown,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateOrigin {
    Live,
    Retained,
}
#[derive(Clone, Serialize)]
pub struct StateField {
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_at_ms: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<StateOrigin>,
    pub freshness: Freshness,
}
#[derive(Clone, Serialize)]
pub struct StateGroup {
    pub name: String,
    pub fields: BTreeMap<String, StateField>,
}
#[derive(Clone, Serialize)]
pub struct StateResource {
    pub name: String,
    pub groups: BTreeMap<String, StateGroup>,
}
pub type StateSnapshot = BTreeMap<String, StateResource>;

pub trait ConnectionStateReader: Send + Sync {
    fn read_connection_state(&self, query: StateQuery) -> PortFuture<'_, StateSnapshot>;
}

/// Conversation identity is created by trusted adapter configuration, not from
/// arbitrary message payload; the caller must authorize the exact conversation.
pub trait ConnectionMessageReader: Send + Sync {
    fn read_connection_messages(
        &self,
        conversation: ConversationKey,
        before: Option<u64>,
        limit: usize,
    ) -> PortFuture<'_, RecentPage>;
}
