use super::*;
use crate::{
    domain::{connector::ConnectionScope, managed_route::RouteSnapshot},
    infrastructure::pipeline::message_connections::{ConnectionInput, ConnectionSnapshot},
};
impl SqliteStore {
    pub(crate) async fn message_connections(
        &self,
        scope: ConnectionScope,
    ) -> Result<ConnectionSnapshot, PortError> {
        let path = self.path.clone();
        self.run(move |c| {
            let record: Option<String> = c
                .query_row(
                    "SELECT record FROM message_connections WHERE workspace=?1 AND source=?2",
                    params![
                        scope.workspace_id.to_string(),
                        scope.connection_id.to_string()
                    ],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            let mut snapshot:ConnectionSnapshot = record.as_deref().map(decode).unwrap_or_else(||Ok(ConnectionSnapshot::default()))?;
            for connection in &mut snapshot.connections {
                if let crate::infrastructure::connectors::message_config::ConnectionConfig::QqBot(qq) = &mut connection.config {
                    let encrypted:Option<Vec<u8>> = c.query_row("SELECT encrypted FROM connection_secrets WHERE workspace=?1 AND source=?2 AND connection=?3",params![scope.workspace_id.to_string(),scope.connection_id.to_string(),connection.id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
                    if let Some(value)=encrypted {
                        qq.resolved_secret=Some(Arc::from(connection_secrets::Key::load(&path,false)?.open(&connection_secrets::aad(scope,connection.id),value)?));
                    }
                }
            }
            Ok(snapshot)
        })
        .await
    }
    pub(crate) async fn connection_operation(
        &self,
        scope: ConnectionScope,
        actor: Uuid,
        input: ConnectionInput,
    ) -> Result<Option<u64>, PortError> {
        let path = self.path.clone();
        self.run(move|c|{
        let prior:Option<(String,String,u64)>=c.query_row("SELECT actor,request,revision FROM message_connection_operations WHERE workspace=?1 AND source=?2 AND operation_id=?3",params![scope.workspace_id.to_string(),scope.connection_id.to_string(),input.operation_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
        match prior {Some((a,r,v)) if a==actor.to_string()&&r==connection_secrets::request_record(c,&path,&input)?=>Ok(Some(v)),Some(_)=>Err(PortError::Conflict),None=>Ok(None)}
    }).await
    }
    pub(crate) async fn save_message_connections(
        &self,
        scope: ConnectionScope,
        actor: Uuid,
        input: ConnectionInput,
        next: ConnectionSnapshot,
        routes: RouteSnapshot,
        affected: Option<(Uuid, Uuid)>,
    ) -> Result<(), PortError> {
        let path = self.path.clone();
        self.run_critical(move|c|{
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
        let ws=scope.workspace_id.to_string();let src=scope.connection_id.to_string();
        let revision:Option<u64>=tx.query_row("SELECT revision FROM message_connections WHERE workspace=?1 AND source=?2",params![ws,src],|r|r.get(0)).optional().map_err(db_error)?;
        if revision.unwrap_or(0)!=input.expected_revision{return Err(PortError::Conflict);}
        let limits:String=tx.query_row("SELECT record FROM capacity_policy WHERE singleton=1",[],|r|r.get(0)).map_err(db_error)?;
        let limits:capacity::CapacityLimits=decode(&limits)?;
        if capacity::available_space(&path)?<limits.min_free_disk_bytes {return Err(PortError::Unavailable);}

        if let Some((source,target))=affected {
            // Never reroute queued work to a changed URL/topic or orphan unresolved history.
            let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM deliveries WHERE workspace=?1 AND target_id=?2 AND state<>'submitted' AND resolution IS NULL)",params![ws,target.to_string()],|r|r.get(0)).map_err(db_error)?;
            let inbound:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM routing_tasks WHERE workspace=?1 AND source=?2 AND state<>'completed')",params![ws,source.to_string()],|r|r.get(0)).map_err(db_error)?;
            if pending||inbound{return Err(PortError::InvalidInput{code:"connection_has_pending_work"});}
        }
        // Remove obsolete account discoveries when a bot is removed or replaced.
        if let Some(crate::infrastructure::connectors::message_config::ConnectionConfig::QqBot(qq))=input.connection.as_ref().map(|c|&c.config) {
            tx.execute("DELETE FROM connection_conversations WHERE workspace=?1 AND connection=?2 AND account<>?3",params![ws,input.connection_id.to_string(),qq.app_id]).map_err(db_error)?;
        }else {
            tx.execute("DELETE FROM connection_conversations WHERE workspace=?1 AND connection=?2",params![ws,input.connection_id.to_string()]).map_err(db_error)?;
        }
        super::device_state::configure(&tx,scope.workspace_id,input.connection_id,input.connection.as_ref())?;
        let request=connection_secrets::request_record(&tx,&path,&input)?;
        if let Some(crate::infrastructure::connectors::message_config::ConnectionConfig::QqBot(_))=input.connection.as_ref().map(|c|&c.config) {
            if let Some(value)=input.secret.as_ref().filter(|v|!v.is_empty()) {
                let encrypted=connection_secrets::Key::load(&path,false)?.seal(&connection_secrets::aad(scope,input.connection_id),value)?;
                tx.execute("INSERT INTO connection_secrets VALUES(?1,?2,?3,?4) ON CONFLICT(workspace,source,connection) DO UPDATE SET encrypted=excluded.encrypted",params![ws,src,input.connection_id.to_string(),encrypted]).map_err(db_error)?;
            }
        } else {
            tx.execute("DELETE FROM connection_secrets WHERE workspace=?1 AND source=?2 AND connection=?3",params![ws,src,input.connection_id.to_string()]).map_err(db_error)?;
        }
        tx.execute("INSERT INTO message_connections VALUES(?1,?2,?3,?4) ON CONFLICT(workspace,source) DO UPDATE SET revision=excluded.revision,record=excluded.record",params![ws,src,next.revision,capacity::encode_bounded(&next)?]).map_err(db_error)?;
        tx.execute("UPDATE managed_routes SET revision=?1,record=?2 WHERE workspace=?3 AND source=?4",params![routes.revision,capacity::encode_bounded(&routes)?,ws,src]).map_err(db_error)?;
        tx.execute("INSERT INTO message_connection_operations VALUES(?1,?2,?3,?4,?5,?6,?7)",params![ws,src,input.operation_id.to_string(),actor.to_string(),request,next.revision,millis(SystemTime::now())?]).map_err(db_error)?;
        tx.commit().map_err(|_|PortError::OutcomeUnknown)
    }).await
    }
}
