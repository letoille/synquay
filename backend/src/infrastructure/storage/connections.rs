use super::{SqliteStore, db_error, decode, millis};
use crate::{
    application::ports::{
        PortError,
        publications::{PublicationReceipt, PublicationScope},
    },
    domain::profile::PublishedProfile,
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use std::time::SystemTime;
use uuid::Uuid;

pub struct ConnectionPlan {
    pub scope: PublicationScope,
    pub operation_id: Uuid,
    pub expected_revision: u64,
    pub request: Value,
    pub tool: Value,
    pub profile: PublishedProfile,
    pub snapshot: Value,
}
impl SqliteStore {
    pub async fn managed_tools(
        &self,
        workspace: Uuid,
        profile: Uuid,
    ) -> Result<Vec<Value>, PortError> {
        self.run(move |c| {
            let mut stmt=c.prepare("SELECT m.tool_record FROM tool_connection_overrides o JOIN managed_tool_connections m ON m.workspace=o.workspace AND m.profile=o.profile AND m.operation=o.connection WHERE o.workspace=?1 AND o.profile=?2 AND o.tool_record IS NOT NULL ORDER BY o.connection LIMIT 33").map_err(db_error)?;
            let rows=stmt.query_map(params![workspace.to_string(),profile.to_string()],|r|r.get::<_,String>(0)).map_err(db_error)?;
            let result=rows.map(|r|decode(&r.map_err(db_error)?)).collect::<Result<Vec<_>,_>>()?;
            if result.len()>32 {return Err(PortError::PayloadTooLarge)}
            Ok(result)
        }).await
    }
    pub async fn connection_receipt(
        &self,
        scope: PublicationScope,
        operation: Uuid,
    ) -> Result<Option<(Value, PublicationReceipt)>, PortError> {
        self.run(move|c| {
            let row:Option<(String,String,u64)>=c.query_row("SELECT actor,request_record,revision FROM managed_tool_connections WHERE workspace=?1 AND profile=?2 AND operation=?3 UNION ALL SELECT actor,request_record,revision FROM tool_connection_changes WHERE workspace=?1 AND profile=?2 AND operation=?3",params![scope.workspace_id.to_string(),scope.profile_id.to_string(),operation.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            row.map(|(actor,request,revision)| {
                if actor!=scope.actor_id.to_string() {return Err(PortError::PermissionDenied)}
                Ok((decode(&request)?,PublicationReceipt{operation_id:operation,revision,replayed:true}))
            }).transpose()
        }).await
    }
    pub async fn create_connection(
        &self,
        plan: ConnectionPlan,
    ) -> Result<PublicationReceipt, PortError> {
        let path = self.path.clone();
        self.run_critical(move|c| {
            let ConnectionPlan{scope,operation_id,expected_revision,request,tool,profile,snapshot}=plan;
            if operation_id.is_nil() || expected_revision==0 || expected_revision>=i64::MAX as u64-1 || profile.revision!=expected_revision+1 || profile.workspace_id!=scope.workspace_id || profile.profile_id!=scope.profile_id || !profile.is_valid() {return Err(PortError::InvalidInput{code:"invalid_connection_plan"})}
            let request=super::capacity::encode_bounded(&request)?;
            let tool=super::capacity::encode_bounded(&tool)?;
            let record=super::capacity::encode_bounded(&serde_json::json!({"profile":profile,"config":snapshot}))?;
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let w=scope.workspace_id.to_string();let p=scope.profile_id.to_string();
            let existing:Option<(String,String,u64)>=tx.query_row("SELECT actor,request_record,revision FROM managed_tool_connections WHERE workspace=?1 AND profile=?2 AND operation=?3",params![w,p,operation_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            if let Some((actor,old,revision))=existing {
                if actor!=scope.actor_id.to_string() || old!=request {return Err(PortError::Conflict)}
                return Ok(PublicationReceipt{operation_id,revision,replayed:true});
            }
            if super::capacity::available_space(&path)?<256*1024*1024 {return Err(PortError::CapacityExceeded{reason:"disk_low"})}
            let (active,old):(u64,String)=tx.query_row("SELECT revision,record FROM tool_heads JOIN tool_releases USING(workspace,profile,revision) WHERE workspace=?1 AND profile=?2",params![w,p],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
            let old:Value=decode(&old)?;
            if active!=expected_revision || old["profile"]["tools"]!=serde_json::to_value(&profile.tools).map_err(|_|PortError::Unavailable)? {return Err(PortError::Conflict)}
            let now=millis(SystemTime::now())?;
            tx.execute("INSERT INTO tool_releases VALUES(?1,?2,?3,?4,?5)",params![w,p,profile.revision,record,now]).map_err(db_error)?;
            tx.execute("UPDATE tool_heads SET revision=?3 WHERE workspace=?1 AND profile=?2",params![w,p,profile.revision]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_publications VALUES(?1,?2,?3,?4,?5,?6)",params![Uuid::new_v4().to_string(),w,p,profile.revision,scope.actor_id.to_string(),now]).map_err(db_error)?;
            tx.execute("INSERT INTO managed_tool_connections VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![w,p,operation_id.to_string(),scope.actor_id.to_string(),expected_revision,profile.revision,request,tool]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_connection_overrides VALUES(?1,?2,?3,?4)",params![w,p,operation_id.to_string(),tool]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(PublicationReceipt{operation_id,revision:profile.revision,replayed:false})
        }).await
    }
}

impl SqliteStore {
    /// Status only, scoped to the authenticated calling principal; never reveals tool output.
    pub async fn invocation_status(
        &self,
        workspace: Uuid,
        principal: Uuid,
        invocation: Uuid,
    ) -> Result<&'static str, PortError> {
        self.run(move|c| {
            let row:Option<Option<String>>=c.query_row("SELECT outcome FROM tool_invocations WHERE workspace=?1 AND principal=?2 AND invocation=?3",params![workspace.to_string(),principal.to_string(),invocation.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            use crate::application::ports::invocations::SavedOutcome;
            match row {
                None=>Ok("unconfirmed"),Some(None)=>Ok("unknown"),
                Some(Some(value))=>match decode::<SavedOutcome>(&value)? {SavedOutcome::Completed(result)=>Ok(if result.is_error {"failed"} else {"succeeded"}),SavedOutcome::Failed=>Ok("failed"),SavedOutcome::Unknown=>Ok("unknown")}
            }
        }).await
    }
}

impl SqliteStore {
    pub async fn connection_overrides(
        &self,
        workspace: Uuid,
        profile: Uuid,
        ids: Vec<Uuid>,
    ) -> Result<Vec<(Uuid, Option<Value>)>, PortError> {
        self.run(move |c| {
            let mut stmt = c.prepare("SELECT connection,tool_record FROM tool_connection_overrides WHERE workspace=?1 AND profile=?2 AND connection IN (SELECT value FROM json_each(?3)) ORDER BY connection").map_err(db_error)?;
            let rows = stmt.query_map(params![workspace.to_string(),profile.to_string(),serde_json::to_string(&ids).map_err(|_|PortError::Unavailable)?], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?))).map_err(db_error)?;
            rows.map(|row| { let (id,record)=row.map_err(db_error)?; Ok((Uuid::parse_str(&id).map_err(|_|PortError::Unavailable)?, record.map(|v|decode(&v)).transpose()?)) }).collect()
        }).await
    }
    /// Resolve only previously configured, now-deleted names for startup grant validation.
    /// These records never enter the active inventory or connector registry.
    pub async fn retired_grant_tools(
        &self,
        workspace: Uuid,
        profile: Uuid,
        names: Vec<String>,
    ) -> Result<Vec<Value>, PortError> {
        self.run(move |c| {
            let mut stmt=c.prepare("SELECT m.tool_record FROM tool_connection_overrides o JOIN managed_tool_connections m ON m.workspace=o.workspace AND m.profile=o.profile AND m.operation=o.connection WHERE o.workspace=?1 AND o.profile=?2 AND o.tool_record IS NULL AND json_extract(m.tool_record,'$.public_name') IN (SELECT value FROM json_each(?3)) GROUP BY json_extract(m.tool_record,'$.public_name')").map_err(db_error)?;
            let rows=stmt.query_map(params![workspace.to_string(),profile.to_string(),serde_json::to_string(&names).map_err(|_|PortError::Unavailable)?],|r|r.get::<_,String>(0)).map_err(db_error)?;
            rows.map(|r|decode(&r.map_err(db_error)?)).collect()
        }).await
    }
    /// The inventory, every affected publication, tombstone and audit receipt commit together.
    pub async fn change_connection(
        &self,
        plan: ConnectionPlan,
        connection: Uuid,
        replacement: Option<crate::domain::profile::PublishedTool>,
    ) -> Result<PublicationReceipt, PortError> {
        let path = self.path.clone();
        self.run_critical(move |c| {
            let ConnectionPlan {scope,operation_id,expected_revision,request,tool,profile,snapshot}=plan;
            let w=scope.workspace_id.to_string(); let p=scope.profile_id.to_string();
            let request=super::capacity::encode_bounded(&request)?;
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let previous:Option<(String,String,u64)>=tx.query_row("SELECT actor,request_record,revision FROM tool_connection_changes WHERE workspace=?1 AND profile=?2 AND operation=?3 UNION ALL SELECT actor,request_record,revision FROM managed_tool_connections WHERE workspace=?1 AND profile=?2 AND operation=?3",params![w,p,operation_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            if let Some((actor,old,revision))=previous {
                if actor!=scope.actor_id.to_string() || old!=request { return Err(PortError::Conflict); }
                return Ok(PublicationReceipt {operation_id,revision,replayed:true});
            }
            if super::capacity::available_space(&path)? < 256*1024*1024 { return Err(PortError::CapacityExceeded{reason:"disk_low"}); }
            let active:u64=tx.query_row("SELECT revision FROM tool_heads WHERE workspace=?1 AND profile=?2",params![w,p],|r|r.get(0)).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
            if active!=expected_revision || profile.revision!=active+1 || profile.profile_id!=scope.profile_id || profile.workspace_id!=scope.workspace_id || !profile.is_valid() {return Err(PortError::Conflict);}
            let releases = {
                let mut stmt=tx.prepare("SELECT record FROM tool_heads JOIN tool_releases USING(workspace,profile,revision) WHERE workspace=?1").map_err(db_error)?;
                let rows=stmt.query_map([&w],|r|r.get::<_,String>(0)).map_err(db_error)?;
                rows.map(|r|decode::<Value>(&r.map_err(db_error)?)).collect::<Result<Vec<_>,_>>()?
            };
            let old_name=releases.iter().find(|v|v["profile"]["profile_id"].as_str()==Some(&p))
                .and_then(|v|v["config"]["tools"].as_array()).and_then(|tools|tools.iter().find(|t|t["connection_id"].as_str()==Some(&connection.to_string())))
                .and_then(|t|t["public_name"].as_str()).ok_or(PortError::NotFound)?.to_string();
            let now=millis(SystemTime::now())?;
            for old in releases {
                let mut next:PublishedProfile=serde_json::from_value(old["profile"].clone()).map_err(|_|PortError::Unavailable)?;
                let default=next.profile_id==scope.profile_id;
                if !default && !next.tools.iter().any(|t|t.scope.connection_id==connection) {continue;}
                let old_revision=next.revision;
                if default {next=profile.clone();} else {
                    next.revision=next.revision.checked_add(1).ok_or(PortError::Conflict)?;
                    next.tools=next.tools.into_iter().filter_map(|t|if t.scope.connection_id==connection {replacement.clone()}else{Some(t)}).collect();
                }
                if next.revision>=i64::MAX as u64 || !next.is_valid() {return Err(PortError::Conflict);}
                let mut config=snapshot.clone(); config["profile_id"]=serde_json::json!(next.profile_id);
                if let Some(tools)=config["tools"].as_array_mut() {for t in tools {t["exposed"]=serde_json::json!(next.tools.iter().any(|b|Some(b.scope.connection_id.to_string()).as_deref()==t["connection_id"].as_str()));}}
                let record=super::capacity::encode_bounded(&serde_json::json!({"profile":next,"config":config}))?;
                let id=next.profile_id.to_string();
                tx.execute("INSERT INTO tool_releases VALUES(?1,?2,?3,?4,?5)",params![w,id,next.revision,record,now]).map_err(db_error)?;
                let updated=tx.execute("UPDATE tool_heads SET revision=?3 WHERE workspace=?1 AND profile=?2 AND revision=?4",params![w,id,next.revision,old_revision]).map_err(db_error)?;
                if updated!=1 {return Err(PortError::Conflict);}
                tx.execute("INSERT INTO tool_publications VALUES(?1,?2,?3,?4,?5,?6)",params![Uuid::new_v4().to_string(),w,id,next.revision,scope.actor_id.to_string(),now]).map_err(db_error)?;
            }
            // Invalidate stale draft generations while preserving unrelated selections.
            let drafts={
                let mut stmt=tx.prepare("SELECT profile,generation,base_revision,exposed_tools FROM tool_drafts WHERE workspace=?1").map_err(db_error)?;
                let rows=stmt.query_map([&w],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?,r.get::<_,u64>(2)?,r.get::<_,String>(3)?))).map_err(db_error)?;
                rows.collect::<Result<Vec<_>,_>>().map_err(db_error)?
            };
            for (draft_profile,generation,base,names) in drafts {
                let mut names:Vec<String>=decode(&names)?;
                if !names.contains(&old_name) {continue;}
                names=names.into_iter().filter_map(|name|if name==old_name {replacement.as_ref().map(|t|t.public_name.clone())}else{Some(name)}).collect();
                names.sort();names.dedup();
                let generation=generation.checked_add(1).filter(|v|*v<i64::MAX as u64).ok_or(PortError::Conflict)?;
                let names=super::capacity::encode_bounded(&serde_json::json!(names))?;
                tx.execute("UPDATE tool_drafts SET generation=?3,exposed_tools=?4,actor=?5,updated_ms=?6 WHERE workspace=?1 AND profile=?2",params![w,draft_profile,generation,names,scope.actor_id.to_string(),now]).map_err(db_error)?;
                tx.execute("INSERT INTO tool_draft_audit VALUES(?1,?2,?3,?4,?5,?6,?7)",params![w,draft_profile,generation,base,names,scope.actor_id.to_string(),now]).map_err(db_error)?;
            }
            let tool=if tool.is_null(){None}else{Some(super::capacity::encode_bounded(&tool)?)};
            tx.execute("INSERT INTO tool_connection_overrides VALUES(?1,?2,?3,?4) ON CONFLICT(workspace,profile,connection) DO UPDATE SET tool_record=excluded.tool_record",params![w,p,connection.to_string(),tool]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_connection_changes VALUES(?1,?2,?3,?4,?5,?6)",params![w,p,operation_id.to_string(),scope.actor_id.to_string(),request,profile.revision]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(PublicationReceipt {operation_id,revision:profile.revision,replayed:false})
        }).await
    }
}
