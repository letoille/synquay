use super::{SqliteStore, db_error};
use crate::{
    application::ports::{
        PortError, PortFuture,
        checkpoints::{Checkpoint, SourceCheckpoints},
    },
    domain::connector::ConnectionScope,
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
pub(super) fn validate(partition: &str) -> Result<(), PortError> {
    if partition.is_empty() || partition.len() > 256 {
        Err(PortError::InvalidInput {
            code: "source_partition",
        })
    } else {
        Ok(())
    }
}
impl SourceCheckpoints for SqliteStore {
    fn activate(
        &self,
        scope: ConnectionScope,
        partition: String,
        expected: Option<u64>,
    ) -> PortFuture<'_, Checkpoint> {
        Box::pin(self.run_critical(move |c| {
            validate(&partition)?;
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let w=scope.workspace_id.to_string(); let s=scope.connection_id.to_string();
            let old: Option<(u64,u64,Option<String>)>=tx.query_row("SELECT generation,revision,cursor FROM source_checkpoints WHERE workspace=?1 AND source=?2 AND partition=?3",params![w,s,partition],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            if old.as_ref().map(|x|x.0)!=expected { return Err(PortError::Conflict); }
            let (generation,revision,cursor)=old.unwrap_or((0,0,None));
            let generation=generation.checked_add(1).filter(|v|*v<=i64::MAX as u64).ok_or(PortError::Conflict)?;
            tx.execute("INSERT INTO source_checkpoints(workspace,source,partition,generation,revision,cursor) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(workspace,source,partition) DO UPDATE SET generation=excluded.generation",params![w,s,partition,generation,revision,cursor]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(Checkpoint {partition,generation,revision,cursor})
        }))
    }
    fn checkpoint(
        &self,
        scope: ConnectionScope,
        partition: String,
    ) -> PortFuture<'_, Option<Checkpoint>> {
        Box::pin(self.run(move |c| {
            validate(&partition)?;
            c.query_row("SELECT generation,revision,cursor FROM source_checkpoints WHERE workspace=?1 AND source=?2 AND partition=?3",params![scope.workspace_id.to_string(),scope.connection_id.to_string(),partition],|r|Ok(Checkpoint{partition:partition.clone(),generation:r.get(0)?,revision:r.get(1)?,cursor:r.get(2)?})).optional().map_err(db_error)
        }))
    }
}

/// Same transaction as event/dedupe/outbox reservation. Never acknowledge before this commit.
pub(super) fn advance(
    tx: &rusqlite::Transaction<'_>,
    scope: ConnectionScope,
    checkpoint: &Checkpoint,
    event_id: uuid::Uuid,
) -> Result<(), PortError> {
    validate(&checkpoint.partition)?;
    if checkpoint
        .cursor
        .as_ref()
        .is_none_or(|s| s.is_empty() || s.len() > 4096)
        || checkpoint.revision >= i64::MAX as u64
    {
        return Err(PortError::InvalidInput {
            code: "source_cursor",
        });
    }
    let w = scope.workspace_id.to_string();
    let s = scope.connection_id.to_string();
    let old:Option<(u64,u64,Option<String>,Option<String>)>=tx.query_row("SELECT generation,revision,cursor,event_id FROM source_checkpoints WHERE workspace=?1 AND source=?2 AND partition=?3",params![w,s,checkpoint.partition],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
    let Some((generation, revision, cursor, stored_id)) = old else {
        return Err(PortError::Conflict);
    };
    if generation != checkpoint.generation {
        return Err(PortError::Conflict);
    }
    // Retrying an uncertain commit with precisely the same cursor is safe.
    if revision == checkpoint.revision + 1
        && cursor == checkpoint.cursor
        && stored_id == Some(event_id.to_string())
    {
        return Ok(());
    }
    if revision != checkpoint.revision {
        return Err(PortError::Conflict);
    }
    tx.execute("UPDATE source_checkpoints SET revision=revision+1,cursor=?4,event_id=?5 WHERE workspace=?1 AND source=?2 AND partition=?3",params![w,s,checkpoint.partition,checkpoint.cursor,event_id.to_string()]).map_err(db_error)?;
    Ok(())
}
