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
    // The common path is a primary-key update; count the bounded directory only for new peers.
    if tx.execute("UPDATE connection_conversations SET last_seen_ms=MAX(last_seen_ms,?6) WHERE workspace=?1 AND connection=?2 AND account=?3 AND kind=?4 AND id=?5",params![w,c,account,kind,id,millis(event.received_at)?]).map_err(db_error)? > 0 {
        return Ok(());
    }
    // Fixed bound per bot. Existing conversations keep refreshing once the directory is full.
    tx.execute("INSERT INTO connection_conversations(workspace,connection,account,kind,id,last_seen_ms) SELECT ?1,?2,?3,?4,?5,?6 WHERE (SELECT count(*) FROM connection_conversations WHERE workspace=?1 AND connection=?2)<10000 ON CONFLICT(workspace,connection,account,kind,id) DO UPDATE SET last_seen_ms=MAX(last_seen_ms,excluded.last_seen_ms)",params![w,c,account,kind,id,millis(event.received_at)?]).map_err(db_error)?;
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
            let total=c.query_row("SELECT count(*) FROM connection_conversations WHERE workspace=?1 AND connection=?2 AND account=?3 AND kind=?4 AND instr(id,?5)>0",params![w,conn,account,kind,search],|r|r.get(0)).map_err(db_error)?;
            let mut query=c.prepare("SELECT kind,id,last_seen_ms FROM connection_conversations WHERE workspace=?1 AND connection=?2 AND account=?3 AND kind=?4 AND instr(id,?5)>0 ORDER BY last_seen_ms DESC,id LIMIT ?6 OFFSET ?7").map_err(db_error)?;
            let items=query.query_map(params![w,conn,account,kind,search,size,offset],|r|Ok(Conversation{kind:r.get(0)?,id:r.get(1)?,last_seen_ms:r.get(2)?})).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?;
            Ok(ConversationPage{items,total,page,page_size:size})
        }).await
    }
}
