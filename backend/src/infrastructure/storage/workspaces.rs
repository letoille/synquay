use super::{SqliteStore, db_error, encode, millis};
use crate::{
    application::ports::{PortError, PortFuture, workspaces::WorkspaceRepository},
    domain::{
        audit::{AuditPhase, AuditRecord},
        identity::{Action, Resource},
        workspace::*,
    },
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{collections::HashSet, time::SystemTime};
use uuid::Uuid;

fn token<T: serde::Serialize>(v: &T) -> Result<String, PortError> {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or(PortError::Unavailable)
}
fn from_token<T: serde::de::DeserializeOwned>(v: String) -> Result<T, PortError> {
    serde_json::from_value(serde_json::Value::String(v)).map_err(|_| PortError::Unavailable)
}
fn workspace(c: &Connection, id: Uuid) -> Result<Workspace, PortError> {
    let (name, state, revision): (String, String, u64) = c
        .query_row(
            "SELECT name,state,revision FROM workspaces WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or(PortError::PermissionDenied)?;
    Ok(Workspace {
        id,
        name,
        state: from_token(state)?,
        revision,
    })
}
fn member(c: &Connection, workspace_id: Uuid, principal_id: Uuid) -> Result<Membership, PortError> {
    let (role, state): (String, String) = c
        .query_row(
            "SELECT role,state FROM workspace_members WHERE workspace=?1 AND principal=?2",
            params![workspace_id.to_string(), principal_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or(PortError::PermissionDenied)?;
    Ok(Membership {
        workspace_id,
        principal_id,
        role: from_token(role)?,
        state: from_token(state)?,
    })
}
fn access(c: &Connection, id: Uuid, principal: Uuid) -> Result<WorkspaceAccess, PortError> {
    Ok(WorkspaceAccess {
        workspace: workspace(c, id)?,
        member: member(c, id, principal)?,
    })
}
fn audit(c: &Connection, id: Uuid, actor: Uuid, request_id: Uuid) -> Result<(), PortError> {
    let record = AuditRecord {
        record_id: Uuid::new_v4(),
        operation_id: request_id,
        request_id,
        workspace_id: id,
        principal_id: actor,
        action: Action::ManageWorkspace,
        resource: Resource::Workspace { workspace_id: id },
        phase: AuditPhase::Succeeded,
        occurred_at: SystemTime::now(),
    };
    c.execute(
        "INSERT INTO audit(record_id,workspace,occurred_ms,record) VALUES(?1,?2,?3,?4)",
        params![
            record.record_id.to_string(),
            id.to_string(),
            millis(record.occurred_at)?,
            encode(&record)?
        ],
    )
    .map_err(db_error)?;
    Ok(())
}
impl WorkspaceRepository for SqliteStore {
    fn bootstrap(
        &self,
        space: Workspace,
        owner: Uuid,
        members: Vec<Membership>,
    ) -> PortFuture<'_, ()> {
        Box::pin(self.run_critical(move |c| {
            let invalid=||PortError::InvalidInput {code:"invalid_workspace_bootstrap"};
            let mut ids=HashSet::new();
            if !space.is_valid() || space.revision != 1 || space.state != WorkspaceState::Active || owner.is_nil() || members.len()>256
                || !members.iter().all(|m| m.is_valid() && m.workspace_id==space.id && m.principal_id!=owner && m.role!=WorkspaceRole::Owner && ids.insert(m.principal_id)) { return Err(invalid()); }
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspaces WHERE id=?1)",[space.id.to_string()],|r|r.get(0)).map_err(db_error)?;
            if !exists {
                let request_id = Uuid::new_v4();
                tx.execute("INSERT INTO workspaces VALUES(?1,?2,'active',1)",params![space.id.to_string(),space.name]).map_err(db_error)?;
                tx.execute("INSERT INTO workspace_members VALUES(?1,?2,'owner','active')",params![space.id.to_string(),owner.to_string()]).map_err(db_error)?;
                let initial=encode(&serde_json::json!({"bootstrap":{"workspace":space,"owner":owner,"members":members}}))?;
                tx.execute("INSERT INTO workspace_changes VALUES(?1,1,?2,?3,?4,?5)",params![space.id.to_string(),owner.to_string(),request_id.to_string(),millis(SystemTime::now())?,initial]).map_err(db_error)?;
                for m in members {
                    tx.execute("INSERT INTO workspace_members VALUES(?1,?2,?3,?4)",params![m.workspace_id.to_string(),m.principal_id.to_string(),token(&m.role)?,token(&m.state)?]).map_err(db_error)?;
                }
                audit(&tx, space.id, owner, request_id)?;
            }
            tx.commit().map_err(|_|PortError::OutcomeUnknown)
        }))
    }
    fn access(&self, id: Uuid, principal: Uuid) -> PortFuture<'_, WorkspaceAccess> {
        Box::pin(self.run(move |c| {
            let tx = c.transaction().map_err(db_error)?;
            access(&tx, id, principal)
        }))
    }
    fn update(
        &self,
        id: Uuid,
        actor: Uuid,
        request_id: Uuid,
        update: WorkspaceUpdate,
    ) -> PortFuture<'_, Workspace> {
        Box::pin(self.run_critical(move |c| {
            if request_id.is_nil() || update.expected_revision==0 || update.expected_revision>=i64::MAX as u64-1 { return Err(PortError::InvalidInput {code:"invalid_workspace_update"}); }
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let current=access(&tx,id,actor)?;
            if !current.manages() || (current.member.role!=WorkspaceRole::Owner && !matches!(update.change,WorkspaceChange::Rename {..})) { return Err(PortError::PermissionDenied); }
            if current.workspace.revision!=update.expected_revision { return Err(PortError::Conflict); }
            let change_record=encode(&update)?;
            let mut next=current.workspace;
            match update.change {
                WorkspaceChange::Rename {name} => { next.name=name; if !next.is_valid() {return Err(PortError::InvalidInput {code:"invalid_workspace_name"});} }
                WorkspaceChange::SetState {state} => next.state=state,
                WorkspaceChange::SetMember {principal_id,role,state} => {
                    if principal_id.is_nil() { return Err(PortError::InvalidInput {code:"invalid_member"}); }
                    let previous=member(&tx,id,principal_id);
                    if let Ok(old)=&previous {
                        if old.role==WorkspaceRole::Owner && old.state==MemberState::Active && (role!=WorkspaceRole::Owner || state!=MemberState::Active) {
                            let another:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_members WHERE workspace=?1 AND principal<>?2 AND role='owner' AND state='active')",params![id.to_string(),principal_id.to_string()],|r|r.get(0)).map_err(db_error)?;
                            if !another { return Err(PortError::Conflict); }
                        }
                    } else if !matches!(previous,Err(PortError::PermissionDenied)) { return Err(PortError::Unavailable); }
                    if previous.is_err() {
                        let count:u32=tx.query_row("SELECT COUNT(*) FROM workspace_members WHERE workspace=?1",[id.to_string()],|r|r.get(0)).map_err(db_error)?;
                        if count>=256 { return Err(PortError::CapacityExceeded {reason:"workspace_members"}); }
                    }
                    tx.execute("INSERT INTO workspace_members VALUES(?1,?2,?3,?4) ON CONFLICT(workspace,principal) DO UPDATE SET role=excluded.role,state=excluded.state",params![id.to_string(),principal_id.to_string(),token(&role)?,token(&state)?]).map_err(db_error)?;
                }
            }
            next.revision+=1;
            tx.execute("UPDATE workspaces SET name=?2,state=?3,revision=?4 WHERE id=?1",params![id.to_string(),next.name,token(&next.state)?,next.revision]).map_err(db_error)?;
            tx.execute("INSERT INTO workspace_changes VALUES(?1,?2,?3,?4,?5,?6)",params![id.to_string(),next.revision,actor.to_string(),request_id.to_string(),millis(SystemTime::now())?,change_record]).map_err(db_error)?;
            audit(&tx,id,actor,request_id)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(next)
        }))
    }
    fn members(
        &self,
        id: Uuid,
        actor: Uuid,
        after: Option<Uuid>,
        limit: u32,
    ) -> PortFuture<'_, Vec<Membership>> {
        Box::pin(self.run(move|c| {
            if !(1..=100).contains(&limit) { return Err(PortError::InvalidInput {code:"invalid_member_page"}); }
            let tx=c.transaction().map_err(db_error)?;
            if !access(&tx,id,actor)?.manages() { return Err(PortError::PermissionDenied); }
            let mut stmt=tx.prepare("SELECT principal,role,state FROM workspace_members WHERE workspace=?1 AND principal>?2 ORDER BY principal LIMIT ?3").map_err(db_error)?;
            let rows=stmt.query_map(params![id.to_string(),after.map(|id|id.to_string()).unwrap_or_default(),limit],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(db_error)?;
            rows.map(|r| { let (principal,role,state)=r.map_err(db_error)?; Ok(Membership{workspace_id:id,principal_id:Uuid::parse_str(&principal).map_err(|_|PortError::Unavailable)?,role:from_token(role)?,state:from_token(state)?}) }).collect()
        }))
    }
}
