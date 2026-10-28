//! Explicit, default-deny visibility for Synquay built-in tools.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinTool {
    ConnectionsList,
    ConnectionStatus,
    ConnectionStatistics,
    RecentMessages,
    QqBotInfo,
    QqSendText,
}

impl BuiltinTool {
    pub const ALL: [Self; 6] = [
        Self::ConnectionsList,
        Self::ConnectionStatus,
        Self::ConnectionStatistics,
        Self::RecentMessages,
        Self::QqBotInfo,
        Self::QqSendText,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::ConnectionsList => "synquay_builtin_connections_list",
            Self::ConnectionStatus => "synquay_builtin_connection_status",
            Self::ConnectionStatistics => "synquay_builtin_connection_statistics",
            Self::RecentMessages => "synquay_builtin_recent_messages",
            Self::QqBotInfo => "synquay_builtin_qq_bot_info",
            Self::QqSendText => "synquay_builtin_qq_send_text",
        }
    }
}

/// All identifiers originate from operator-approved connection and conversation data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationGrant {
    pub connection_id: Uuid,
    pub account: String,
    pub kind: String,
    pub conversation_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinScope {
    #[serde(default)]
    pub tools: Vec<BuiltinTool>,
    #[serde(default)]
    pub connections: Vec<Uuid>,
    #[serde(default)]
    pub conversations: Vec<ConversationGrant>,
    #[serde(default)]
    pub qq_send_conversations: Vec<ConversationGrant>,
}
impl BuiltinScope {
    /// Validation checks bounded structure; existence and current access need an
    /// authoritative connection snapshot at publication and again at execution.
    pub fn is_valid(&self) -> bool {
        let ids_valid = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        };
        self.tools.len() <= 6
            && self.tools.iter().collect::<HashSet<_>>().len() == self.tools.len()
            && self.connections.len() <= 32
            && self.connections.iter().all(|id| !id.is_nil())
            && self.connections.iter().collect::<HashSet<_>>().len() == self.connections.len()
            && self.conversations.len() <= 100
            && self.conversations.iter().collect::<HashSet<_>>().len() == self.conversations.len()
            && self.qq_send_conversations.len() <= 100
            && self
                .qq_send_conversations
                .iter()
                .collect::<HashSet<_>>()
                .len()
                == self.qq_send_conversations.len()
            && self.qq_send_conversations.iter().all(|c| {
                self.connections.contains(&c.connection_id)
                    && matches!(c.kind.as_str(), "group" | "direct")
                    && ids_valid(&c.account)
                    && ids_valid(&c.conversation_id)
            })
            && self.conversations.iter().all(|conversation| {
                self.connections.contains(&conversation.connection_id)
                    && ((matches!(conversation.kind.as_str(), "group" | "direct")
                        && ids_valid(&conversation.account)
                        && ids_valid(&conversation.conversation_id))
                        || (conversation.kind == "stream"
                            && conversation.account == "connection"
                            && conversation.conversation_id == "default"))
            })
    }
    pub fn permits_connection(&self, tool: BuiltinTool, connection: Uuid) -> bool {
        self.is_valid() && self.tools.contains(&tool) && self.connections.contains(&connection)
    }
    pub fn permits_conversation(&self, conversation: &ConversationGrant) -> bool {
        self.permits_connection(BuiltinTool::RecentMessages, conversation.connection_id)
            && self.conversations.contains(conversation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_is_default_deny_and_conversations_are_exact() {
        let connection = Uuid::new_v4();
        let group = ConversationGrant {
            connection_id: connection,
            account: "app1".into(),
            kind: "group".into(),
            conversation_id: "group1".into(),
        };
        assert!(!BuiltinScope::default().permits_conversation(&group));
        let scope = BuiltinScope {
            tools: vec![BuiltinTool::RecentMessages],
            connections: vec![connection],
            conversations: vec![group.clone()],
            qq_send_conversations: vec![],
        };
        assert!(scope.is_valid());
        assert!(scope.permits_conversation(&group));
        assert!(scope.qq_send_conversations.is_empty());
        assert!(!scope.permits_connection(BuiltinTool::QqSendText, connection));
        let mut sending = scope.clone();
        sending.tools.push(BuiltinTool::QqSendText);
        sending.qq_send_conversations.push(group.clone());
        assert!(sending.is_valid());
        let mut stream = group.clone();
        stream.kind = "stream".into();
        stream.account = "connection".into();
        stream.conversation_id = "default".into();
        sending.qq_send_conversations = vec![stream];
        assert!(!sending.is_valid());
        assert!(!scope.permits_connection(BuiltinTool::ConnectionStatus, connection));
        let mut other = group.clone();
        other.account = "app2".into();
        assert!(!scope.permits_conversation(&other));
        other = group.clone();
        other.kind = "direct".into();
        assert!(!scope.permits_conversation(&other));
        other = group.clone();
        other.conversation_id = "group2".into();
        assert!(!scope.permits_conversation(&other));
        let invalid = BuiltinScope {
            connections: vec![],
            ..scope
        };
        assert!(!invalid.is_valid());
        assert!(!invalid.permits_conversation(&group));
    }
}
