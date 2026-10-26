use super::{SqliteStore, capacity, db_error, decode, encode, millis, record};
use crate::{
    application::ports::{PortError, PortFuture, delivery::DeliveryOutbox},
    domain::delivery::*,
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn dispatch_key(key: &DispatchKey) -> String {
    match key {
        DispatchKey::Event { scope, event_id } => {
            format!("event:{}:{}", scope.connection_id, event_id)
        }
        DispatchKey::Reply { reply_id, .. } => format!("reply:{reply_id}"),
    }
}
impl DeliveryOutbox for SqliteStore {
    fn enqueue(&self, plan: DispatchPlan) -> PortFuture<'_, DispatchReceipt> {
        let path = self.path.clone();
        Box::pin(self.run(move |c| {
            if plan.deliveries.len() > capacity::MAX_DISPATCH_TARGETS { return Err(PortError::InvalidInput { code:"dispatch_target_limit" }); }
            let workspace = plan.workspace_id;
            let key_workspace = match &plan.key { DispatchKey::Event {scope,..}=>scope.workspace_id, DispatchKey::Reply {workspace_id,..}=>*workspace_id };
            if workspace != key_workspace || plan.deliveries.iter().any(|j| j.destination.scope.workspace_id != workspace) { return Err(PortError::ScopeMismatch); }
            let mut ids = std::collections::HashSet::new();
            if plan.deliveries.iter().any(|j| !ids.insert(j.delivery_id)) { return Err(PortError::Conflict); }
            // Semantic reply fingerprint excludes generated IDs and destination replay hints.
            let fingerprint = if matches!(plan.key, DispatchKey::Reply {..}) {
                Some(capacity::encode_bounded(&plan.deliveries.iter().map(|j| (&j.destination.id,&j.destination.scope,&j.message,&j.causation_id)).collect::<Vec<_>>())?)
            } else { None };
            let key = dispatch_key(&plan.key); let w=workspace.to_string();
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let existing: Option<(String,Option<String>)> = tx.query_row("SELECT receipt,reply_fingerprint FROM dispatches WHERE workspace=?1 AND dispatch_key=?2", params![w,key], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
            if let Some((receipt,previous))=existing {
                if previous != fingerprint { return Err(PortError::Conflict); }
                return Ok(DispatchReceipt {delivery_ids:decode(&receipt)?,duplicate:true});
            }
            if let DispatchKey::Event {scope,event_id} = &plan.key {
                let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM events e JOIN routing_tasks r USING(workspace,source,event_id) WHERE e.workspace=?1 AND e.source=?2 AND e.event_id=?3 AND r.resolution IS NULL)",params![w,scope.connection_id.to_string(),event_id.to_string()],|r|r.get(0)).map_err(db_error)?;
                if !exists { return Err(PortError::NotFound); }
            }
            for job in &plan.deliveries {
                for part in &job.message.parts {
                    if let crate::domain::message::ContentPart::StoredContent(reference)=part {
                        let DispatchKey::Event{scope,event_id}=&plan.key else {return Err(PortError::Unsupported);};
                        let linked:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM event_content WHERE workspace=?1 AND source=?2 AND event_id=?3 AND content_id=?4)",params![w,scope.connection_id.to_string(),event_id.to_string(),reference.id.to_string()],|r|r.get(0)).map_err(db_error)?;
                        if !linked || reference.scope!=*scope {return Err(PortError::PermissionDenied);}
                    }
                }
            }
            let config_revision:Option<u64>=match &plan.key {
                DispatchKey::Event{scope,event_id}=>tx.query_row("SELECT config_revision FROM events WHERE workspace=?1 AND source=?2 AND event_id=?3",params![w,scope.connection_id.to_string(),event_id.to_string()],|r|r.get(0)).map_err(db_error)?,
                DispatchKey::Reply{..}=>tx.query_row("SELECT revision FROM active_release WHERE singleton=1",[],|r|r.get(0)).optional().map_err(db_error)?,
            };
            let records:Vec<_> = plan.deliveries.iter().map(record::encode).collect::<Result<_,_>>()?;
            if matches!(plan.key,DispatchKey::Reply {..}) && !records.is_empty()
                && let Err(error) = capacity::reserve(&tx,&path,&w,&key,records.iter().map(String::len).sum(),millis(SystemTime::now())?) {
                    if matches!(error,PortError::CapacityExceeded {..}) { tx.commit().map_err(|_|PortError::OutcomeUnknown)?; }
                    return Err(error);
            }
            let delivery_ids:Vec<_>=plan.deliveries.iter().map(|j|j.delivery_id).collect();
            tx.execute("INSERT INTO dispatches(workspace,dispatch_key,receipt,reply_fingerprint) VALUES(?1,?2,?3,?4)",params![w,key,encode(&delivery_ids)?,fingerprint]).map_err(db_error)?;
            for (job,record) in plan.deliveries.into_iter().zip(records) {
                tx.execute("INSERT INTO deliveries(workspace,delivery_id,dispatch_key,record,state,due_ms,target_id,config_revision,route_rule_id) VALUES(?1,?2,?3,?4,'pending',?5,?6,?7,?8)",params![w,job.delivery_id.to_string(),key,record,millis(SystemTime::now())?,job.destination.id.to_string(),config_revision,job.route_rule.map(|(id,_)|id.to_string())]).map_err(db_error)?;
            }
            if let DispatchKey::Event {scope,event_id} = &plan.key {
                tx.execute("UPDATE routing_tasks SET state='completed',error_code=NULL WHERE workspace=?1 AND source=?2 AND event_id=?3", params![w,scope.connection_id.to_string(),event_id.to_string()]).map_err(db_error)?;
            }
            capacity::release_completed(&tx,&w,&key)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(DispatchReceipt {delivery_ids,duplicate:false})
        }))
    }
    fn claim(
        &self,
        workspace_id: Uuid,
        now: SystemTime,
        lease_for: Duration,
    ) -> PortFuture<'_, Option<LeasedDelivery>> {
        self.claim_filtered(workspace_id, None, false, now, lease_for)
    }
    fn claim_target(
        &self,
        workspace_id: Uuid,
        target_id: Uuid,
        strict_order: bool,
        now: SystemTime,
        lease_for: Duration,
    ) -> PortFuture<'_, Option<LeasedDelivery>> {
        self.claim_filtered(workspace_id, Some(target_id), strict_order, now, lease_for)
    }

    fn mark_sending(&self, token: LeaseToken) -> PortFuture<'_, ()> {
        Box::pin(self.run_critical(move|c|{
       let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
       let now=millis(SystemTime::now())?;
       let count=tx.execute("UPDATE deliveries SET state='sending' WHERE workspace=?1 AND delivery_id=?2 AND lease_id=?3 AND state='leased' AND lease_until_ms=?4 AND lease_until_ms>?5",params![token.workspace_id.to_string(),token.delivery_id.to_string(),token.lease_id.to_string(),millis(token.expires_at)?,now]).map_err(db_error)?;
       if count!=1{return Err(PortError::Conflict);}
       tx.execute("UPDATE delivery_attempts SET started_ms=?4,outcome='sending' WHERE workspace=?1 AND delivery_id=?2 AND lease_id=?3",params![token.workspace_id.to_string(),token.delivery_id.to_string(),token.lease_id.to_string(),now]).map_err(db_error)?;
       tx.commit().map_err(|_|PortError::OutcomeUnknown)
      }))
    }
    fn finish(&self, token: LeaseToken, outcome: DeliveryOutcome) -> PortFuture<'_, ()> {
        Box::pin(self.run_critical(move |c| {
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let now=SystemTime::now();
            let (state,due,result,requires_send)=match outcome {
                DeliveryOutcome::Submitted(receipt)=> {
                    if receipt.delivery_id != token.delivery_id {return Err(PortError::Conflict);}
                    ("submitted",now,Some(encode(&receipt)?),true)
                },
                DeliveryOutcome::Deferred(delay)=>("pending",now.checked_add(delay).ok_or(PortError::Conflict)?,None,false),
                DeliveryOutcome::RetryAfter(delay)=>("pending",now.checked_add(delay).ok_or(PortError::Conflict)?,None,true),
                DeliveryOutcome::Failed{code}=>("failed",now,Some(encode(&code)?),false),
                DeliveryOutcome::Reconcile=>("reconcile",now,None,false),
            };
            let w=token.workspace_id.to_string();
            let count=tx.execute("UPDATE deliveries SET state=?6,due_ms=?7,result=?8,lease_id=NULL,lease_until_ms=NULL WHERE workspace=?1 AND delivery_id=?2 AND lease_id=?3 AND lease_until_ms=?4 AND lease_until_ms>?5 AND (state='sending' OR (state='leased' AND ?9=0))",params![w,token.delivery_id.to_string(),token.lease_id.to_string(),millis(token.expires_at)?,millis(now)?,state,millis(due)?,result,requires_send]).map_err(db_error)?;
            if count!=1 { return Err(PortError::Conflict); }
            tx.execute("UPDATE delivery_attempts SET finished_ms=?4,outcome=?5 WHERE workspace=?1 AND delivery_id=?2 AND lease_id=?3",params![w,token.delivery_id.to_string(),token.lease_id.to_string(),millis(now)?,state]).map_err(db_error)?;
            let key:String=tx.query_row("SELECT dispatch_key FROM deliveries WHERE workspace=?1 AND delivery_id=?2",params![w,token.delivery_id.to_string()],|r|r.get(0)).map_err(db_error)?;
            capacity::release_completed(&tx,&w,&key)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)
        }))
    }
}

