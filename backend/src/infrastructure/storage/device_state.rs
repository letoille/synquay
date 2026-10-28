//! State projection commits in the same transaction as event ingress, before PUBACK.
use super::*;
use crate::domain::{
    capability::mqtt::*,
    connector::ConnectionScope,
    event::{EventEnvelope, EventPayload},
};
use crate::infrastructure::connectors::message_config::{ConnectionConfig, MessageConnection};
use serde_json::{Value, json};
pub(super) fn gate(path: &std::path::Path) -> Arc<tokio::sync::RwLock<()>> {
    type Gates =
        std::collections::HashMap<std::path::PathBuf, std::sync::Weak<tokio::sync::RwLock<()>>>;
    static GATES: std::sync::OnceLock<std::sync::Mutex<Gates>> = std::sync::OnceLock::new();
    let mut gates = GATES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    gates.retain(|_, v| v.strong_count() > 0);
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(gate) = gates.get(&key).and_then(std::sync::Weak::upgrade) {
        return gate;
    }
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    gates.insert(key, Arc::downgrade(&gate));
    gate
}
impl SqliteStore {
    pub(crate) async fn device_sent(
        &self,
        workspace: Uuid,
        connection: Uuid,
    ) -> Result<(), PortError> {
        self.run(move|c|{c.execute("INSERT INTO message_totals VALUES(?1,'sent',?2,1) ON CONFLICT(workspace,kind,id) DO UPDATE SET total=total+1",params![workspace.to_string(),Uuid::from_u128(connection.as_u128() ^ (1u128 << 127)).to_string()]).map_err(db_error)?;Ok(())}).await
    }
    pub(crate) async fn device_reply(
        &self,
        scope: ConnectionScope,
        topic: String,
        value: Value,
    ) -> Result<(), PortError> {
        self.run(move|c|{
            let record:String=c.query_row("SELECT record FROM connection_devices WHERE workspace=?1 AND connection=?2",params![scope.workspace_id.to_string(),scope.connection_id.to_string()],|r|r.get(0)).map_err(db_error)?;
            let conn:MessageConnection=decode(&record)?;
            let wrapped=matches!(&conn.config,ConnectionConfig::Mqtt(c) if c.subscription.as_ref().is_some_and(|s|s.include_metadata));
            let event=EventEnvelope{state_report:false,event_id:Uuid::new_v4(),scope,event_type:"mqtt.reply".into(),schema_version:1,occurred_at:None,received_at:SystemTime::now(),source_event_id:None,correlation_id:Uuid::new_v4(),causation_id:None,route_trace:vec![],route_context:Some(crate::domain::connection_routing::RouteBinding::new("mqtt",json!({"topic":topic,"retain":false}))),payload:EventPayload::Json(if wrapped{json!({"payload":value})}else{value})};
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;observe(&tx,&event)?;tx.commit().map_err(db_error)
        }).await
    }

