//! Bounded retirement: body, audit and dedupe windows are independent.
use super::{SqliteStore, db_error, millis};
use crate::application::ports::PortError;
use rusqlite::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetentionPolicy {
    pub payload_days: u32,
    pub audit_days: u32,
    pub dedup_days: u32,
    pub batch_size: usize,
}
impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            payload_days: 7,
            audit_days: 30,
            dedup_days: 90,
            batch_size: 100,
        }
    }
}
impl RetentionPolicy {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.payload_days == 0
            || self.audit_days == 0
            || self.dedup_days < self.payload_days
            || self.dedup_days > 36500
            || self.audit_days > 36500
            || !(1..=100).contains(&self.batch_size)
        {
            Err(PortError::InvalidInput {
                code: "retention_policy",
            })
        } else {
            Ok(())
        }
    }
}
#[derive(Default, Debug, Serialize)]
pub struct MaintenanceReport {
    pub payload_rows: usize,
    pub audit_rows: usize,
    pub expired_rows: usize,
    pub wal_busy: i64,
    pub wal_frames: i64,
    pub wal_checkpointed: i64,
}
fn cutoff(now: SystemTime, days: u32) -> Result<i64, PortError> {
    millis(
        now.checked_sub(Duration::from_secs(u64::from(days) * 86400))
            .ok_or(PortError::InvalidInput {
                code: "retention_clock",
            })?,
    )
}
impl SqliteStore {
    pub async fn maintain(
        &self,
        workspace: Uuid,
        policy: RetentionPolicy,
    ) -> Result<MaintenanceReport, PortError> {
        policy.validate()?;
        self.run(move|c| {
            let now=SystemTime::now();let body=cutoff(now,policy.payload_days)?;let audit=cutoff(now,policy.audit_days)?;let dedup=cutoff(now,policy.dedup_days)?;
            let w=workspace.to_string();let limit=policy.batch_size as i64;
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let mut report=MaintenanceReport::default();
            // Each statement changes <= batch_size rows, even a dispatch with many manual replays.
            report.payload_rows+=tx.execute("UPDATE deliveries SET record='' WHERE rowid IN (SELECT d.rowid FROM retired_work r JOIN deliveries d ON d.workspace=r.workspace AND d.dispatch_key=r.work_key WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=0 AND d.record<>'' LIMIT ?3)",params![w,body,limit]).map_err(db_error)?;
            report.payload_rows+=tx.execute("UPDATE events SET record='' WHERE rowid IN (SELECT e.rowid FROM retired_work r JOIN events e ON e.workspace=r.workspace AND e.source=substr(r.work_key,7,36) AND e.event_id=substr(r.work_key,44,36) WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=0 AND r.work_key LIKE 'event:%' AND e.record<>'' LIMIT ?3)",params![w,body,limit]).map_err(db_error)?;
            // Release content only after the entire dispatch's body has been removed.
            tx.execute("DELETE FROM event_content WHERE rowid IN (SELECT ec.rowid FROM retired_work r JOIN event_content ec ON ec.workspace=r.workspace AND ec.source=substr(r.work_key,7,36) AND ec.event_id=substr(r.work_key,44,36) WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.work_key LIKE 'event:%' AND NOT EXISTS(SELECT 1 FROM events e WHERE e.workspace=r.workspace AND e.source=ec.source AND e.event_id=ec.event_id AND e.record<>'') AND NOT EXISTS(SELECT 1 FROM deliveries d WHERE d.workspace=r.workspace AND d.dispatch_key=r.work_key AND d.record<>'') LIMIT ?3)",params![w,body,limit]).map_err(db_error)?;
            tx.execute("UPDATE retired_work SET payload_purged=1 WHERE rowid IN (SELECT r.rowid FROM retired_work r WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=0 AND NOT EXISTS(SELECT 1 FROM deliveries d WHERE d.workspace=r.workspace AND d.dispatch_key=r.work_key AND d.record<>'') AND NOT EXISTS(SELECT 1 FROM events e WHERE e.workspace=r.workspace AND e.source=substr(r.work_key,7,36) AND e.event_id=substr(r.work_key,44,36) AND e.record<>'') AND NOT EXISTS(SELECT 1 FROM event_content ec WHERE ec.workspace=r.workspace AND ec.source=substr(r.work_key,7,36) AND ec.event_id=substr(r.work_key,44,36)) LIMIT ?3)",params![w,body,limit]).map_err(db_error)?;
            report.audit_rows=tx.execute("DELETE FROM audit WHERE rowid IN (SELECT rowid FROM audit WHERE workspace=?1 AND occurred_ms<=?2 ORDER BY occurred_ms LIMIT ?3)",params![w,audit,limit]).map_err(db_error)?;
            report.expired_rows+=tx.execute("DELETE FROM delivery_attempts WHERE rowid IN (SELECT a.rowid FROM retired_work r JOIN deliveries d ON d.workspace=r.workspace AND d.dispatch_key=r.work_key JOIN delivery_attempts a ON a.workspace=d.workspace AND a.delivery_id=d.delivery_id WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=1 LIMIT ?3)",params![w,dedup,limit]).map_err(db_error)?;
            report.expired_rows+=tx.execute("DELETE FROM deliveries WHERE rowid IN (SELECT d.rowid FROM retired_work r JOIN deliveries d ON d.workspace=r.workspace AND d.dispatch_key=r.work_key WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=1 AND NOT EXISTS(SELECT 1 FROM delivery_attempts a WHERE a.workspace=d.workspace AND a.delivery_id=d.delivery_id) LIMIT ?3)",params![w,dedup,limit]).map_err(db_error)?;
            // dispatches FK keeps receipts until every delivery is gone.
            report.expired_rows+=tx.execute("DELETE FROM dispatches WHERE rowid IN (SELECT d.rowid FROM retired_work r JOIN dispatches d ON d.workspace=r.workspace AND d.dispatch_key=r.work_key WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=1 AND NOT EXISTS(SELECT 1 FROM deliveries j WHERE j.workspace=d.workspace AND j.dispatch_key=d.dispatch_key) LIMIT ?3)",params![w,dedup,limit]).map_err(db_error)?;
            report.expired_rows+=tx.execute("DELETE FROM routing_tasks WHERE rowid IN (SELECT t.rowid FROM retired_work r JOIN routing_tasks t ON t.workspace=r.workspace AND t.source=substr(r.work_key,7,36) AND t.event_id=substr(r.work_key,44,36) WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=1 AND r.work_key LIKE 'event:%' AND NOT EXISTS(SELECT 1 FROM dispatches d WHERE d.workspace=r.workspace AND d.dispatch_key=r.work_key) LIMIT ?3)",params![w,dedup,limit]).map_err(db_error)?;
            report.expired_rows+=tx.execute("DELETE FROM events WHERE rowid IN (SELECT e.rowid FROM retired_work r JOIN events e ON e.workspace=r.workspace AND e.source=substr(r.work_key,7,36) AND e.event_id=substr(r.work_key,44,36) WHERE r.workspace=?1 AND r.retired_ms<=?2 AND r.payload_purged=1 AND r.work_key LIKE 'event:%' AND NOT EXISTS(SELECT 1 FROM routing_tasks t WHERE t.workspace=e.workspace AND t.source=e.source AND t.event_id=e.event_id) LIMIT ?3)",params![w,dedup,limit]).map_err(db_error)?;
            tx.execute("DELETE FROM retired_work WHERE rowid IN (SELECT r.rowid FROM retired_work r WHERE workspace=?1 AND retired_ms<=?2 AND payload_purged=1 AND NOT EXISTS(SELECT 1 FROM dispatches d WHERE d.workspace=r.workspace AND d.dispatch_key=r.work_key) AND NOT EXISTS(SELECT 1 FROM events e WHERE e.workspace=r.workspace AND e.source=substr(r.work_key,7,36) AND e.event_id=substr(r.work_key,44,36)) LIMIT ?3)",params![w,dedup,limit]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            // PASSIVE does not block readers or truncate an active WAL. Physical shrink is offline.
            (report.wal_busy,report.wal_frames,report.wal_checkpointed)=c.query_row("PRAGMA wal_checkpoint(PASSIVE)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_error)?;
            Ok(report)
        }).await
    }
}
