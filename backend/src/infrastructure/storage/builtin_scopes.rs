//! Validate operator-selected scopes against the current trusted connection directory.
use super::*;
use crate::{
    domain::builtin_tools::BuiltinScope,
    infrastructure::{
        connectors::message_config::ConnectionConfig,
        pipeline::message_connections::ConnectionSnapshot,
    },
};

impl SqliteStore {
    pub(crate) async fn validate_builtin_scope(
        &self,
        workspace: Uuid,
        scope: BuiltinScope,
    ) -> Result<(), PortError> {
        if !scope.is_valid() {
            return Err(PortError::InvalidInput {
                code: "invalid_builtin_scope",
            });
        }
        self.run(move |c| {
            let mut q=c.prepare("SELECT record FROM message_connections WHERE workspace=?1").map_err(db_error)?;
            let records=q.query_map([workspace.to_string()],|r|r.get::<_,String>(0)).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
            let mut connections=Vec::new();
            for raw in records { connections.extend(decode::<ConnectionSnapshot>(&raw)?.connections); }
            for id in &scope.connections {
                if connections.iter().filter(|c|c.id==*id && c.enabled).count()!=1 { return Err(PortError::InvalidInput { code:"invalid_builtin_scope" }); }
            }
            for grant in scope.conversations.iter().chain(&scope.qq_send_conversations) {
                let Some(connection)=connections.iter().find(|c|c.id==grant.connection_id) else { return Err(PortError::InvalidInput { code:"invalid_builtin_scope" }); };
                if scope.qq_send_conversations.contains(grant) && !matches!(&connection.config, ConnectionConfig::QqBot(qq) if qq.allow_proactive) { return Err(PortError::InvalidInput { code:"invalid_builtin_scope" }); }
                match &connection.config {
                    ConnectionConfig::QqBot(qq) => {
                        if !matches!(grant.kind.as_str(), "group" | "direct") || qq.app_id!=grant.account || (!qq.group_openid.is_empty() && (grant.kind!="group" || grant.conversation_id!=qq.group_openid)) { return Err(PortError::InvalidInput { code:"invalid_builtin_scope" }); }
                        let present:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM connection_conversations WHERE workspace=?1 AND connection=?2 AND account=?3 AND kind=?4 AND id=?5)",params![workspace.to_string(),grant.connection_id.to_string(),grant.account,grant.kind,grant.conversation_id],|r|r.get(0)).map_err(db_error)?;
                        if !present { return Err(PortError::InvalidInput { code:"invalid_builtin_scope" }); }
                    }
                    ConnectionConfig::Mqtt(_) | ConnectionConfig::Webhook { .. } if connection.receives() && grant.kind=="stream" && grant.account=="connection" && grant.conversation_id=="default" => (),
                    _ => return Err(PortError::InvalidInput { code:"invalid_builtin_scope" }),
                }
            }
            Ok(())
        }).await
    }
}