impl SqliteStore {
    fn claim_filtered(
        &self,
        workspace_id: Uuid,
        target: Option<Uuid>,
        strict_order: bool,
        now: SystemTime,
        lease_for: Duration,
    ) -> PortFuture<'_, Option<LeasedDelivery>> {
        Box::pin(self.run(move |c| {
            if lease_for.is_zero() || lease_for > Duration::from_secs(3600) { return Err(PortError::InvalidInput{code:"lease_duration"}); }
            let now=millis(now)?; let until=now.checked_add(lease_for.as_millis() as i64).ok_or(PortError::InvalidInput{code:"lease_duration"})?;
            let w=workspace_id.to_string(); let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            tx.execute("UPDATE delivery_attempts SET outcome='lease_expired',finished_ms=?2 WHERE workspace=?1 AND finished_ms IS NULL AND EXISTS(SELECT 1 FROM deliveries d WHERE d.workspace=delivery_attempts.workspace AND d.delivery_id=delivery_attempts.delivery_id AND d.lease_id=delivery_attempts.lease_id AND d.lease_until_ms<=?2)",params![w,now]).map_err(db_error)?;
            // Conservative recovery: even configured idempotent Sending jobs require reconciliation after a crash.
            tx.execute("UPDATE deliveries SET state='reconcile',lease_id=NULL,lease_until_ms=NULL WHERE workspace=?1 AND state='sending' AND lease_until_ms<=?2",params![w,now]).map_err(db_error)?;
            tx.execute("UPDATE deliveries SET state='pending',lease_id=NULL,lease_until_ms=NULL WHERE workspace=?1 AND state='leased' AND lease_until_ms<=?2",params![w,now]).map_err(db_error)?;
            let query=if target.is_some() {"SELECT d.delivery_id,d.record,d.attempt FROM deliveries d WHERE d.workspace=?1 AND d.state='pending' AND d.resolution IS NULL AND d.due_ms<=?2 AND d.target_id=?3 AND (?4=0 OR NOT EXISTS(SELECT 1 FROM deliveries p WHERE p.workspace=d.workspace AND p.target_id=d.target_id AND p.rowid<d.rowid AND p.state<>'submitted' AND p.resolution IS NULL)) ORDER BY d.due_ms,d.rowid LIMIT 1"} else {"SELECT d.delivery_id,d.record,d.attempt FROM deliveries d WHERE d.workspace=?1 AND d.state='pending' AND d.resolution IS NULL AND d.due_ms<=?2 AND ?3 IS NULL AND (?4=0 OR NOT EXISTS(SELECT 1 FROM deliveries p WHERE p.workspace=d.workspace AND p.target_id=d.target_id AND p.rowid<d.rowid AND p.state<>'submitted' AND p.resolution IS NULL)) ORDER BY d.due_ms,d.rowid LIMIT 1"};
            let candidate:Option<(String,String,u32)>=tx.query_row(query,params![w,now,target.map(|x|x.to_string()),strict_order],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            let result=if let Some((id,record,attempt))=candidate {
                let job:PendingDelivery=record::decode(&record)?;
                if job.destination.scope.workspace_id != workspace_id || job.delivery_id.to_string()!=id { return Err(PortError::ScopeMismatch); }
                let lease_id=Uuid::new_v4(); let attempt=attempt.checked_add(1).ok_or(PortError::Conflict)?;
                tx.execute("UPDATE deliveries SET state='leased',lease_id=?3,lease_until_ms=?4,attempt=?5 WHERE workspace=?1 AND delivery_id=?2",params![w,id,lease_id.to_string(),until,attempt]).map_err(db_error)?;
                tx.execute("INSERT INTO delivery_attempts(workspace,delivery_id,attempt,lease_id,claimed_ms,outcome) VALUES(?1,?2,?3,?4,?5,'leased')",params![w,id,attempt,lease_id.to_string(),now]).map_err(db_error)?;
                Some(LeasedDelivery {token:LeaseToken {workspace_id,delivery_id:job.delivery_id,lease_id,expires_at:UNIX_EPOCH+Duration::from_millis(until as u64)},job,attempt})
            } else {None};
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?; Ok(result)
        }))
    }
}
