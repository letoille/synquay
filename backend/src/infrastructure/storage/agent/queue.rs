//! Ingress-to-inbox commits and bounded expiry maintenance.
use super::*;

impl SqliteStore {
    pub async fn agent_binding(&self) -> Result<Option<AgentBinding>, PortError> {
        self.run(|c| match binding(c) {
            Ok(b) => Ok(Some(b)),
            Err(PortError::NotFound) => Ok(None),
            Err(e) => Err(e),
        })
        .await
    }
    pub async fn bind_agent(&self, b: AgentBinding) -> Result<(), PortError> {
        self.run_critical(move |c| {
            if !b.config.valid() {
                return Err(PortError::InvalidInput {
                    code: "agent_config",
                });
            }
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;

            let s = encode(&b)?;

            let old: Option<String> = tx
                .query_row(
                    "SELECT record FROM agent_binding WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;

            if let Some(old) = old {
                if old != s {
                    return Err(PortError::Conflict);
                }
            } else {
                tx.execute(
                    "INSERT INTO agent_binding(singleton,record) VALUES(1,?1)",
                    [s],
                )
                .map_err(db_error)?;
            }
            tx.commit().map_err(|_| PortError::OutcomeUnknown)
        })
        .await
    }
    pub async fn expire_agent_jobs(&self) -> Result<(), PortError> {
        self.run_critical(|c| {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;

            let b = binding(&tx)?;

            sweep(&tx, &b)?;

            tx.commit().map_err(|_| PortError::OutcomeUnknown)
        })
        .await
    }
    pub async fn queue_agent_event(&self, id: Uuid) -> Result<(), PortError> {
        self.run_critical(move|c|{
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
let b=binding(&tx)?;
let w=b.source.workspace_id.to_string();

        if tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_jobs WHERE workspace=?1 AND job_id=?2)",params![w,id.to_string()],|r|r.get::<_,bool>(0)).map_err(db_error)? {return Ok(())}
        let count:u64=tx.query_row("SELECT job_count FROM agent_binding WHERE singleton=1",[],|r|r.get(0)).map_err(db_error)?;


        let (raw,received):(String,i64)=tx.query_row("SELECT e.record,e.received_ms FROM events e JOIN routing_tasks r USING(workspace,source,event_id) WHERE e.workspace=?1 AND e.source=?2 AND e.event_id=?3 AND r.state IN ('ready','retry_wait') AND r.resolution IS NULL",params![w,b.source.connection_id.to_string(),id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;

        let e:EventEnvelope=record::decode(&raw)?;
if !matches!(e.payload,EventPayload::Json(_)){return Err(PortError::Unsupported)}
        if let Some(command) = &b.config.command {
            match command.capture(&e.payload) {
                Some(captured) => { command.render(&e.payload, &captured).map_err(|_|PortError::InvalidInput{code:"command_mapping_failed"})?; }
                None => {
                    let work_key=key(&b,id);
                    tx.execute("INSERT INTO dispatches(workspace,dispatch_key,receipt) VALUES(?1,?2,'[]')",params![w,work_key]).map_err(db_error)?;
                    tx.execute("UPDATE routing_tasks SET state='completed',error_code=NULL WHERE workspace=?1 AND source=?2 AND event_id=?3",params![w,b.source.connection_id.to_string(),id.to_string()]).map_err(db_error)?;
                    capacity::release_completed(&tx,&w,&work_key)?;
                    record_audit(&tx,&b,b.executor,Uuid::new_v4(),true,id)?;
                    return tx.commit().map_err(|_|PortError::OutcomeUnknown);
                }
            }
        }
        if count>=MAX_JOBS{return Err(PortError::CapacityExceeded{reason:"agent_history_full"})}
        let deadline=received+b.config.deadline_seconds as i64*1000;

        tx.execute("INSERT INTO agent_jobs(workspace,job_id,source,profile,state,due_ms,deadline_ms) VALUES(?1,?2,?3,?4,'queued',?5,?6)",params![w,id.to_string(),b.source.connection_id.to_string(),b.config.profile_id.to_string(),received,deadline]).map_err(db_error)?;

        tx.execute("UPDATE routing_tasks SET state='completed',error_code=NULL WHERE workspace=?1 AND source=?2 AND event_id=?3",params![w,b.source.connection_id.to_string(),id.to_string()]).map_err(db_error)?;

        record_audit(&tx,&b,b.executor,Uuid::new_v4(),true,id)?;

        tx.commit().map_err(|_|PortError::OutcomeUnknown)
    }).await
    }
}
