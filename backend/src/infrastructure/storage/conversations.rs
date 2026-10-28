use super::*;
use crate::domain::event::EventEnvelope;
use serde::{Deserialize, Serialize};
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationQuery {
    pub kind: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}
#[derive(Serialize)]
pub struct Conversation {
    pub kind: String,
    pub id: String,
    pub last_seen_ms: i64,
    pub display_name: Option<String>,
}
#[derive(Serialize)]
pub struct ConversationPage {
    pub items: Vec<Conversation>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}
/// Called in the event transaction: verified discovery and event acceptance commit together.
pub(super) fn observe(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
) -> Result<(), PortError> {
    let Some(b) = event
        .route_context
        .as_ref()
        .filter(|b| b.adapter == "qq" && b.version == 1)
    else {
        return Ok(());
    };
    let s = |key: &str| b.config[key].as_str().ok_or(PortError::ScopeMismatch);
    let account = s("app_id")?;
    let kind = s("kind")?;
    let id = s("id")?;
    if !["group", "direct"].contains(&kind) || !crate::infrastructure::connectors::qq::openid(id) {
        return Err(PortError::ScopeMismatch);
    }
    let w = event.scope.workspace_id.to_string();
    let c = event.scope.connection_id.to_string();
    let display_name = match &event.payload {
        crate::domain::event::EventPayload::Json(value) if kind == "group" => value["group_name"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)),
        _ => None,
    };
    // The common path is a primary-key update; count the bounded directory only for new peers.
    if tx.execute("UPDATE connection_conversations SET last_seen_ms=MAX(last_seen_ms,?6),display_name=CASE WHEN ?6>=last_seen_ms THEN COALESCE(?7,display_name) ELSE display_name END WHERE workspace=?1 AND connection=?2 AND account=?3 AND kind=?4 AND id=?5",params![w,c,account,kind,id,millis(event.received_at)?,display_name]).map_err(db_error)? > 0 {
        return Ok(());
    }
    // Fixed bound per bot. Existing conversations keep refreshing once the directory is full.
    tx.execute("INSERT INTO connection_conversations(workspace,connection,account,kind,id,last_seen_ms,display_name) SELECT ?1,?2,?3,?4,?5,?6,?7 WHERE (SELECT count(*) FROM connection_conversations WHERE workspace=?1 AND connection=?2)<10000 ON CONFLICT(workspace,connection,account,kind,id) DO UPDATE SET last_seen_ms=MAX(last_seen_ms,excluded.last_seen_ms)",params![w,c,account,kind,id,millis(event.received_at)?,display_name]).map_err(db_error)?;
    Ok(())
}
impl SqliteStore {
    pub(crate) async fn conversations(
        &self,
        workspace: Uuid,
        connection: Uuid,
        account: String,
        q: ConversationQuery,
    ) -> Result<ConversationPage, PortError> {
        let page = q.page.unwrap_or(1);
        let size = q.page_size.unwrap_or(20);
        let kind = q.kind.unwrap_or_else(|| "group".into());
        let search = q.search.unwrap_or_default();
        if page == 0
            || !(1..=100).contains(&size)
            || search.len() > 128
            || !["group", "direct"].contains(&kind.as_str())
        {
            return Err(PortError::InvalidInput {
                code: "invalid_conversation_query",
            });
        }
        self.run(move|c|{
            let w=workspace.to_string();let conn=connection.to_string();let offset=u64::from(page-1)*u64::from(size);
            let total=c.query_row("SELECT count(*) FROM connection_conversations WHERE workspace=?1 AND connection=?2 AND account=?3 AND kind=?4 AND (instr(id,?5)>0 OR instr(COALESCE(display_name,''),?5)>0)",params![w,conn,account,kind,search],|r|r.get(0)).map_err(db_error)?;
            let mut query=c.prepare("SELECT kind,id,last_seen_ms,display_name FROM connection_conversations WHERE workspace=?1 AND connection=?2 AND account=?3 AND kind=?4 AND (instr(id,?5)>0 OR instr(COALESCE(display_name,''),?5)>0) ORDER BY last_seen_ms DESC,id LIMIT ?6 OFFSET ?7").map_err(db_error)?;
            let items=query.query_map(params![w,conn,account,kind,search,size,offset],|r|Ok(Conversation{kind:r.get(0)?,id:r.get(1)?,last_seen_ms:r.get(2)?,display_name:r.get(3)?})).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?;
            Ok(ConversationPage{items,total,page,page_size:size})
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn migration_preserves_existing_conversations_and_adds_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration.db");
        drop(SqliteStore::open(&path).await.unwrap());
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("ALTER TABLE connection_conversations DROP COLUMN display_name; PRAGMA user_version=20;").unwrap();
        c.execute("INSERT INTO connection_conversations(workspace,connection,account,kind,id,last_seen_ms) VALUES('w','c','a','group','g',1)",[]).unwrap();
        drop(c);
        drop(SqliteStore::open(&path).await.unwrap());
        let c = rusqlite::Connection::open(&path).unwrap();
        let (id, name): (String, Option<String>) = c
            .query_row(
                "SELECT id,display_name FROM connection_conversations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(id, "g");
        assert!(name.is_none());
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            21
        );
    }
}