    pub async fn device_connections(
        &self,
        workspace: Uuid,
    ) -> Result<Vec<MessageConnection>, PortError> {
        self.run(move |c| {
            let mut q = c
                .prepare(
                    "SELECT record FROM connection_devices WHERE workspace=?1 ORDER BY connection",
                )
                .map_err(db_error)?;
            q.query_map([workspace.to_string()], |r| r.get::<_, String>(0))
                .map_err(db_error)?
                .map(|r| decode(&r.map_err(db_error)?))
                .collect()
        })
        .await
    }
    pub async fn device_connection(
        &self,
        scope: ConnectionScope,
    ) -> Result<MessageConnection, PortError> {
        self.run(move |c| {
            let s: Option<String> = c
                .query_row(
                    "SELECT record FROM connection_devices WHERE workspace=?1 AND connection=?2",
                    params![
                        scope.workspace_id.to_string(),
                        scope.connection_id.to_string()
                    ],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            decode(&s.ok_or(PortError::NotFound)?)
        })
        .await
    }
    pub async fn resource_states(&self, scope: ConnectionScope) -> Result<Value, PortError> {
        use crate::application::ports::connection_data::{ConnectionStateReader, StateQuery};
        let snapshot = self
            .read_connection_state(StateQuery {
                scope,
                resource: None,
                group: None,
            })
            .await?;
        serde_json::to_value(snapshot).map_err(|_| PortError::Unavailable)
    }
}

impl crate::application::ports::connection_data::ConnectionStateReader for SqliteStore {
    fn read_connection_state(
        &self,
        query: crate::application::ports::connection_data::StateQuery,
    ) -> crate::application::ports::PortFuture<
        '_,
        crate::application::ports::connection_data::StateSnapshot,
    > {
        use crate::application::ports::connection_data::{
            Freshness, StateField, StateGroup, StateOrigin, StateResource,
        };
        Box::pin(self.run(move |c| {
            if query.group.is_some() && query.resource.is_none() { return Err(PortError::InvalidInput { code: "state_group_requires_resource" }); }
            if query.resource.as_ref().is_some_and(|s| s.is_empty() || s.len() > 128) || query.group.as_ref().is_some_and(|s| s.is_empty() || s.len() > 128) { return Err(PortError::InvalidInput { code: "invalid_state_query" }); }
            let scope = query.scope;
            // Read the connection definition and all fields from one SQLite snapshot.
            let tx = c.transaction_with_behavior(TransactionBehavior::Deferred).map_err(db_error)?;
            let record:Option<String>=tx.query_row("SELECT record FROM connection_devices WHERE workspace=?1 AND connection=?2",params![scope.workspace_id.to_string(),scope.connection_id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            let conn:MessageConnection=decode(&record.ok_or(PortError::NotFound)?)?;
            let ConnectionConfig::Mqtt(config)=conn.config else {return Err(PortError::Unsupported)};
            if let Some(resource) = &query.resource {
                let found = config.capabilities.resources.iter().find(|r| &r.id == resource).ok_or(PortError::NotFound)?;
                if query.group.as_ref().is_some_and(|group| !found.groups.iter().any(|g| &g.id == group)) { return Err(PortError::NotFound); }
            }
            let now=millis(SystemTime::now())?;
            let mut result = std::collections::BTreeMap::new();
            for resource in config.capabilities.resources.into_iter().filter(|r| query.resource.as_ref().is_none_or(|id| *id==r.id)) {
                let mut groups=std::collections::BTreeMap::new();
                for group in resource.groups.into_iter().filter(|g| query.group.as_ref().is_none_or(|id| *id==g.id)) {
                    let mut q=tx.prepare("SELECT field,value,received_ms,source_ms,retained FROM resource_state WHERE workspace=?1 AND connection=?2 AND resource=?3 AND state_group=?4").map_err(db_error)?;
                    let rows=q.query_map(params![scope.workspace_id.to_string(),scope.connection_id.to_string(),resource.id,group.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<i64>>(3)?,r.get::<_,bool>(4)?))).map_err(db_error)?;
                    let mut fields=std::collections::BTreeMap::new();
                    for row in rows { let (key,value,received,source,retained)=row.map_err(db_error)?;let age=now.saturating_sub(source.unwrap_or(received)).max(0) as u64;
                        fields.insert(key,StateField { value:decode(&value)?,received_at_ms:Some(received),source_at_ms:Some(source),origin:Some(if retained {StateOrigin::Retained}else{StateOrigin::Live}),freshness:if retained && source.is_none(){Freshness::Unknown}else if !conn.enabled || (group.max_age_ms>0 && age>group.max_age_ms){Freshness::Stale}else{Freshness::Fresh} });
                    }
                    for field in &group.fields { fields.entry(field.name.clone()).or_insert(StateField{value:Value::Null,received_at_ms:None,source_at_ms:None,origin:None,freshness:Freshness::Unknown}); }
                    groups.insert(group.id,StateGroup { name:group.name,fields });
                }
                result.insert(resource.id,StateResource{name:resource.name,groups});
            }
            tx.commit().map_err(db_error)?;
            Ok(result)
        }))
    }
}

pub(super) fn observe(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
) -> Result<bool, PortError> {
    let scope = event.scope;
    let w = scope.workspace_id.to_string();
    let c = scope.connection_id.to_string();
    let record: Option<String> = tx
        .query_row(
            "SELECT record FROM connection_devices WHERE workspace=?1 AND connection=?2",
            params![w, c],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let Some(record) = record else {
        return Ok(false);
    };
    let conn: MessageConnection = decode(&record)?;
    if !conn.enabled {
        return Ok(false);
    }
    let ConnectionConfig::Mqtt(config) = conn.config else {
        return Ok(false);
    };
    let Some(context) = event.route_context.as_ref().filter(|r| r.adapter == "mqtt") else {
        return Ok(false);
    };
    let topic = context.config["topic"].as_str().unwrap_or("");
    let retained = context.config["retain"].as_bool().unwrap_or(true);
    let EventPayload::Json(value) = &event.payload else {
        return Ok(false);
    };
    let value = if config
        .subscription
        .as_ref()
        .is_some_and(|s| s.include_metadata)
    {
        &value["payload"]
    } else {
        value
    };
    let received = millis(event.received_at)?;
    let mut matched = false;
    for resource in &config.capabilities.resources {
        for r in &resource.groups {
            if r.topic != topic
                || r.identity
                    .as_ref()
                    .is_some_and(|i| value.pointer(&i.pointer) != Some(&i.value))
            {
                continue;
            }
            let mut mapped = Vec::new();
            for field in &r.fields {
                let Some(raw) = value.pointer(&field.pointer) else {
                    continue;
                };
                let v = if let Some(eq) = &field.equals {
                    if !(raw.is_number() && eq.is_number()
                        || raw.is_string() && eq.is_string()
                        || raw.is_boolean() && eq.is_boolean())
                    {
                        continue;
                    }
                    Value::Bool(raw == eq)
                } else {
                    raw.clone()
                };
                let valid = match field.value_type {
                    StateType::Boolean => v.is_boolean(),
                    StateType::Number => v.is_number(),
                    StateType::String => v.as_str().is_some_and(|s| s.len() <= 1024),
                };
                if valid {
                    mapped.push((&field.name, v));
                }
            }
            if mapped.is_empty() {
                continue;
            }
            matched = true;
            let source = match &r.source_time_pointer {
                Some(p) => match value.pointer(p).and_then(Value::as_i64) {
                    Some(t) if t >= 0 && t <= received => Some(t),
                    _ => continue,
                },
                None => None,
            };
            // Retained data without a device timestamp cannot overwrite live observations.
            let latest:Option<i64>=tx.query_row("SELECT max(coalesce(source_ms,received_ms)) FROM resource_state WHERE workspace=?1 AND connection=?2 AND resource=?3 AND state_group=?4",params![w,c,resource.id,r.id],|row|row.get(0)).map_err(db_error)?;
            if source.is_some_and(|s| latest.is_some_and(|l| s < l)) {
                continue;
            }
            if retained && source.is_none() && latest.is_some() {
                continue;
            }
            if r.replace {
                tx.execute("DELETE FROM resource_state WHERE workspace=?1 AND connection=?2 AND resource=?3 AND state_group=?4",params![w,c,resource.id,r.id]).map_err(db_error)?;
            }
            for (name, v) in mapped {
                tx.execute("INSERT INTO resource_state VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(workspace,connection,resource,state_group,field) DO UPDATE SET value=excluded.value,received_ms=excluded.received_ms,source_ms=excluded.source_ms,retained=excluded.retained",params![w,c,resource.id,r.id,name,encode(&v)?,received,source,retained]).map_err(db_error)?;
            }
            tracing::debug!(event="resource.state_updated",connection_id=%scope.connection_id,resource=%resource.id,state_group=%r.id);
        }
    }
    Ok(matched)
}

/// Invalidate observations when their definition or transport changes; never relabel old data.
pub(super) fn configure(
    tx: &rusqlite::Transaction<'_>,
    workspace: Uuid,
    connection: Uuid,
    next: Option<&MessageConnection>,
) -> Result<(), PortError> {
    let w = workspace.to_string();
    let c = connection.to_string();
    let old: Option<String> = tx
        .query_row(
            "SELECT record FROM connection_devices WHERE workspace=?1 AND connection=?2",
            params![w, c],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let next = next
        .filter(|c| matches!(c.config, ConnectionConfig::Mqtt(_)))
        .map(encode)
        .transpose()?;
    if old == next {
        return Ok(());
    }
    if let (Some(old), Some(new)) = (&old, &next) {
        let old: MessageConnection = decode(old)?;
        let new: MessageConnection = decode(new)?;
        if serde_json::to_value(&old.config).ok() == serde_json::to_value(&new.config).ok() {
            tx.execute(
                "UPDATE connection_devices SET record=?3 WHERE workspace=?1 AND connection=?2",
                params![w, c, encode(&new)?],
            )
            .map_err(db_error)?;
            return Ok(());
        }
    }
    tx.execute(
        "DELETE FROM resource_state WHERE workspace=?1 AND connection=?2",
        params![w, c],
    )
    .map_err(db_error)?;
    match next {
        Some(s) => {
            tx.execute("INSERT INTO connection_devices VALUES(?1,?2,?3) ON CONFLICT(workspace,connection) DO UPDATE SET record=excluded.record",params![w,c,s]).map_err(db_error)?;
        }
        None => {
            tx.execute(
                "DELETE FROM connection_devices WHERE workspace=?1 AND connection=?2",
                params![w, c],
            )
            .map_err(db_error)?;
        }
    }
    // Revoke old bindings atomically with configuration. Changed capabilities need explicit republication.
    let mut q=tx.prepare("SELECT h.profile,h.revision,r.record FROM tool_heads h JOIN tool_releases r USING(workspace,profile,revision) WHERE h.workspace=?1").map_err(db_error)?;
    let rows = q
        .query_map([&w], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    drop(q);
    for (profile, revision, record) in rows {
        let mut value: Value = decode(&record)?;
        let tools = value["profile"]["tools"]
            .as_array_mut()
            .ok_or(PortError::Unavailable)?;
        let count = tools.len();
        tools.retain(|t| t["scope"]["connection_id"] != c);
        if count == tools.len() {
            continue;
        }
        value["profile"]["revision"] = json!(revision + 1);
        tx.execute(
            "INSERT INTO tool_releases VALUES(?1,?2,?3,?4,?5)",
            params![
                w,
                profile,
                revision + 1,
                encode(&value)?,
                millis(SystemTime::now())?
            ],
        )
        .map_err(db_error)?;
        tx.execute(
            "UPDATE tool_heads SET revision=?3 WHERE workspace=?1 AND profile=?2",
            params![w, profile, revision + 1],
        )
        .map_err(db_error)?;
    }
    Ok(())
}
