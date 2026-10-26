use super::{SqliteStore, db_error, millis};
use crate::application::ports::PortError;
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};
use uuid::Uuid;

const BOUNDS: [u64; 8] = [100, 1000, 5000, 10000, 50000, 100000, 1000000, u64::MAX];
#[derive(Default)]
pub struct Histogram {
    counts: [AtomicU64; 8],
    sum: AtomicU64,
}
impl Histogram {
    pub fn observe(&self, micros: u64) {
        self.sum.fetch_add(micros, Ordering::Relaxed);
        let i = BOUNDS.iter().position(|x| micros <= *x).unwrap();
        self.counts[i].fetch_add(1, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> Value {
        json!({"unit":"microseconds","bounds":BOUNDS,"bucket_counts":self.counts.iter().map(|x|x.load(Ordering::Relaxed)).collect::<Vec<_>>(),"sum":self.sum.load(Ordering::Relaxed)})
    }
}
#[derive(Default)]
pub struct RuntimeMetrics {
    pub sql_wait: Histogram,
    pub sql_execution: Histogram,
    pub ingest_latency: Histogram,
    pub delivery_latency: Histogram,
    pub db_rejected: AtomicU64,
    pub ingress_rejected: AtomicU64,
    pub stored: AtomicU64,
    pub duplicates: AtomicU64,
    pub capacity_rejected: AtomicU64,
    pub submitted: AtomicU64,
    pub failed: AtomicU64,
    pub reconcile: AtomicU64,
    pub retried: AtomicU64,
    pub worker_errors: AtomicU64,
}
impl RuntimeMetrics {
    pub fn snapshot(&self) -> Value {
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
        json!({"scope":"process_since_start","sql_wait":self.sql_wait.snapshot(),"sql_execution":self.sql_execution.snapshot(),"ingest_latency":self.ingest_latency.snapshot(),"delivery_latency":self.delivery_latency.snapshot(),"db_rejected":n(&self.db_rejected),"ingress_rejected":n(&self.ingress_rejected),"stored":n(&self.stored),"duplicates":n(&self.duplicates),"capacity_rejected":n(&self.capacity_rejected),"submitted":n(&self.submitted),"failed":n(&self.failed),"reconcile":n(&self.reconcile),"retried":n(&self.retried),"worker_errors":n(&self.worker_errors)})
    }
}
impl SqliteStore {
    /// Bounded metadata-only snapshot. No message bodies, tokens, recovery evidence or URLs.
    pub async fn operations_snapshot(&self, workspace: Uuid) -> Result<Value, PortError> {
        let path = self.path.clone();
        self.run(move|c|{
   let tx=c.transaction().map_err(db_error)?;let w=workspace.to_string();
   let (items,bytes,paused):(u64,u64,bool)=tx.query_row("SELECT COALESCE(SUM(items),0),COALESCE(SUM(bytes),0),COALESCE(MAX(paused),0) FROM capacity_state WHERE workspace=?1",[&w],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_error)?;
   let oldest:Option<i64>=tx.query_row("SELECT MIN(admitted_ms) FROM pending_work WHERE workspace=?1",[&w],|r|r.get(0)).map_err(db_error)?;
   let mut counts=serde_json::Map::new();
   for table in ["deliveries","routing_tasks"] {let mut q=tx.prepare(&format!("SELECT state,COUNT(*) FROM {table} WHERE workspace=?1 AND resolution IS NULL GROUP BY state")).map_err(db_error)?;let rows=q.query_map([&w],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?))).map_err(db_error)?;let mut m=serde_json::Map::new();for row in rows {let (s,n)=row.map_err(db_error)?;m.insert(s,json!(n));}counts.insert(table.into(),Value::Object(m));}
   let mut cases=Vec::new();
   {let mut q=tx.prepare("SELECT delivery_id,target_id,state,attempt,recovery_revision FROM deliveries WHERE workspace=?1 AND state IN ('failed','reconcile') AND resolution IS NULL ORDER BY rowid LIMIT 100").map_err(db_error)?;for row in q.query_map([&w],|r|Ok(json!({"kind":"delivery","delivery_id":r.get::<_,String>(0)?,"target_id":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"attempt":r.get::<_,u64>(3)?,"revision":r.get::<_,u64>(4)?}))).map_err(db_error)?{cases.push(row.map_err(db_error)?);}}
   {let mut q=tx.prepare("SELECT source,event_id,state,attempt,recovery_revision FROM routing_tasks WHERE workspace=?1 AND state='blocked' AND resolution IS NULL ORDER BY due_ms LIMIT 100").map_err(db_error)?;for row in q.query_map([&w],|r|Ok(json!({"kind":"route","source_id":r.get::<_,String>(0)?,"event_id":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"attempt":r.get::<_,u64>(3)?,"revision":r.get::<_,u64>(4)?}))).map_err(db_error)?{cases.push(row.map_err(db_error)?);}}
   let content:(u64,u64)=tx.query_row("SELECT bytes,items FROM content_usage WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
   let release:Option<u64>=tx.query_row("SELECT MAX(revision) FROM active_release",[],|r|r.get(0)).map_err(db_error)?;
   let size=|p:&std::path::Path|std::fs::metadata(p).ok().map(|x|x.len());
   let wal=std::path::PathBuf::from(format!("{}-wal",path.display()));
   Ok(json!({"schema_version":4,"active_config_revision":release,"content_reserved_bytes":content.0,"content_objects":content.1,"pending_items":items,"pending_bytes":bytes,"paused":paused,"oldest_pending_ms":oldest.map(|x|millis(SystemTime::now()).unwrap_or(x).saturating_sub(x)),"states":counts,"recovery_cases":cases,"case_limit_per_kind":100,"database_bytes":size(&path),"wal_bytes":size(&wal),"disk_free_bytes":super::capacity::available_space(&path).ok()}))
  }).await
    }
}
