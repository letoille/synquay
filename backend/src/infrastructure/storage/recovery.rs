use super::{SqliteStore, capacity, db_error, decode, encode, millis, record};
use crate::{
    application::ports::{PortError, PortFuture, recovery::RecoveryStore},
    domain::{delivery::PendingDelivery, recovery::*},
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use std::time::SystemTime;
use uuid::Uuid;

impl RecoveryStore for SqliteStore {
    fn recover(
        &self,
        w: Uuid,
        principal: Uuid,
        cmd: RecoveryCommand,
    ) -> PortFuture<'_, RecoveryReceipt> {
        Box::pin(self.run_critical(move |c|{
   if principal.is_nil() || cmd.command_id.is_nil() || cmd.reason.trim().is_empty() || cmd.reason.len()>1024 || cmd.expected_state.len()>32 || cmd.evidence.as_ref().is_some_and(|x| x.trim().is_empty()||x.len()>2048) {
    return Err(PortError::InvalidInput{code:"recovery_command"});
   }
   let fingerprint=encode(&(principal,&cmd))?;
   let w=w.to_string();let now=millis(SystemTime::now())?;
   let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
   let previous:Option<(String,String)>=tx.query_row("SELECT command,receipt FROM recovery_actions WHERE workspace=?1 AND command_id=?2",params![w,cmd.command_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
   if let Some((old,receipt))=previous {return if old==fingerprint {decode(&receipt)} else {Err(PortError::Conflict)};}
   let mut receipt=RecoveryReceipt{command_id:cmd.command_id,new_delivery_id:None};
   match &cmd.target {
    RecoveryTarget::Delivery{delivery_id}=>{
     let row:Option<(String,Option<String>,String,String,u64)>=tx.query_row("SELECT state,resolution,dispatch_key,record,recovery_revision FROM deliveries WHERE workspace=?1 AND delivery_id=?2",params![w,delivery_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db_error)?;
     let (state,resolved,key,record,revision)=row.ok_or(PortError::NotFound)?;
     if revision!=cmd.expected_revision || state!=cmd.expected_state || resolved.is_some() || !matches!(state.as_str(),"failed"|"reconcile") {return Err(PortError::Conflict);}
     let resolution=match cmd.action {
      RecoveryAction::Abandon=>"abandoned",
      RecoveryAction::ConfirmSubmitted=>{
       if state!="reconcile" || cmd.evidence.is_none() {return Err(PortError::InvalidInput{code:"submission_evidence_required"});}
       "confirmed_submitted"
      },
      RecoveryAction::Replay=>{
       if state=="reconcile" && (!cmd.confirmed_not_submitted || cmd.evidence.is_none()) {return Err(PortError::InvalidInput{code:"non_submission_evidence_required"});}
       let mut job:PendingDelivery=record::decode(&record)?;job.delivery_id=Uuid::new_v4();
       // This is a new run of the same logical work, preserving its pending reservation.
       let new_record=record::encode(&job)?;
       tx.execute("INSERT INTO deliveries(workspace,delivery_id,dispatch_key,record,state,due_ms,target_id,parent_delivery_id,config_revision) VALUES(?1,?2,?3,?4,'pending',?5,?6,?7,(SELECT config_revision FROM deliveries WHERE workspace=?1 AND delivery_id=?7))",params![w,job.delivery_id.to_string(),key,new_record,now,job.destination.id.to_string(),delivery_id.to_string()]).map_err(db_error)?;
       receipt.new_delivery_id=Some(job.delivery_id);"superseded"
      }
     };
     tx.execute("UPDATE deliveries SET resolution=?3,recovery_revision=recovery_revision+1 WHERE workspace=?1 AND delivery_id=?2",params![w,delivery_id.to_string(),resolution]).map_err(db_error)?;
     capacity::release_completed(&tx,&w,&key)?;
    },
    RecoveryTarget::Route{source_id,event_id}=>{
     let row:Option<(String,Option<String>,u64)>=tx.query_row("SELECT state,resolution,recovery_revision FROM routing_tasks WHERE workspace=?1 AND source=?2 AND event_id=?3",params![w,source_id.to_string(),event_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
     let (state,resolution,revision)=row.ok_or(PortError::NotFound)?;
     if revision!=cmd.expected_revision || state!="blocked" || state!=cmd.expected_state || resolution.is_some(){return Err(PortError::Conflict);}
     match cmd.action {
      RecoveryAction::Replay=>{tx.execute("UPDATE routing_tasks SET recovery_revision=recovery_revision+1,state='ready',attempt=0,due_ms=?4,error_code=NULL WHERE workspace=?1 AND source=?2 AND event_id=?3",params![w,source_id.to_string(),event_id.to_string(),now]).map_err(db_error)?;},
      RecoveryAction::Abandon=>{
       tx.execute("UPDATE routing_tasks SET recovery_revision=recovery_revision+1,resolution='abandoned' WHERE workspace=?1 AND source=?2 AND event_id=?3",params![w,source_id.to_string(),event_id.to_string()]).map_err(db_error)?;
       let key=format!("event:{source_id}:{event_id}");
       tx.execute("DELETE FROM pending_work WHERE workspace=?1 AND work_key=?2",params![w,key]).map_err(db_error)?;
       tx.execute("INSERT OR IGNORE INTO retired_work(workspace,work_key,retired_ms) VALUES(?1,?2,?3)",params![w,key,now]).map_err(db_error)?;
      },
      RecoveryAction::ConfirmSubmitted=>return Err(PortError::InvalidInput{code:"route_not_delivery"})
     }
    }
   }
   // Recovery record is the audit fact; committed with state and capacity, never separately.
   let target_key=match &cmd.target {RecoveryTarget::Delivery{delivery_id}=>format!("delivery:{delivery_id}"),RecoveryTarget::Route{source_id,event_id}=>format!("route:{source_id}:{event_id}")};
   tx.execute("INSERT INTO recovery_actions VALUES(?1,?2,?3,?4,?5,?6,?7)",params![w,cmd.command_id.to_string(),principal.to_string(),now,fingerprint,encode(&receipt)?,target_key]).map_err(db_error)?;
   tx.commit().map_err(|_|PortError::OutcomeUnknown)?;Ok(receipt)
  }))
    }
}

impl SqliteStore {
    pub async fn inspect_recovery(
        &self,
        workspace: Uuid,
        target: RecoveryTarget,
    ) -> Result<serde_json::Value, PortError> {
        self.run(move|c|{
   let tx=c.transaction().map_err(db_error)?;let w=workspace.to_string();let mut attempts=Vec::new();
   let (key,summary)=match target {
    RecoveryTarget::Delivery{delivery_id}=>{
     let id=delivery_id.to_string();
     let row=tx.query_row("SELECT target_id,state,resolution,parent_delivery_id,attempt,recovery_revision FROM deliveries WHERE workspace=?1 AND delivery_id=?2",params![w,id],|r|Ok(serde_json::json!({"delivery_id":id,"target_id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"resolution":r.get::<_,Option<String>>(2)?,"parent_delivery_id":r.get::<_,Option<String>>(3)?,"attempt":r.get::<_,u64>(4)?,"revision":r.get::<_,u64>(5)?}))).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
     let mut q=tx.prepare("SELECT attempt,claimed_ms,started_ms,finished_ms,outcome FROM delivery_attempts WHERE workspace=?1 AND delivery_id=?2 ORDER BY attempt DESC LIMIT 100").map_err(db_error)?;
     for r in q.query_map(params![w,id],|r|Ok(serde_json::json!({"attempt":r.get::<_,u64>(0)?,"claimed_ms":r.get::<_,i64>(1)?,"started_ms":r.get::<_,Option<i64>>(2)?,"finished_ms":r.get::<_,Option<i64>>(3)?,"outcome":r.get::<_,String>(4)?}))).map_err(db_error)?{attempts.push(r.map_err(db_error)?);}
     (format!("delivery:{id}"),row)
    },
    RecoveryTarget::Route{source_id,event_id}=>{
     let row=tx.query_row("SELECT state,resolution,attempt,recovery_revision,error_code FROM routing_tasks WHERE workspace=?1 AND source=?2 AND event_id=?3",params![w,source_id.to_string(),event_id.to_string()],|r|Ok(serde_json::json!({"source_id":source_id,"event_id":event_id,"state":r.get::<_,String>(0)?,"resolution":r.get::<_,Option<String>>(1)?,"attempt":r.get::<_,u64>(2)?,"revision":r.get::<_,u64>(3)?,"error_code":r.get::<_,Option<String>>(4)?}))).optional().map_err(db_error)?.ok_or(PortError::NotFound)?;
     (format!("route:{source_id}:{event_id}"),row)
    }
   };
   let mut history=Vec::new();let mut q=tx.prepare("SELECT command_id,principal,occurred_ms,command,receipt FROM recovery_actions WHERE workspace=?1 AND target_key=?2 ORDER BY occurred_ms DESC LIMIT 100").map_err(db_error)?;
   for r in q.query_map(params![w,key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))).map_err(db_error)?{let(id,principal,at,command,receipt)=r.map_err(db_error)?;let(_,command):(Uuid,RecoveryCommand)=decode(&command)?;let receipt:RecoveryReceipt=decode(&receipt)?;history.push(serde_json::json!({"command_id":id,"principal_id":principal,"occurred_ms":at,"action":command.action,"reason":command.reason,"evidence":command.evidence,"receipt":receipt}));}
   Ok(serde_json::json!({"case":summary,"attempts":attempts,"recovery_history":history,"history_limit":100}))
  }).await
    }
}
