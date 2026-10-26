use super::{SqliteStore, db_error, encode, millis};
use crate::{
    application::ports::{PortFuture, audit::AuditLog},
    domain::audit::AuditRecord,
};
use rusqlite::params;

impl AuditLog for SqliteStore {
    fn append(&self, record: AuditRecord) -> PortFuture<'_, ()> {
        Box::pin(self.run_with_gate(
            matches!(
                record.phase,
                crate::domain::audit::AuditPhase::Succeeded
                    | crate::domain::audit::AuditPhase::Failed
            ),
            move |c| {
                c.execute(
                    "INSERT INTO audit(record_id,workspace,occurred_ms,record) VALUES(?1,?2,?3,?4)",
                    params![
                        record.record_id.to_string(),
                        record.workspace_id.to_string(),
                        millis(record.occurred_at)?,
                        encode(&record)?
                    ],
                )
                .map_err(db_error)?;
                Ok(())
            },
        ))
    }
}
