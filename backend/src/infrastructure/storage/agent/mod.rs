//! One configured Agent queue per database. No model execution or model-selected destinations.
use super::{SqliteStore, capacity, db_error, decode, encode, millis, record};

use crate::{
    application::ports::{PortError, PortFuture, agent::AgentJobStore, identity::AccessContext},
    domain::{
        agent::*,
        audit::{AuditPhase, AuditRecord},
        delivery::PendingDelivery,
        event::{EventEnvelope, EventPayload},
        identity::{Action, Resource},
        message::{ContentPart, Message},
    },
};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use std::time::{Duration, SystemTime};

use uuid::Uuid;

const MAX_JOBS: u64 = 10_000;

// Pre-reserve reply storage at ingress so a full queue can still complete existing jobs.
pub(super) const REPLY_BUDGET: usize = 64 * 1024;

fn binding(c: &Connection) -> Result<AgentBinding, PortError> {
    let s: String = c
        .query_row(
            "SELECT record FROM agent_binding WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?
        .ok_or(PortError::NotFound)?;

    decode(&s)
}
fn key(b: &AgentBinding, id: Uuid) -> String {
    format!("event:{}:{id}", b.source.connection_id)
}
fn authorize(c: &Connection, ctx: &AccessContext, write: bool) -> Result<AgentBinding, PortError> {
    ctx.check()?;

    let b = binding(c)?;

    if b.source.workspace_id != ctx.identity.workspace_id
        || b.config.profile_id != ctx.operation.scope.connection_id
    {
        return Err(PortError::PermissionDenied);
    }
    let allowed:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM workspaces w JOIN workspace_members m ON m.workspace=w.id WHERE w.id=?1 AND w.state='active' AND m.principal=?2 AND m.state='active' AND (?3=0 OR m.role<>'viewer'))",params![ctx.identity.workspace_id.to_string(),ctx.identity.principal_id.to_string(),write],|r|r.get(0)).map_err(db_error)?;

    if !allowed {
        return Err(PortError::PermissionDenied);
    }
    Ok(b)
}
fn audit(
    c: &Connection,
    ctx: &AccessContext,
    b: &AgentBinding,
    write: bool,
    id: Uuid,
) -> Result<(), PortError> {
    record_audit(
        c,
        b,
        ctx.identity.principal_id,
        ctx.operation.request_id,
        write,
        id,
    )
}
fn record_audit(
    c: &Connection,
    b: &AgentBinding,
    principal: Uuid,
    request: Uuid,
    write: bool,
    id: Uuid,
) -> Result<(), PortError> {
    let r = AuditRecord {
        record_id: Uuid::new_v4(),
        operation_id: id,
        request_id: request,
        workspace_id: b.source.workspace_id,
        principal_id: principal,
        action: if write {
            Action::ManageAgentJobs
        } else {
            Action::ReadAgentJobs
        },
        resource: Resource::AgentQueue {
            workspace_id: b.source.workspace_id,
            profile_id: b.config.profile_id,
        },
        phase: AuditPhase::Succeeded,
        occurred_at: SystemTime::now(),
    };

    c.execute(
        "INSERT INTO audit(record_id,workspace,occurred_ms,record) VALUES(?1,?2,?3,?4)",
        params![
            r.record_id.to_string(),
            r.workspace_id.to_string(),
            millis(r.occurred_at)?,
            encode(&r)?
        ],
    )
    .map_err(db_error)?;

    Ok(())
}