/// Invalidate grants atomically with a connection change so a removed or replaced
/// connection cannot silently regain access if its identifier is reused.
pub(super) fn prune_connection_grants(
    tx: &rusqlite::Transaction<'_>,
    workspace: Uuid,
    connection_id: Uuid,
    previous: Option<&crate::infrastructure::connectors::message_config::MessageConnection>,
    next: Option<&crate::infrastructure::connectors::message_config::MessageConnection>,
    actor: Uuid,
    operation: Uuid,
) -> Result<(), PortError> {
    use crate::domain::mcp_server::McpServer;
    let Some(previous) = previous else {
        return Ok(());
    };
    let remove_connection = match next {
        None => true,
        Some(next) if !next.enabled => true,
        Some(next) => match (&previous.config, &next.config) {
            (ConnectionConfig::QqBot(a), ConnectionConfig::QqBot(b)) => a.app_id != b.app_id,
            (a, b) => std::mem::discriminant(a) != std::mem::discriminant(b),
        },
    };
    let mut query = tx
        .prepare("SELECT record FROM mcp_servers WHERE workspace=?1 AND deleted=0")
        .map_err(db_error)?;
    let records = query
        .query_map([workspace.to_string()], |r| r.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    drop(query);
    for raw in records {
        let mut server: McpServer = decode(&raw)?;
        let before = (
            server.builtin_scope.connections.len(),
            server.builtin_scope.conversations.len(),
            server.builtin_scope.qq_send_conversations.len(),
        );
        let changed_stream = next.is_some_and(|next| {
            matches!(
                &previous.config,
                ConnectionConfig::Mqtt(_) | ConnectionConfig::Webhook { .. }
            ) && serde_json::to_value(&previous.config).ok()
                != serde_json::to_value(&next.config).ok()
        });
        if changed_stream {
            server
                .builtin_scope
                .conversations
                .retain(|grant| grant.connection_id != connection_id);
        }
        if remove_connection {
            server
                .builtin_scope
                .qq_send_conversations
                .retain(|grant| grant.connection_id != connection_id);
        } else if let Some(ConnectionConfig::QqBot(qq)) = next.map(|c| &c.config) {
            server.builtin_scope.qq_send_conversations.retain(|grant| {
                grant.connection_id != connection_id
                    || (qq.allow_proactive
                        && grant.account == qq.app_id
                        && (qq.group_openid.is_empty()
                            || (grant.kind == "group" && grant.conversation_id == qq.group_openid)))
            });
        }
        if remove_connection {
            server
                .builtin_scope
                .connections
                .retain(|id| *id != connection_id);
            server
                .builtin_scope
                .conversations
                .retain(|grant| grant.connection_id != connection_id);
        } else if let Some(ConnectionConfig::QqBot(qq)) = next.map(|c| &c.config) {
            server.builtin_scope.conversations.retain(|grant| {
                grant.connection_id != connection_id
                    || (grant.account == qq.app_id
                        && (qq.group_openid.is_empty()
                            || (grant.kind == "group" && grant.conversation_id == qq.group_openid)))
            });
        }
        if before
            == (
                server.builtin_scope.connections.len(),
                server.builtin_scope.conversations.len(),
                server.builtin_scope.qq_send_conversations.len(),
            )
        {
            continue;
        }
        server.revision = server
            .revision
            .checked_add(1)
            .ok_or(PortError::Unavailable)?;
        let record = encode(&server)?;
        let now = millis(SystemTime::now())?;
        tx.execute("UPDATE mcp_servers SET record=?3,last_operation=NULL,last_request=NULL WHERE workspace=?1 AND id=?2",params![workspace.to_string(), server.id.to_string(), record]).map_err(db_error)?;
        tx.execute(
            "INSERT INTO mcp_server_changes VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                workspace.to_string(),
                server.id.to_string(),
                server.revision,
                actor.to_string(),
                operation.to_string(),
                record,
                now
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::builtin_tools::{BuiltinScope, BuiltinTool, ConversationGrant};
    use serde_json::json;
    #[tokio::test]
    async fn removing_or_replacing_a_connection_revokes_existing_grants() {
        use crate::domain::mcp_server::McpServer;
        use crate::infrastructure::connectors::message_config::MessageConnection;
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path().join("revocation.db"))
            .await
            .unwrap();
        let workspace = Uuid::new_v4();
        let server_id = Uuid::new_v4();
        let connection = Uuid::new_v4();
        store
            .bootstrap_mcp_server(workspace, server_id)
            .await
            .unwrap();
        let old: MessageConnection = serde_json::from_value(json!({"id":connection,"name":"QQ","enabled":true,"config":{"kind":"qq_bot","app_id":"123456","group_openid":"","allowed_senders":[],"allow_proactive":false}})).unwrap();
        let new: MessageConnection = serde_json::from_value(json!({"id":connection,"name":"QQ","enabled":true,"config":{"kind":"qq_bot","app_id":"987654","group_openid":"","allowed_senders":[],"allow_proactive":false}})).unwrap();
        let actor = Uuid::new_v4();
        let operation = Uuid::new_v4();
        store.run(move |c| {
            let tx = c.transaction().map_err(db_error)?;
            let raw: String = tx.query_row("SELECT record FROM mcp_servers WHERE workspace=?1 AND id=?2",params![workspace.to_string(),server_id.to_string()],|r|r.get(0)).map_err(db_error)?;
            let mut server: McpServer = decode(&raw)?;
            server.builtin_scope = BuiltinScope { tools: vec![BuiltinTool::RecentMessages], connections: vec![connection], conversations: vec![ConversationGrant { connection_id: connection, account: "123456".into(), kind: "group".into(), conversation_id: "group_A".into() }], qq_send_conversations: vec![] };
            tx.execute("UPDATE mcp_servers SET record=?3 WHERE workspace=?1 AND id=?2",params![workspace.to_string(),server_id.to_string(),encode(&server)?]).map_err(db_error)?;
            prune_connection_grants(&tx, workspace, connection, Some(&old), Some(&new), actor, operation)?;
            let raw: String = tx.query_row("SELECT record FROM mcp_servers WHERE workspace=?1 AND id=?2",params![workspace.to_string(),server_id.to_string()],|r|r.get(0)).map_err(db_error)?;
            let updated: McpServer = decode(&raw)?;
            assert_eq!(updated.revision, 2);
            assert!(updated.builtin_scope.connections.is_empty());
            assert!(updated.builtin_scope.conversations.is_empty());
            let changes: u64 = tx.query_row("SELECT count(*) FROM mcp_server_changes WHERE workspace=?1 AND server_id=?2",params![workspace.to_string(),server_id.to_string()],|r|r.get(0)).map_err(db_error)?;
            assert_eq!(changes, 1);
            // A subsequent reuse of the same identifier must not restore grants.
            prune_connection_grants(&tx, workspace, connection, Some(&new), Some(&old), actor, Uuid::new_v4())?;
            let raw: String = tx.query_row("SELECT record FROM mcp_servers WHERE workspace=?1 AND id=?2",params![workspace.to_string(),server_id.to_string()],|r|r.get(0)).map_err(db_error)?;
            let updated: McpServer = decode(&raw)?;
            assert_eq!(updated.revision, 2);
            assert!(updated.builtin_scope.connections.is_empty());
            tx.commit().map_err(db_error)?;
            Ok(())
        }).await.unwrap();
    }

    #[tokio::test]
    async fn scope_uses_discovered_conversations_in_the_same_workspace_and_account() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path().join("scope.db"))
            .await
            .unwrap();
        let workspace = Uuid::new_v4();
        let source = Uuid::new_v4();
        let connection = Uuid::new_v4();
        let snapshot=json!({"revision":1,"connections":[{"id":connection,"name":"QQ","enabled":true,"config":{"kind":"qq_bot","app_id":"123456","group_openid":"","allowed_senders":[],"allow_proactive":false}}]}).to_string();
        store.run(move |c| {
            c.execute("INSERT INTO message_connections(workspace,source,revision,record) VALUES(?1,?2,1,?3)",params![workspace.to_string(),source.to_string(),snapshot]).map_err(db_error)?;
            c.execute("INSERT INTO connection_conversations(workspace,connection,account,kind,id,last_seen_ms) VALUES(?1,?2,'123456','group','group_A',1)",params![workspace.to_string(),connection.to_string()]).map_err(db_error)?;
            Ok(())
        }).await.unwrap();
        let group = ConversationGrant {
            connection_id: connection,
            account: "123456".into(),
            kind: "group".into(),
            conversation_id: "group_A".into(),
        };
        let scope = BuiltinScope {
            tools: vec![BuiltinTool::RecentMessages],
            connections: vec![connection],
            conversations: vec![group],
            qq_send_conversations: vec![],
        };
        assert!(
            store
                .validate_builtin_scope(workspace, scope.clone())
                .await
                .is_ok()
        );
        assert!(
            store
                .validate_builtin_scope(Uuid::new_v4(), scope.clone())
                .await
                .is_err()
        );
        let mut send = scope.clone();
        send.qq_send_conversations = send.conversations.clone();
        assert!(
            store.validate_builtin_scope(workspace, send).await.is_err(),
            "sending requires proactive permission even when reading is granted"
        );
        let mut wrong = scope.clone();
        wrong.conversations[0].account = "999999".into();
        assert!(
            store
                .validate_builtin_scope(workspace, wrong)
                .await
                .is_err()
        );
        let mut wrong = scope;
        wrong.conversations[0].conversation_id = "group_B".into();
        assert!(
            store
                .validate_builtin_scope(workspace, wrong)
                .await
                .is_err()
        );
    }
}
