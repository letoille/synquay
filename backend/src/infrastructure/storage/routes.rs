use super::*;
use crate::domain::{connector::ConnectionScope, managed_route::*};

impl SqliteStore {
    pub(crate) async fn route_counts(
        &self,
        workspace: Uuid,
        rules: Vec<Uuid>,
    ) -> Result<std::collections::BTreeMap<Uuid, std::collections::BTreeMap<String, u64>>, PortError>
    {
        self.run(move |c| {
            let mut query=c.prepare("SELECT route_rule_id,state,COUNT(*) FROM deliveries WHERE workspace=?1 AND route_rule_id IN (SELECT value FROM json_each(?2)) GROUP BY route_rule_id,state").map_err(db_error)?;
            let rows=query.query_map(params![workspace.to_string(),encode(&rules)?], |r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,u64>(2)?))).map_err(db_error)?;
            let mut counts=std::collections::BTreeMap::<Uuid,std::collections::BTreeMap<String,u64>>::new();
            for row in rows {let (id,state,count)=row.map_err(db_error)?; counts.entry(Uuid::parse_str(&id).map_err(|_|PortError::Unavailable)?).or_default().insert(state,count);}
            Ok(counts)
        }).await
    }

    pub(crate) async fn initialize_routes(
        &self,
        scope: ConnectionScope,
        initial: RouteSnapshot,
    ) -> Result<RouteSnapshot, PortError> {
        self.run_critical(move |c| {
            c.execute(
                "INSERT OR IGNORE INTO managed_routes VALUES(?1,?2,?3,?4)",
                params![
                    scope.workspace_id.to_string(),
                    scope.connection_id.to_string(),
                    initial.revision,
                    capacity::encode_bounded(&initial)?
                ],
            )
            .map_err(db_error)?;
            let record: String = c
                .query_row(
                    "SELECT record FROM managed_routes WHERE workspace=?1 AND source=?2",
                    params![
                        scope.workspace_id.to_string(),
                        scope.connection_id.to_string()
                    ],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            decode(&record)
        })
        .await
    }
    pub(crate) async fn route_snapshot(
        &self,
        scope: ConnectionScope,
    ) -> Result<RouteSnapshot, PortError> {
        self.run(move |c| {
            let record: String = c
                .query_row(
                    "SELECT record FROM managed_routes WHERE workspace=?1 AND source=?2",
                    params![
                        scope.workspace_id.to_string(),
                        scope.connection_id.to_string()
                    ],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            decode(&record)
        })
        .await
    }
    pub(crate) async fn route_receipt(
        &self,
        scope: ConnectionScope,
        actor: Uuid,
        operation: Uuid,
    ) -> Result<Option<RouteReceipt>, PortError> {
        self.run(move |c| {
            c.query_row("SELECT revision FROM route_operations WHERE workspace=?1 AND source=?2 AND operation_id=?3 AND actor=?4", params![scope.workspace_id.to_string(), scope.connection_id.to_string(), operation.to_string(), actor.to_string()], |r| Ok(RouteReceipt{operation_id: operation, revision: r.get(0)?})).optional().map_err(db_error)
        }).await
    }
    pub(crate) async fn save_routes(
        &self,
        scope: ConnectionScope,
        actor: Uuid,
        input: RouteInput,
        next: RouteSnapshot,
    ) -> Result<RouteReceipt, PortError> {
        let path = self.path.clone();
        self.run_critical(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let ws = scope.workspace_id.to_string();
            let source = scope.connection_id.to_string();
            let op = input.operation_id.to_string();
            let request = capacity::encode_bounded(&input)?;
            let prior: Option<(String,String,u64)> = tx.query_row("SELECT actor,request,revision FROM route_operations WHERE workspace=?1 AND source=?2 AND operation_id=?3", params![ws,source,op], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            if let Some((owner, previous, revision)) = prior {
                return if owner == actor.to_string() && previous == request { Ok(RouteReceipt{operation_id:input.operation_id,revision}) } else { Err(PortError::Conflict) };
            }
            let limits: String = tx.query_row("SELECT record FROM capacity_policy WHERE singleton=1", [], |r| r.get(0)).map_err(db_error)?;
            let limits: capacity::CapacityLimits = decode(&limits)?;
            if capacity::available_space(&path)? < limits.min_free_disk_bytes { return Err(PortError::Unavailable); }
            let changed = tx.execute("UPDATE managed_routes SET revision=?1, record=?2 WHERE workspace=?3 AND source=?4 AND revision=?5", params![next.revision,capacity::encode_bounded(&next)?,ws,source,input.expected_revision]).map_err(db_error)?;
            if changed != 1 { return Err(PortError::Conflict); }
            tx.execute("INSERT INTO route_operations VALUES(?1,?2,?3,?4,?5,?6,?7)", params![ws,source,op,actor.to_string(),request,next.revision,millis(SystemTime::now())?]).map_err(db_error)?;
            tx.commit().map_err(|_| PortError::OutcomeUnknown)?;
            Ok(RouteReceipt{operation_id: input.operation_id, revision: next.revision})
        }).await
    }
}
