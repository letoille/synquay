use super::{SqliteStore, db_error, millis};
use crate::{
    application::ports::PortError,
    domain::{connector::ConnectionScope, content::ContentRef},
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use std::time::SystemTime;
use uuid::Uuid;
impl SqliteStore {
    pub(crate) async fn bind_content_directory(&self, directory: String) -> Result<(), PortError> {
        self.run_critical(move |c| {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            tx.execute(
                "INSERT OR IGNORE INTO content_binding VALUES(1,?1)",
                [&directory],
            )
            .map_err(db_error)?;
            let old: String = tx
                .query_row(
                    "SELECT directory FROM content_binding WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if old != directory {
                return Err(PortError::InvalidInput {
                    code: "content_directory_changed",
                });
            }
            tx.commit().map_err(|_| PortError::OutcomeUnknown)
        })
        .await
    }

    pub(crate) async fn content_reference(
        &self,
        scope: ConnectionScope,
        id: Uuid,
    ) -> Result<ContentRef, PortError> {
        self.run(move|c| c.query_row("SELECT size,digest,media_type FROM content_objects WHERE id=?1 AND workspace=?2 AND source=?3 AND state IN ('staged','available')",params![id.to_string(),scope.workspace_id.to_string(),scope.connection_id.to_string()],|r|Ok(ContentRef{id,scope,size:r.get(0)?,sha256:r.get(1)?,media_type:r.get(2)?})).optional().map_err(db_error)?.ok_or(PortError::NotFound)).await
    }

    pub(crate) async fn abort_content(&self, id: Uuid) -> Result<bool, PortError> {
        self.run_critical(move|c| Ok(c.execute("UPDATE content_objects SET state='deleting' WHERE id=?1 AND state IN ('uploading','staged') AND NOT EXISTS(SELECT 1 FROM event_content WHERE content_id=?1)",[id.to_string()]).map_err(db_error)?==1)).await
    }

    pub(crate) async fn reserve_content(
        &self,
        reference: ContentRef,
        max_bytes: u64,
        max_objects: u64,
        path: std::path::PathBuf,
    ) -> Result<(), PortError> {
        let ContentRef {
            id,
            scope,
            size,
            media_type: media,
            ..
        } = reference;
        self.run(move|c| {
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            // The quota is global to this store, including in-progress and orphan uploads.
            let (bytes,objects):(u64,u64)=tx.query_row("SELECT bytes,items FROM content_usage WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
            if objects>=max_objects || bytes.saturating_add(size)>max_bytes {return Err(PortError::CapacityExceeded{reason:"content_quota"});}
            let in_flight:u64=tx.query_row("SELECT COALESCE(SUM(size),0) FROM content_objects WHERE state='uploading'",[],|r|r.get(0)).map_err(db_error)?;
            if super::capacity::available_space(&path)? < size.saturating_add(in_flight).saturating_add(256*1024*1024) {return Err(PortError::CapacityExceeded{reason:"disk_low"});}
            tx.execute("INSERT INTO content_objects(id,workspace,source,size,media_type,state,created_ms) VALUES(?1,?2,?3,?4,?5,'uploading',?6)",params![id.to_string(),scope.workspace_id.to_string(),scope.connection_id.to_string(),size,media,millis(SystemTime::now())?]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)
        }).await
    }
    pub(crate) async fn stage_content(&self, reference: ContentRef) -> Result<(), PortError> {
        self.run_critical(move|c| {
            let n=c.execute("UPDATE content_objects SET digest=?2,state='staged' WHERE id=?1 AND state='uploading'",params![reference.id.to_string(),reference.sha256]).map_err(db_error)?;
            if n==1 { Ok(()) } else { Err(PortError::Conflict) }
        }).await
    }
    pub(crate) async fn check_content(&self, reference: ContentRef) -> Result<(), PortError> {
        self.run(move |c| check(c, &reference)).await
    }
    pub(crate) async fn content_garbage(
        &self,
        before_ms: i64,
        limit: usize,
    ) -> Result<Vec<Uuid>, PortError> {
        self.run(move|c| {
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let mut ids:Vec<String>={let mut q=tx.prepare("SELECT id FROM content_objects WHERE state='deleting' ORDER BY created_ms LIMIT ?1").map_err(db_error)?;
                q.query_map([limit.min(100)],|r|r.get(0)).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?};
            let remaining=limit.min(100)-ids.len();
            {let mut q=tx.prepare("SELECT id FROM content_objects WHERE state IN ('uploading','staged') AND created_ms<?1 ORDER BY created_ms LIMIT ?2").map_err(db_error)?;
                let more=q.query_map(params![before_ms,remaining],|r|r.get::<_,String>(0)).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;ids.extend(more);}
            for id in &ids {tx.execute("UPDATE content_objects SET state='deleting' WHERE id=?1",[id]).map_err(db_error)?;}
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            ids.into_iter().map(|s|Uuid::parse_str(&s).map_err(|_|PortError::Failed{code:"stored_content_id"})).collect()
        }).await
    }
    pub(crate) async fn forget_content(&self, id: Uuid) -> Result<(), PortError> {
        self.run(move |c| {
            c.execute(
                "DELETE FROM content_objects WHERE id=?1 AND state='deleting'",
                [id.to_string()],
            )
            .map_err(db_error)?;
            Ok(())
        })
        .await
    }
}
fn check(c: &rusqlite::Connection, r: &ContentRef) -> Result<(), PortError> {
    let exists:Option<bool>=c.query_row("SELECT 1 FROM content_objects WHERE id=?1 AND workspace=?2 AND source=?3 AND size=?4 AND digest=?5 AND media_type=?6 AND state IN ('staged','available')",params![r.id.to_string(),r.scope.workspace_id.to_string(),r.scope.connection_id.to_string(),r.size,r.sha256,r.media_type],|r|r.get(0)).optional().map_err(db_error)?;
    if exists == Some(true) {
        Ok(())
    } else {
        Err(PortError::NotFound)
    }
}
pub(super) fn attach(
    tx: &Transaction<'_>,
    scope: ConnectionScope,
    event_id: Uuid,
    r: &ContentRef,
) -> Result<(), PortError> {
    if r.scope != scope {
        return Err(PortError::ScopeMismatch);
    }
    check(tx, r)?;
    tx.execute(
        "INSERT INTO event_content VALUES(?1,?2,?3,?4)",
        params![
            scope.workspace_id.to_string(),
            scope.connection_id.to_string(),
            event_id.to_string(),
            r.id.to_string()
        ],
    )
    .map_err(db_error)?;
    tx.execute(
        "UPDATE content_objects SET state='available' WHERE id=?1",
        [r.id.to_string()],
    )
    .map_err(db_error)?;
    Ok(())
}
