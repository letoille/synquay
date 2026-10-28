use super::{SqliteStore, db_error, decode, encode, millis};
use crate::{
    application::ports::PortError,
    domain::{mcp_server::McpServer, profile::PublishedProfile},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use std::time::SystemTime;
use uuid::Uuid;

impl SqliteStore {
    pub async fn bootstrap_mcp_server(&self, workspace: Uuid, id: Uuid) -> Result<(), PortError> {
        self.run_critical(move |c| {
            let server = McpServer {
                id,
                builtin_scope: Default::default(),
                name: "默认 MCP 服务".into(),
                description: String::new(),
                public_url: None,
                content_scope: None,
                enabled: true,
                deleted: false,
                revision: 1,
                auth: Default::default(),
                transport: Default::default(),
            };
            // Tombstones are retained: a deleted endpoint must not reappear at restart.
            c.execute(
                "INSERT OR IGNORE INTO mcp_servers(workspace,id,name,record) VALUES(?1,?2,?3,?4)",
                params![
                    workspace.to_string(),
                    id.to_string(),
                    server.name,
                    encode(&server)?
                ],
            )
            .map_err(db_error)?;
            let record: String = c
                .query_row(
                    "SELECT record FROM mcp_servers WHERE workspace=?1 AND id=?2",
                    params![workspace.to_string(), id.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            let current: McpServer = decode(&record)?;
            c.execute(
                "UPDATE mcp_servers SET route_path=?3 WHERE workspace=?1 AND id=?2",
                params![workspace.to_string(), id.to_string(), current.path(id)],
            )
            .map_err(db_error)?;
            Ok(())
        })
        .await
    }
    pub async fn mcp_servers(&self, workspace: Uuid) -> Result<Vec<McpServer>, PortError> {
        self.run(move |c| {
            let mut query = c.prepare("SELECT record FROM mcp_servers WHERE workspace=?1 AND deleted=0 ORDER BY name,id LIMIT 64").map_err(db_error)?;
            let rows = query.query_map([workspace.to_string()], |r| r.get::<_,String>(0)).map_err(db_error)?;
            rows.map(|r| decode(&r.map_err(db_error)?)).collect()
        }).await
    }
    pub async fn mcp_server_by_path(
        &self,
        workspace: Uuid,
        path: String,
    ) -> Result<McpServer, PortError> {
        self.run(move |c| {
            let record: String = c.query_row("SELECT record FROM mcp_servers WHERE workspace=?1 AND route_path=?2 AND deleted=0",params![workspace.to_string(),path],|r|r.get(0)).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
            decode(&record)
        }).await
    }
    pub async fn mcp_server(&self, workspace: Uuid, id: Uuid) -> Result<McpServer, PortError> {
        self.run(move |c| {
            let record: String = c
                .query_row(
                    "SELECT record FROM mcp_servers WHERE workspace=?1 AND id=?2",
                    params![workspace.to_string(), id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?
                .ok_or(PortError::NotFound)?;
            decode(&record)
        })
        .await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn commit_mcp_server(
        &self,
        workspace: Uuid,
        id: Uuid,
        actor: Uuid,
        operation: Uuid,
        expected: u64,
        expected_tools: u64,
        request: Value,
        next: McpServer,
        route_path: String,
        publication: Option<(PublishedProfile, Value)>,
    ) -> Result<McpServer, PortError> {
        self.run_critical(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let w=workspace.to_string(); let p=id.to_string(); let op=operation.to_string();
            let request=encode(&request)?;
            let previous: Option<(String,Option<String>,Option<String>)> = tx.query_row("SELECT record,last_operation,last_request FROM mcp_servers WHERE workspace=?1 AND id=?2",params![w,p],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            if let Some((record,last_op,last_request))=&previous {
                if last_op.as_ref()==Some(&op) {
                    return if last_request.as_ref()==Some(&request) { decode(record) } else { Err(PortError::Conflict) };
                }
                let old: McpServer=decode(record)?;
                if old.deleted || old.revision!=expected { return Err(PortError::Conflict); }
            } else if expected!=0 || next.deleted { return Err(PortError::NotFound); }
            if previous.is_none() {
                let count: u64=tx.query_row("SELECT count(*) FROM mcp_servers WHERE workspace=?1",[&w],|r|r.get(0)).map_err(db_error)?;
                if count>=64 { return Err(PortError::CapacityExceeded { reason: "mcp_servers" }); }
            }
            let taken: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE workspace=?1 AND lower(name)=lower(?2) AND deleted=0 AND id!=?3)",params![w,next.name,p],|r|r.get(0)).map_err(db_error)?;
            if !next.deleted && taken { return Err(PortError::Conflict); }
            let route_taken: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mcp_servers WHERE workspace=?1 AND route_path=?2 AND deleted=0 AND id!=?3)",params![w,route_path,p],|r|r.get(0)).map_err(db_error)?;
            if !next.deleted && route_taken { return Err(PortError::InvalidInput { code: "mcp_url_taken" }); }
            let now=millis(SystemTime::now())?;
            if let Some((profile,snapshot))=publication {
                let head: Option<u64>=tx.query_row("SELECT revision FROM tool_heads WHERE workspace=?1 AND profile=?2",params![w,p],|r|r.get(0)).optional().map_err(db_error)?;
                if head.unwrap_or(0)!=expected_tools || profile.profile_id!=id || profile.workspace_id!=workspace || profile.revision!=expected_tools+1 || !profile.is_valid() { return Err(PortError::Conflict); }
                let record=super::capacity::encode_bounded(&serde_json::json!({"profile":profile,"config":snapshot}))?;
                tx.execute("INSERT INTO tool_releases VALUES(?1,?2,?3,?4,?5)",params![w,p,profile.revision,record,now]).map_err(db_error)?;
                tx.execute("INSERT INTO tool_heads VALUES(?1,?2,?3) ON CONFLICT(workspace,profile) DO UPDATE SET revision=excluded.revision",params![w,p,profile.revision]).map_err(db_error)?;
                tx.execute("INSERT INTO tool_publications VALUES(?1,?2,?3,?4,?5,?6)",params![Uuid::new_v4().to_string(),w,p,profile.revision,actor.to_string(),now]).map_err(db_error)?;
            }
            let record=encode(&next)?;
            tx.execute("INSERT INTO mcp_servers(workspace,id,name,deleted,record,last_operation,last_request,route_path) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(workspace,id) DO UPDATE SET name=excluded.name,deleted=excluded.deleted,record=excluded.record,last_operation=excluded.last_operation,last_request=excluded.last_request,route_path=excluded.route_path",params![w,p,next.name,next.deleted,record,op,request,route_path]).map_err(db_error)?;
            tx.execute("INSERT INTO mcp_server_changes VALUES(?1,?2,?3,?4,?5,?6,?7)",params![w,p,next.revision,actor.to_string(),op,record,now]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(next)
        }).await
    }
}
