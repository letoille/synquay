//! Adapter checks for connection message reads, separate from caller authorization.
use super::Pipeline;
use crate::{
    application::ports::{
        PortError, PortFuture, connection_data::ConnectionMessageReader,
        recent_messages::RecentMessages,
    },
    domain::recent_messages::{ConversationKey, RecentPage},
    infrastructure::connectors::message_config::ConnectionConfig,
};

impl Pipeline {
    // Caller holds connection_io.read through the query and any permission recheck.
    pub(crate) async fn read_connection_messages_locked(
        &self,
        key: ConversationKey,
        before: Option<u64>,
        limit: usize,
    ) -> Result<RecentPage, PortError> {
        if key.scope.workspace_id != self.source.workspace_id || key.scope.connection_id.is_nil() {
            return Err(PortError::PermissionDenied);
        }
        let connection = self
            .connection_snapshot()?
            .connections
            .into_iter()
            .find(|c| c.id == key.scope.connection_id && c.enabled)
            .ok_or(PortError::PermissionDenied)?;
        match connection.config {
            ConnectionConfig::QqBot(qq)
                if qq.app_id == key.account
                    && matches!(key.kind.as_str(), "group" | "direct")
                    && !key.id.is_empty()
                    && key.id.len() <= 128
                    && (qq.group_openid.is_empty()
                        || (key.kind == "group" && key.id == qq.group_openid)) =>
            {
                self.store.read_recent(key, before, limit).await
            }
            ConnectionConfig::Mqtt(_) | ConnectionConfig::Webhook { .. }
                if connection.recent_messages.enabled
                    && key.account == "connection"
                    && key.kind == "stream"
                    && key.id == "default" =>
            {
                self.store.read_recent(key, before, limit).await
            }
            ConnectionConfig::QqBot(_) => Err(PortError::PermissionDenied),
            _ => Err(PortError::Unsupported),
        }
    }
}
impl ConnectionMessageReader for Pipeline {
    fn read_connection_messages(
        &self,
        key: ConversationKey,
        before: Option<u64>,
        limit: usize,
    ) -> PortFuture<'_, RecentPage> {
        Box::pin(async move {
            let _guard = self.connection_io.read().await;
            self.read_connection_messages_locked(key, before, limit)
                .await
        })
    }
}
