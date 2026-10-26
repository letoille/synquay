use super::{SqliteStore, capacity, db_error, millis, record};
use crate::{
    application::ports::{PortError, PortFuture, events::EventStore, routing::EventReader},
    domain::{connector::ConnectionScope, event::*},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use uuid::Uuid;

impl EventStore for SqliteStore {
    fn append(&self, event: EventEnvelope) -> PortFuture<'_, IngestReceipt> {
        self.append_inner(event, None)
    }
    fn append_checkpoint(
        &self,
        event: EventEnvelope,
        checkpoint: crate::application::ports::checkpoints::Checkpoint,
    ) -> PortFuture<'_, IngestReceipt> {
        self.append_inner(event, Some(checkpoint))
    }
}
impl SqliteStore {
    fn append_inner(
        &self,
        event: EventEnvelope,
        checkpoint: Option<crate::application::ports::checkpoints::Checkpoint>,
    ) -> PortFuture<'_, IngestReceipt> {
        let path = self.path.clone();
        Box::pin(self.run(move |c| {
            if checkpoint.is_some() && event.source_event_id.as_ref().is_none_or(|s|s.is_empty() || s.len()>256) {return Err(PortError::InvalidInput{code:"checkpoint_requires_source_id"});}
            let fingerprint = fingerprint(&event)?;
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let w = event.scope.workspace_id.to_string(); let s = event.scope.connection_id.to_string();
            if let Some(ref source_id) = event.source_event_id {
                let existing: Option<(String, Option<String>, String)> = tx.query_row("SELECT event_id,fingerprint,record FROM events WHERE workspace=?1 AND source=?2 AND source_event_id=?3",
                    params![w,s,source_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
                if let Some((id, old_fingerprint, old_record)) = existing {
                    let old = match old_fingerprint {
                        Some(value)=>value,
                        None=>self::fingerprint(&record::decode::<EventEnvelope>(&old_record).map_err(|_|PortError::Conflict)?)?,
                    };
                    if old!=fingerprint {return Err(PortError::Conflict)}
                    tx.execute("UPDATE events SET fingerprint=?4 WHERE workspace=?1 AND source=?2 AND event_id=?3 AND fingerprint IS NULL",params![w,s,id,fingerprint]).map_err(db_error)?;
                    if let Some(cp)=&checkpoint { super::checkpoints::advance(&tx,event.scope,cp,Uuid::parse_str(&id).map_err(|_|PortError::Conflict)?)?; }
                    tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
                    return Ok(IngestReceipt { event_id: Uuid::parse_str(&id).map_err(|_| PortError::Failed {code:"stored_id"})?, disposition: IngestDisposition::Duplicate });
                }
            }
            let record = record::encode(&event)?;
            let key = format!("event:{s}:{}",event.event_id);
            if let Err(error) = capacity::reserve(&tx,&path,&w,&key,record.len() + if tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_binding)",[],|r|r.get::<_,bool>(0)).map_err(db_error)? {super::agent::REPLY_BUDGET} else {0},millis(event.received_at)?) {
                // Persist the high-watermark latch, without accepting this event.
                if matches!(error,PortError::CapacityExceeded {..}) { tx.commit().map_err(|_| PortError::OutcomeUnknown)?; }
                return Err(error);
            }
            if let Some(cp)=&checkpoint { super::checkpoints::advance(&tx,event.scope,cp,event.event_id)?; }
            tx.execute("INSERT INTO events(workspace,source,event_id,source_event_id,received_ms,record,config_revision,fingerprint) VALUES(?1,?2,?3,?4,?5,?6,(SELECT revision FROM active_release WHERE singleton=1),?7)",
                params![w,s,event.event_id.to_string(),event.source_event_id,millis(event.received_at)?,record,fingerprint]).map_err(db_error)?;
            super::conversations::observe(&tx,&event)?;
            if let EventPayload::Content(reference)=&event.payload { super::content::attach(&tx,event.scope,event.event_id,reference)?; }
            tx.execute("INSERT INTO routing_tasks(workspace,source,event_id,state,due_ms) VALUES(?1,?2,?3,'ready',?4)",params![w,s,event.event_id.to_string(),millis(event.received_at)?]).map_err(db_error)?;
            tx.commit().map_err(|_| PortError::OutcomeUnknown)?;
            Ok(IngestReceipt {event_id: event.event_id, disposition: IngestDisposition::Stored})
        }))
    }
}
impl EventReader for SqliteStore {
    fn load(&self, scope: ConnectionScope, event_id: Uuid) -> PortFuture<'_, EventEnvelope> {
        Box::pin(self.run(move |c| {
            let record: String = c
                .query_row(
                    "SELECT record FROM events WHERE workspace=?1 AND source=?2 AND event_id=?3",
                    params![
                        scope.workspace_id.to_string(),
                        scope.connection_id.to_string(),
                        event_id.to_string()
                    ],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?
                .ok_or(PortError::NotFound)?;
            record::decode(&record)
        }))
    }
    fn unrouted(
        &self,
        workspace_id: Uuid,
        limit: usize,
    ) -> PortFuture<'_, Vec<(ConnectionScope, Uuid)>> {
        Box::pin(self.run(move |c| {
            let mut query = c.prepare("SELECT source,event_id FROM routing_tasks WHERE workspace=?1 AND state IN ('ready','retry_wait') AND due_ms<=?3 ORDER BY due_ms,event_id LIMIT ?2").map_err(db_error)?;
            query.query_map(params![workspace_id.to_string(),limit.min(1000) as i64,millis(std::time::SystemTime::now())?], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(db_error)?
                .map(|row| { let (source,id) = row.map_err(db_error)?;
                    let parse = |s: &str| Uuid::parse_str(s).map_err(|_| PortError::Failed {code:"stored_id"});
                    Ok((ConnectionScope {workspace_id,connection_id:parse(&source)?},parse(&id)?)) }).collect()
        }))
    }
}

/// Canonical business intent only: generated event IDs, receipt times and trace IDs change on retry.
/// Hash through a writer to avoid allocating a second payload-sized buffer.
fn fingerprint(event: &EventEnvelope) -> Result<String, PortError> {
    struct DigestWriter(ring::digest::Context, usize);
    impl std::io::Write for DigestWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > capacity::MAX_RECORD_BYTES - self.1 {
                return Err(std::io::Error::other("fingerprint size limit"));
            }
            self.1 += bytes.len();
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = DigestWriter(ring::digest::Context::new(&ring::digest::SHA256), 0);
    serde_json::to_writer(
        &mut writer,
        &(
            &event.event_type,
            event.schema_version,
            &event.occurred_at,
            &event.payload,
        ),
    )
    .map_err(|e| {
        if e.is_io() {
            PortError::PayloadTooLarge
        } else {
            PortError::InvalidInput {
                code: "event_fingerprint",
            }
        }
    })?;
    if let Some(context) = &event.route_context {
        serde_json::to_writer(&mut writer, context).map_err(|_| PortError::PayloadTooLarge)?;
    }
    Ok(writer
        .0
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
