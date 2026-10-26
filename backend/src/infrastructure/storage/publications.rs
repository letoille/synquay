use super::{SqliteStore, db_error, decode, millis};
use crate::application::ports::{PortError, PortFuture, publications::*};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use std::time::SystemTime;
use uuid::Uuid;

fn operation(
    c: &Connection,
    scope: &PublicationScope,
    r: &PublishInput,
) -> Result<Option<PublicationReceipt>, PortError> {
    let old:Option<(String,u64,u64,u64)>=c.query_row(
        "SELECT actor,expected_revision,draft_generation,revision FROM tool_publish_operations WHERE workspace=?1 AND profile=?2 AND operation=?3",
        params![scope.workspace_id.to_string(),scope.profile_id.to_string(),r.operation_id.to_string()],
        |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(db_error)?;
    match old {
        Some((actor, expected, generation, revision)) => {
            if actor != scope.actor_id.to_string()
                || expected != r.expected_revision
                || generation != r.draft_generation
            {
                return Err(PortError::Conflict);
            }
            Ok(Some(PublicationReceipt {
                operation_id: r.operation_id,
                revision,
                replayed: true,
            }))
        }
        None => Ok(None),
    }
}
impl SqliteStore {
    /// Indexed existence checks distinguish console-managed state from file-only releases.
    pub async fn has_online_tool_changes(
        &self,
        workspace: Uuid,
        profile: Uuid,
    ) -> Result<bool, PortError> {
        self.run(move |c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM tool_publish_operations WHERE workspace=?1 AND profile=?2) OR EXISTS(SELECT 1 FROM managed_tool_connections WHERE workspace=?1 AND profile=?2) OR EXISTS(SELECT 1 FROM tool_connection_changes WHERE workspace=?1 AND profile=?2) OR EXISTS(SELECT 1 FROM mcp_server_changes WHERE workspace=?1 AND server_id=?2)",
                params![workspace.to_string(), profile.to_string()],
                |row| row.get(0),
            ).map_err(db_error)
        }).await
    }

    pub async fn tools_snapshot(
        &self,
        workspace: Uuid,
        profile: Uuid,
    ) -> Result<Option<Value>, PortError> {
        self.run(move|c| {
            let record:Option<String>=c.query_row("SELECT r.record FROM tool_heads h JOIN tool_releases r USING(workspace,profile,revision) WHERE h.workspace=?1 AND h.profile=?2",params![workspace.to_string(),profile.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            record.map(|s|decode(&s)).transpose()
        }).await
    }
}
impl ToolPublicationStore for SqliteStore {
    fn history(
        &self,
        scope: PublicationScope,
        before_revision: Option<u64>,
        limit: u32,
    ) -> PortFuture<'_, PublicationHistory> {
        Box::pin(self.run(move |c| {
            if !(1..=50).contains(&limit)
                || before_revision.is_some_and(|r| r == 0 || r > i64::MAX as u64)
            {
                return Err(PortError::InvalidInput {
                    code: "invalid_history_page",
                });
            }
            // Cursor range scan on releases' PK, indexed audit lookup per row.
            // Return capability names, never whole snapshots or secret references.
            let mut query = c
                .prepare(
                    "SELECT r.revision,r.created_ms,p.principal,r.record
                 FROM tool_releases r JOIN tool_publications p
                 ON p.workspace=r.workspace AND p.profile=r.profile AND p.revision=r.revision
                 WHERE r.workspace=?1 AND r.profile=?2 AND r.revision<?3
                 ORDER BY r.revision DESC LIMIT ?4",
                )
                .map_err(db_error)?;
            let rows = query
                .query_map(
                    params![
                        scope.workspace_id.to_string(),
                        scope.profile_id.to_string(),
                        before_revision.unwrap_or(i64::MAX as u64),
                        limit + 1
                    ],
                    |r| {
                        Ok((
                            r.get::<_, u64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    },
                )
                .map_err(db_error)?;
            let mut items = Vec::new();
            for row in rows {
                let (revision, created_ms, actor, record) = row.map_err(db_error)?;
                let record: Value = decode(&record)?;
                let profile: crate::domain::profile::PublishedProfile =
                    serde_json::from_value(record["profile"].clone())
                        .map_err(|_| PortError::Unavailable)?;
                items.push(PublicationEntry {
                    revision,
                    created_ms,
                    actor_id: Uuid::parse_str(&actor).map_err(|_| PortError::Unavailable)?,
                    exposed_tools: profile.tools.into_iter().map(|t| t.public_name).collect(),
                });
            }
            let more = items.len() > limit as usize;
            items.truncate(limit as usize);
            let next_before_revision = if more {
                items.last().map(|e| e.revision)
            } else {
                None
            };
            Ok(PublicationHistory {
                items,
                next_before_revision,
            })
        }))
    }
    fn draft(&self, scope: PublicationScope) -> PortFuture<'_, Option<ExposureDraft>> {
        Box::pin(self.run(move|c| {
            let row:Option<(u64,u64,String,String)>=c.query_row("SELECT generation,base_revision,exposed_tools,actor FROM tool_drafts WHERE workspace=?1 AND profile=?2",params![scope.workspace_id.to_string(),scope.profile_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
            row.map(|(generation,base_revision,names,actor)|Ok(ExposureDraft{generation,base_revision,exposed_tools:decode(&names)?,actor_id:Uuid::parse_str(&actor).map_err(|_|PortError::Unavailable)?})).transpose()
        }))
    }
    fn save_draft(
        &self,
        scope: PublicationScope,
        input: DraftInput,
    ) -> PortFuture<'_, ExposureDraft> {
        let path = self.path.clone();
        Box::pin(self.run(move|c| {
            if super::capacity::available_space(&path)?<256*1024*1024 {return Err(PortError::CapacityExceeded{reason:"disk_low"});}
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let w=scope.workspace_id.to_string();let p=scope.profile_id.to_string();
            let revision:u64=tx.query_row("SELECT revision FROM tool_heads WHERE workspace=?1 AND profile=?2",params![w,p],|r|r.get(0)).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
            let generation:Option<u64>=tx.query_row("SELECT generation FROM tool_drafts WHERE workspace=?1 AND profile=?2",params![w,p],|r|r.get(0)).optional().map_err(db_error)?;
            if revision!=input.base_revision || generation.unwrap_or(0)!=input.expected_generation || input.expected_generation>=i64::MAX as u64-1 {return Err(PortError::Conflict);}
            let generation=input.expected_generation+1;
            let names=super::capacity::encode_bounded(&input.exposed_tools)?;let now=millis(SystemTime::now())?;
            tx.execute("INSERT INTO tool_drafts VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(workspace,profile) DO UPDATE SET generation=excluded.generation,base_revision=excluded.base_revision,exposed_tools=excluded.exposed_tools,actor=excluded.actor,updated_ms=excluded.updated_ms",params![w,p,generation,revision,names,scope.actor_id.to_string(),now]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_draft_audit VALUES(?1,?2,?3,?4,?5,?6,?7)",params![w,p,generation,revision,names,scope.actor_id.to_string(),now]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(ExposureDraft{generation,base_revision:revision,exposed_tools:input.exposed_tools,actor_id:scope.actor_id})
        }))
    }
    fn published_operation(
        &self,
        scope: PublicationScope,
        request: PublishInput,
    ) -> PortFuture<'_, Option<PublicationReceipt>> {
        Box::pin(self.run(move |c| operation(c, &scope, &request)))
    }
    fn publish_draft(&self, plan: PublicationPlan) -> PortFuture<'_, PublicationReceipt> {
        let path = self.path.clone();
        Box::pin(self.run_critical(move|c| {
            let PublicationPlan{scope,request,profile,snapshot}=plan;
            if request.operation_id.is_nil() || request.expected_revision>=i64::MAX as u64-1 || profile.workspace_id!=scope.workspace_id || profile.profile_id!=scope.profile_id || profile.revision!=request.expected_revision+1 || !profile.is_valid() {return Err(PortError::InvalidInput{code:"invalid_publication"});}
            let record=super::capacity::encode_bounded(&serde_json::json!({"profile":profile,"config":snapshot}))?;
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            if let Some(receipt)=operation(&tx,&scope,&request)? {return Ok(receipt);}
            if super::capacity::available_space(&path)?<256*1024*1024 {return Err(PortError::CapacityExceeded{reason:"disk_low"});}
            let w=scope.workspace_id.to_string();let p=scope.profile_id.to_string();
            let active:u64=tx.query_row("SELECT revision FROM tool_heads WHERE workspace=?1 AND profile=?2",params![w,p],|r|r.get(0)).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
            let (generation,base,names):(u64,u64,String)=tx.query_row("SELECT generation,base_revision,exposed_tools FROM tool_drafts WHERE workspace=?1 AND profile=?2",params![w,p],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
            let mut expected:Vec<String>=decode(&names)?;expected.sort();
            let mut actual:Vec<String>=profile.tools.iter().map(|t|t.public_name.clone()).collect();actual.sort();
            if active!=request.expected_revision || base!=active || generation!=request.draft_generation || expected!=actual {return Err(PortError::Conflict);}
            let now=millis(SystemTime::now())?;
            tx.execute("INSERT INTO tool_releases VALUES(?1,?2,?3,?4,?5)",params![w,p,profile.revision,record,now]).map_err(db_error)?;
            tx.execute("UPDATE tool_heads SET revision=?3 WHERE workspace=?1 AND profile=?2",params![w,p,profile.revision]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_publications VALUES(?1,?2,?3,?4,?5,?6)",params![Uuid::new_v4().to_string(),w,p,profile.revision,scope.actor_id.to_string(),now]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_publish_operations VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![w,p,request.operation_id.to_string(),scope.actor_id.to_string(),request.expected_revision,request.draft_generation,profile.revision,now]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(PublicationReceipt{operation_id:request.operation_id,revision:profile.revision,replayed:false})
        }))
    }
}
