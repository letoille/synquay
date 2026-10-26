//! Authorized reads, fenced worker leases and atomic completion with the delivery outbox.
use super::*;

impl AgentJobStore for SqliteStore {
    fn list(&self, ctx: AccessContext, after: Option<Uuid>) -> PortFuture<'_, Vec<JobSummary>> {
        Box::pin(self.run(move|c|{
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
let b=authorize(&tx,&ctx,false)?;

        let mut q=tx.prepare("SELECT job_id FROM agent_jobs WHERE workspace=?1 AND profile=?2 AND job_id>?3 ORDER BY job_id LIMIT 50").map_err(db_error)?;

        let ids=q.query_map(params![b.source.workspace_id.to_string(),b.config.profile_id.to_string(),after.map(|v|v.to_string()).unwrap_or_default()],|r|r.get::<_,String>(0)).map_err(db_error)?.collect::<Result<Vec<_>,_>>().map_err(db_error)?;
drop(q);

        let result=ids.into_iter().map(|s|summary(&tx,&b,Uuid::parse_str(&s).map_err(|_|PortError::Unavailable)?)).collect::<Result<_,_>>()?;

        audit(&tx,&ctx,&b,false,ctx.operation.request_id)?;
tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
Ok(result)
    }))
    }
    fn read(&self, ctx: AccessContext, id: Uuid) -> PortFuture<'_, JobInput> {
        Box::pin(self.run(move |c| {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;

            let b = authorize(&tx, &ctx, false)?;

            let result = input(&tx, &b, id)?;

            audit(&tx, &ctx, &b, false, id)?;

            tx.commit().map_err(|_| PortError::OutcomeUnknown)?;

            Ok(result)
        }))
    }
    fn claim(&self, ctx: AccessContext, claim_id: Uuid) -> PortFuture<'_, Option<ClaimedJob>> {
        Box::pin(self.run_critical(move|c|{
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
let b=authorize(&tx,&ctx,true)?;
let w=b.source.workspace_id.to_string();
let p=b.config.profile_id.to_string();
let owner=ctx.identity.principal_id.to_string();
let client=ctx.identity.client_id.map(|v|v.to_string()).unwrap_or_default();

        let existing:Option<(String,u32)>=tx.query_row("SELECT job_id,generation FROM agent_claims WHERE workspace=?1 AND profile=?2 AND principal=?3 AND client=?4 AND claim_id=?5",params![w,p,owner,client,claim_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;

        if let Some((id,generation))=existing {let token=ClaimToken{job_id:Uuid::parse_str(&id).map_err(|_|PortError::Unavailable)?,claim_id,generation};
let lease_until_ms=owned(&tx,&ctx,&b,&token,true)?;
let job=input(&tx,&b,token.job_id)?;
audit(&tx,&ctx,&b,true,claim_id)?;
tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
return Ok(Some(ClaimedJob{token,lease_until_ms,job}))}
        sweep(&tx,&b)?;
let now=millis(SystemTime::now())?;

        let active:u32=tx.query_row("SELECT COUNT(*) FROM agent_jobs WHERE workspace=?1 AND profile=?2 AND state='claimed' AND lease_until_ms>?3",params![w,p,now],|r|r.get(0)).map_err(db_error)?;

        if active>=b.config.max_in_flight {tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
return Err(PortError::RateLimited{retry_after:Duration::from_secs(1)})}
        let row:Option<(String,u32,i64)>=tx.query_row("SELECT job_id,attempts,deadline_ms FROM agent_jobs WHERE workspace=?1 AND profile=?2 AND state='queued' AND due_ms<=?3 AND deadline_ms>?3 AND attempts<?4 ORDER BY due_ms,job_id LIMIT 1",params![w,p,now,b.config.max_attempts],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;

        let Some((id,attempts,deadline))=row else{tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
return Ok(None)};

        let generation=attempts+1;
let until=(now+b.config.lease_seconds as i64*1000).min(deadline);

        tx.execute("UPDATE agent_jobs SET state='claimed',attempts=?3,owner=?4,client=?5,claim_id=?6,lease_until_ms=?7 WHERE workspace=?1 AND job_id=?2",params![w,id,generation,owner,client,claim_id.to_string(),until]).map_err(db_error)?;

        tx.execute("INSERT INTO agent_claims VALUES(?1,?2,?3,?4,?5,?6,?7)",params![w,p,owner,client,claim_id.to_string(),id,generation]).map_err(db_error)?;

        let token=ClaimToken{job_id:Uuid::parse_str(&id).map_err(|_|PortError::Unavailable)?,claim_id,generation};
let job=input(&tx,&b,token.job_id)?;

        audit(&tx,&ctx,&b,true,claim_id)?;
tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
Ok(Some(ClaimedJob{token,lease_until_ms:until,job}))
    }))
    }
    fn renew(&self, ctx: AccessContext, token: ClaimToken) -> PortFuture<'_, ClaimedJob> {
        Box::pin(self.run_critical(move|c|{
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
let b=authorize(&tx,&ctx,true)?;
owned(&tx,&ctx,&b,&token,true)?;

        let until=millis(SystemTime::now())?+b.config.lease_seconds as i64*1000;

        tx.execute("UPDATE agent_jobs SET lease_until_ms=MIN(deadline_ms,?3) WHERE workspace=?1 AND job_id=?2",params![b.source.workspace_id.to_string(),token.job_id.to_string(),until]).map_err(db_error)?;

        let until=owned(&tx,&ctx,&b,&token,true)?;
let job=input(&tx,&b,token.job_id)?;

        audit(&tx,&ctx,&b,true,token.job_id)?;
tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
Ok(ClaimedJob{token,lease_until_ms:until,job})
    }))
    }
    fn complete(
        &self,
        ctx: AccessContext,
        token: ClaimToken,
        text: String,
    ) -> PortFuture<'_, CompletionReceipt> {
        Box::pin(self.run_critical(move|c|{
        if text.trim().is_empty() || text.len()>8192 || text.contains('\0'){return Err(PortError::InvalidInput{code:"reply_text"})}
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
let b=authorize(&tx,&ctx,true)?;
owned(&tx,&ctx,&b,&token,false)?;

        let w=b.source.workspace_id.to_string();
let id=token.job_id;
let hash=ring::digest::digest(&ring::digest::SHA256,text.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>();

        let old:Option<String>=tx.query_row("SELECT result_hash FROM agent_jobs WHERE workspace=?1 AND job_id=?2",params![w,id.to_string()],|r|r.get(0)).map_err(db_error)?;

        if let Some(old)=old {if old!=hash{return Err(PortError::Conflict)}let receipt=CompletionReceipt{job_id:id,delivery_id:summary(&tx,&b,id)?.delivery_id.ok_or(PortError::Unavailable)?,state:"completed_reply_queued".into(),duplicate:true};
audit(&tx,&ctx,&b,true,id)?;
tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
return Ok(receipt)}
        let job=input(&tx,&b,id)?;
let delivery_id=Uuid::new_v4();

        // Only trusted binding chooses the HTTP adapter. It uses source_event_id to resolve the original conversation.
        let delivery=PendingDelivery{delivery_id,route_rule:None,destination:b.destination.clone(),message:Message{destination_binding:None,parts:vec![ContentPart::Json(serde_json::json!({"type":"agent.reply","event_id":id,"source_event_id":job.source_event_id,"text":text}))],reply_route_id:None},correlation_id:ctx.operation.correlation_id,causation_id:id,route_trace:vec![]};

        let encoded=record::encode(&delivery)?;
if encoded.len()>REPLY_BUDGET{return Err(PortError::PayloadTooLarge)}
        let work_key=key(&b,id);

        let reserved:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM pending_work WHERE workspace=?1 AND work_key=?2)",params![w,work_key],|r|r.get(0)).map_err(db_error)?;
if !reserved{return Err(PortError::Conflict)}
        tx.execute("INSERT INTO dispatches(workspace,dispatch_key,receipt) VALUES(?1,?2,?3)",params![w,work_key,encode(&vec![delivery_id])?]).map_err(db_error)?;

        tx.execute("INSERT INTO deliveries(workspace,delivery_id,dispatch_key,record,state,due_ms,target_id,config_revision) VALUES(?1,?2,?3,?4,'pending',?5,?6,(SELECT config_revision FROM events WHERE workspace=?1 AND source=?7 AND event_id=?8))",params![w,delivery_id.to_string(),work_key,encoded,millis(SystemTime::now())?,b.destination.id.to_string(),b.source.connection_id.to_string(),id.to_string()]).map_err(db_error)?;

        tx.execute("UPDATE agent_jobs SET state='completed',result_hash=?3,delivery_id=?4 WHERE workspace=?1 AND job_id=?2",params![w,id.to_string(),hash,delivery_id.to_string()]).map_err(db_error)?;

        audit(&tx,&ctx,&b,true,id)?;
tx.commit().map_err(|_|PortError::OutcomeUnknown)?;

        Ok(CompletionReceipt{job_id:id,delivery_id,state:"completed_reply_queued".into(),duplicate:false})
    }))
    }
    fn fail(
        &self,
        ctx: AccessContext,
        token: ClaimToken,
        retry: bool,
    ) -> PortFuture<'_, JobSummary> {
        Box::pin(self.run_critical(move |c| {
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;

            let b = authorize(&tx, &ctx, true)?;

            owned(&tx, &ctx, &b, &token, true)?;

            let state = if retry && token.generation < b.config.max_attempts {
                "queued"
            } else {
                "failed"
            };

            let w = b.source.workspace_id.to_string();

            tx.execute(
                "UPDATE agent_jobs SET state=?3,due_ms=?4 WHERE workspace=?1 AND job_id=?2",
                params![
                    w,
                    token.job_id.to_string(),
                    state,
                    millis(SystemTime::now())? + 5000
                ],
            )
            .map_err(db_error)?;

            if state == "failed" {
                capacity::release_completed(&tx, &w, &key(&b, token.job_id))?;
            }
            let result = summary(&tx, &b, token.job_id)?;

            audit(&tx, &ctx, &b, true, token.job_id)?;

            tx.commit().map_err(|_| PortError::OutcomeUnknown)?;

            Ok(result)
        }))
    }
}
