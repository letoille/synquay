//! Internal bounded query implementation; the caller must authorize the current MCP service.
use super::Pipeline;
use crate::{
    application::ports::PortError,
    domain::{
        builtin_tools::{BuiltinScope, BuiltinTool, ConversationGrant},
        connector::ConnectionScope,
        recent_messages::ConversationKey,
    },
    infrastructure::connectors::message_config::ConnectionConfig,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecentCursor {
    workspace: Uuid,
    connection: Uuid,
    account: String,
    kind: String,
    conversation: String,
    generation: Uuid,
    before: u64,
}
use uuid::Uuid;

impl Pipeline {
    /// A scope is not a credential. Do not expose this method without checking the
    /// current endpoint, caller identity, workspace, publication and audit policy.
    pub async fn builtin_connections(&self, scope: &BuiltinScope) -> Result<Value, PortError> {
        let _guard = self.connection_io.read().await;
        self.builtin_connections_locked(scope).await
    }
    pub(crate) async fn builtin_connections_locked(
        &self,
        scope: &BuiltinScope,
    ) -> Result<Value, PortError> {
        if !scope.is_valid() || !scope.tools.contains(&BuiltinTool::ConnectionsList) {
            return Err(PortError::PermissionDenied);
        }
        let snapshot = self.connection_snapshot()?;
        let connections: Vec<_> = snapshot
            .connections
            .into_iter()
            .filter(|c| scope.permits_connection(BuiltinTool::ConnectionsList, c.id))
            .map(|c| {
                let kind = match c.config {
                    ConnectionConfig::QqBot(_) => "qq_bot",
                    ConnectionConfig::Mqtt(_) => "mqtt",
                    ConnectionConfig::HttpClient(_) => "http_client",
                    ConnectionConfig::Webhook { .. } => "webhook",
                };
                json!({"id":c.id,"name":c.name,"kind":kind,"enabled":c.enabled})
            })
            .collect();
        Ok(json!({"connections":connections}))
    }
    pub async fn builtin_observation(
        &self,
        scope: &BuiltinScope,
        tool: BuiltinTool,
        id: Uuid,
    ) -> Result<Value, PortError> {
        let _guard = self.connection_io.read().await;
        self.builtin_observation_locked(scope, tool, id).await
    }
    pub(crate) async fn builtin_observation_locked(
        &self,
        scope: &BuiltinScope,
        tool: BuiltinTool,
        id: Uuid,
    ) -> Result<Value, PortError> {
        if !matches!(
            tool,
            BuiltinTool::ConnectionStatus | BuiltinTool::ConnectionStatistics
        ) || !scope.permits_connection(tool, id)
        {
            return Err(PortError::PermissionDenied);
        }
        let mut snapshot = self.connection_snapshot()?;
        snapshot
            .connections
            .retain(|connection| connection.id == id);
        if snapshot.connections.is_empty() {
            return Err(PortError::PermissionDenied);
        }
        let mut observations = self.connection_observations(&snapshot).await?;
        let observed = observations
            .as_object_mut()
            .and_then(|map| map.remove(&id.to_string()))
            .ok_or(PortError::Unavailable)?;
        Ok(match tool {
            BuiltinTool::ConnectionStatus => {
                json!({"connection_id":id,"status":observed["status"],"observed_at_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|d|u64::try_from(d.as_millis()).ok())})
            }
            BuiltinTool::ConnectionStatistics => {
                json!({"connection_id":id,"received":observed["received"],"sent":observed["sent"],"semantics":"received: accepted events; sent: upstream accepted, not necessarily delivered or executed"})
            }
            _ => return Err(PortError::PermissionDenied),
        })
    }
    pub async fn builtin_recent_messages(
        &self,
        scope: &BuiltinScope,
        conversation: ConversationGrant,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Value, PortError> {
        let _guard = self.connection_io.read().await;
        self.builtin_recent_messages_locked(scope, conversation, cursor, limit)
            .await
    }
    pub(crate) async fn builtin_recent_messages_locked(
        &self,
        scope: &BuiltinScope,
        conversation: ConversationGrant,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Value, PortError> {
        if !scope.permits_conversation(&conversation) {
            return Err(PortError::PermissionDenied);
        }
        let key = ConversationKey {
            scope: ConnectionScope {
                workspace_id: self.source.workspace_id,
                connection_id: conversation.connection_id,
            },
            account: conversation.account,
            kind: conversation.kind,
            id: conversation.conversation_id,
        };
        let cursor = cursor
            .map(|raw| {
                if raw.len() > 2048 {
                    return Err(PortError::InvalidInput {
                        code: "invalid_recent_cursor",
                    });
                }
                let decoded = URL_SAFE_NO_PAD
                    .decode(raw)
                    .map_err(|_| PortError::InvalidInput {
                        code: "invalid_recent_cursor",
                    })?;
                let c: RecentCursor =
                    serde_json::from_slice(&decoded).map_err(|_| PortError::InvalidInput {
                        code: "invalid_recent_cursor",
                    })?;
                if c.workspace != key.scope.workspace_id
                    || c.connection != key.scope.connection_id
                    || c.account != key.account
                    || c.kind != key.kind
                    || c.conversation != key.id
                    || c.before == 0
                {
                    return Err(PortError::InvalidInput {
                        code: "invalid_recent_cursor",
                    });
                }
                Ok(c)
            })
            .transpose()?;
        let page = self
            .read_connection_messages_locked(key.clone(), cursor.as_ref().map(|c| c.before), limit)
            .await?;
        if cursor
            .as_ref()
            .is_some_and(|c| c.generation != page.generation)
        {
            return Err(PortError::InvalidInput {
                code: "expired_recent_cursor",
            });
        }
        // Bound result size independently of the storage budget. Preserve the next
        // cursor at the last returned item when the page is truncated by bytes.
        let mut used = 0usize;
        let mut messages = Vec::new();
        let total = page.messages.len();
        for item in page.messages {
            let size = serde_json::to_vec(&item)
                .map_err(|_| PortError::Unavailable)?
                .len();
            if used + size > 32 * 1024 {
                if messages.is_empty() {
                    return Err(PortError::PayloadTooLarge);
                }
                break;
            }
            used += size;
            messages.push(item);
        }
        let last = messages.last().map(|m| m.sequence);
        let next_before = if messages.len() < total {
            last
        } else {
            page.next_before
        };
        let next_cursor = next_before
            .map(|before| {
                let cursor = RecentCursor {
                    workspace: key.scope.workspace_id,
                    connection: key.scope.connection_id,
                    account: key.account,
                    kind: key.kind,
                    conversation: key.id,
                    generation: page.generation,
                    before,
                };
                serde_json::to_vec(&cursor)
                    .map(|v| URL_SAFE_NO_PAD.encode(v))
                    .map_err(|_| PortError::Unavailable)
            })
            .transpose()?;
        Ok(json!({"messages":messages,"next_cursor":next_cursor,"history_complete":false}))
    }
}
