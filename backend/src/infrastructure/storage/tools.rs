use super::{SqliteStore, db_error, decode, encode, millis};
use crate::{
    application::ports::{
        PortError, PortFuture,
        invocations::{Admission, InvocationKey, InvocationLedger, SavedOutcome},
        profiles::ProfileRepository,
    },
    domain::{audit::AuditRecord, profile::PublishedProfile},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use std::time::SystemTime;
use uuid::Uuid;

impl SqliteStore {
    /// Publish only fully validated trusted configuration. Sequential revisions are a
    /// CAS; restarting the exact active snapshot is idempotent. Secrets are excluded.
    pub async fn publish_tools(
        &self,
        profile: PublishedProfile,
        snapshot: Value,
        principal: Uuid,
    ) -> Result<(), PortError> {
        if !profile.is_valid() {
            return Err(PortError::InvalidInput {
                code: "invalid_profile",
            });
        }
        let record = super::capacity::encode_bounded(
            &serde_json::json!({"profile":profile,"config":snapshot}),
        )?;
        self.run_critical(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let w = profile.workspace_id.to_string();
            let p = profile.profile_id.to_string();
            let head: Option<(u64,String)> = tx.query_row(
                "SELECT h.revision,r.record FROM tool_heads h JOIN tool_releases r USING(workspace,profile,revision) WHERE h.workspace=?1 AND h.profile=?2",
                params![w,p], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
            if let Some((revision, existing)) = &head
                && *revision == profile.revision && *existing == record { return Ok(()); }
            if head.map_or(1, |(r,_)|r+1) != profile.revision { return Err(PortError::Conflict); }
            let now=millis(SystemTime::now())?;
            tx.execute("INSERT INTO tool_releases VALUES(?1,?2,?3,?4,?5)",params![w,p,profile.revision,record,now]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_heads VALUES(?1,?2,?3) ON CONFLICT(workspace,profile) DO UPDATE SET revision=excluded.revision",params![w,p,profile.revision]).map_err(db_error)?;
            tx.execute("INSERT INTO tool_publications VALUES(?1,?2,?3,?4,?5,?6)",params![Uuid::new_v4().to_string(),w,p,profile.revision,principal.to_string(),now]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)
        }).await
    }
}
impl ProfileRepository for SqliteStore {
    fn published(&self, workspace: Uuid, profile: Uuid) -> PortFuture<'_, PublishedProfile> {
        Box::pin(self.run(move |c| {
            let record:String=c.query_row("SELECT r.record FROM tool_heads h JOIN tool_releases r USING(workspace,profile,revision) WHERE h.workspace=?1 AND h.profile=?2",params![workspace.to_string(),profile.to_string()],|r|r.get(0)).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
            let value:Value=decode(&record)?;
            serde_json::from_value(value["profile"].clone()).map_err(|_|PortError::Unavailable)
        }))
    }
}
impl InvocationLedger for SqliteStore {
    fn begin(&self, key: InvocationKey, fingerprint: String) -> PortFuture<'_, Admission> {
        let path = self.path.clone();
        Box::pin(self.run(move |c| {
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let args=params![key.workspace_id.to_string(),key.principal_id.to_string(),key.invocation_id.to_string()];
            let old:Option<(String,Option<String>)>=tx.query_row("SELECT fingerprint,outcome FROM tool_invocations WHERE workspace=?1 AND principal=?2 AND invocation=?3",args,|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
            if let Some((previous,outcome))=old {
                if previous!=fingerprint { return Err(PortError::Conflict); }
                return Ok(Admission::Cached(match outcome {Some(v)=>decode(&v)?,None=>SavedOutcome::Unknown}));
            }
            if super::capacity::available_space(&path)? < 256*1024*1024 {
                return Err(PortError::CapacityExceeded {reason:"disk_low"});
            }
            // Bounded durable ledger: fail closed at capacity; never evict dedupe keys automatically.
            let (count,bytes):(u64,u64)=tx.query_row("SELECT items,bytes FROM tool_invocation_usage WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
            if count>=100_000 || bytes+263168>1024*1024*1024 { return Err(PortError::CapacityExceeded { reason:"invocation_ledger_full" }); }
            tx.execute("INSERT INTO tool_invocations VALUES(?1,?2,?3,?4,'running',NULL,?5,NULL,263168)",params![key.workspace_id.to_string(),key.principal_id.to_string(),key.invocation_id.to_string(),fingerprint,millis(SystemTime::now())?]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(Admission::Fresh)
        }))
    }
    fn finish(
        &self,
        key: InvocationKey,
        outcome: SavedOutcome,
        audit: AuditRecord,
    ) -> PortFuture<'_, ()> {
        Box::pin(self.run_critical(move|c| {
            if audit.workspace_id!=key.workspace_id || audit.principal_id!=key.principal_id || audit.operation_id!=key.invocation_id {
                return Err(PortError::ScopeMismatch);
            }
            let record=super::capacity::encode_bounded(&outcome)?;
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let changed=tx.execute("UPDATE tool_invocations SET state='completed',outcome=?4,finished_ms=?5,budget_bytes=length(CAST(?4 AS BLOB))+1024 WHERE workspace=?1 AND principal=?2 AND invocation=?3 AND state='running'",params![key.workspace_id.to_string(),key.principal_id.to_string(),key.invocation_id.to_string(),record,millis(SystemTime::now())?]).map_err(db_error)?;
            if changed!=1 {return Err(PortError::Conflict);}
            tx.execute("INSERT INTO audit VALUES(?1,?2,?3,?4)",params![audit.record_id.to_string(),audit.workspace_id.to_string(),millis(audit.occurred_at)?,encode(&audit)?]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)
        }))
    }
}