fn summary(c: &Connection, b: &AgentBinding, id: Uuid) -> Result<JobSummary, PortError> {
    c.query_row("SELECT j.state,j.attempts,j.deadline_ms,j.delivery_id,d.state FROM agent_jobs j LEFT JOIN deliveries d ON d.workspace=j.workspace AND d.delivery_id=j.delivery_id WHERE j.workspace=?1 AND j.profile=?2 AND j.job_id=?3",params![b.source.workspace_id.to_string(),b.config.profile_id.to_string(),id.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u32>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?))).optional().map_err(db_error)?.ok_or(PortError::NotFound).and_then(|(state,attempts,deadline_ms,delivery,delivery_state)|Ok(JobSummary{job_id:id,state,attempts,deadline_ms,delivery_id:delivery.map(|s|Uuid::parse_str(&s).map_err(|_|PortError::Unavailable)).transpose()?,delivery_state}))
}
fn input(c: &Connection, b: &AgentBinding, id: Uuid) -> Result<JobInput, PortError> {
    let summary = summary(c, b, id)?;

    let s: Option<String> = c
        .query_row(
            "SELECT record FROM events WHERE workspace=?1 AND source=?2 AND event_id=?3",
            params![
                b.source.workspace_id.to_string(),
                b.source.connection_id.to_string(),
                id.to_string()
            ],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;

    let event = s
        .filter(|s| !s.is_empty())
        .map(|s| record::decode::<EventEnvelope>(&s))
        .transpose()?;

    let (payload, source_event_id, event_type) = match event {
        Some(e) => {
            let mapped = if let Some(command) = &b.config.command {
                let captured = command.capture(&e.payload).ok_or(PortError::Conflict)?;
                command
                    .render(&e.payload, &captured)
                    .map_err(|_| PortError::InvalidInput {
                        code: "command_mapping_failed",
                    })?
            } else {
                None
            };
            let EventPayload::Json(p) = e.payload else {
                return Err(PortError::Unsupported);
            };

            let (event_type, payload) = mapped.unwrap_or((e.event_type, p));
            (Some(payload), e.source_event_id, Some(event_type))
        }
        None => (None, None, None),
    };

    Ok(JobInput {
        summary,
        input: payload,
        source_event_id,
        event_type,
    })
}
fn owned(
    c: &Connection,
    ctx: &AccessContext,
    b: &AgentBinding,
    t: &ClaimToken,
    require_live: bool,
) -> Result<i64, PortError> {
    let now = millis(SystemTime::now())?;

    let row:Option<i64>=c.query_row("SELECT lease_until_ms FROM agent_jobs WHERE workspace=?1 AND profile=?2 AND job_id=?3 AND claim_id=?4 AND attempts=?5 AND owner=?6 AND client=?7 AND ((?8=0 AND state='completed') OR (state='claimed' AND lease_until_ms>?9 AND deadline_ms>?9))",params![b.source.workspace_id.to_string(),b.config.profile_id.to_string(),t.job_id.to_string(),t.claim_id.to_string(),t.generation,ctx.identity.principal_id.to_string(),ctx.identity.client_id.map(|v|v.to_string()).unwrap_or_default(),require_live,now],|r|r.get(0)).optional().map_err(db_error)?;

    row.ok_or(PortError::Conflict)
}
fn sweep(tx: &Transaction<'_>, b: &AgentBinding) -> Result<(), PortError> {
    let w = b.source.workspace_id.to_string();

    let now = millis(SystemTime::now())?;

    let mut q=tx.prepare("SELECT job_id FROM agent_jobs WHERE workspace=?1 AND state IN ('queued','claimed') AND deadline_ms<=?2 ORDER BY deadline_ms LIMIT 100").map_err(db_error)?;

    let ids = q
        .query_map(params![w, now], |r| r.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    drop(q);

    for id in ids {
        let job_id = Uuid::parse_str(&id).map_err(|_| PortError::Unavailable)?;

        record_audit(tx, b, b.executor, Uuid::new_v4(), true, job_id)?;

        tx.execute(
            "UPDATE agent_jobs SET state='expired' WHERE workspace=?1 AND job_id=?2",
            params![w, id],
        )
        .map_err(db_error)?;

        capacity::release_completed(
            tx,
            &w,
            &key(b, Uuid::parse_str(&id).map_err(|_| PortError::Unavailable)?),
        )?;
    }
    let mut q=tx.prepare("SELECT job_id,attempts FROM agent_jobs WHERE workspace=?1 AND profile=?2 AND state='claimed' AND lease_until_ms<=?3 ORDER BY lease_until_ms LIMIT 100").map_err(db_error)?;

    let rows = q
        .query_map(params![w, b.config.profile_id.to_string(), now], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    drop(q);

    for (id, attempt) in rows {
        let job_id = Uuid::parse_str(&id).map_err(|_| PortError::Unavailable)?;

        record_audit(tx, b, b.executor, Uuid::new_v4(), true, job_id)?;

        let state = if attempt >= b.config.max_attempts {
            "failed"
        } else {
            "queued"
        };

        tx.execute(
            "UPDATE agent_jobs SET state=?3,due_ms=?4 WHERE workspace=?1 AND job_id=?2",
            params![w, id, state, now],
        )
        .map_err(db_error)?;

        if state == "failed" {
            capacity::release_completed(
                tx,
                &w,
                &key(b, Uuid::parse_str(&id).map_err(|_| PortError::Unavailable)?),
            )?;
        }
    }
    Ok(())
}

mod jobs;
mod queue;
