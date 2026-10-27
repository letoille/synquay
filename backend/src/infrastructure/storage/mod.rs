//! SQLite v19 schema; v1 record encodings remain readable. Not a public wire DTO.
pub mod agent;
mod audit;
pub mod capacity;
pub mod checkpoints;
#[cfg(test)]
mod concurrency_tests;
pub(crate) mod connections;
mod content;
mod events;
pub mod maintenance;
mod mcp_catalog;
mod mcp_servers;
pub mod metrics;
mod outbox;
mod publications;
mod record;
mod recovery;
mod routes;
mod scheduling;
mod tools;
mod workspaces;

use crate::application::ports::PortError;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;

const DATABASE_ADMISSION_LIMIT: usize = 64;
const DATABASE_CRITICAL_LIMIT: usize = 8;

#[derive(Clone)]
pub struct SqliteStore {
    pub(crate) device_io: Arc<tokio::sync::RwLock<()>>,
    connection: Arc<Mutex<Connection>>,
    scheduler: Arc<scheduling::Scheduler>,
    admission: Arc<Semaphore>,
    critical_admission: Arc<Semaphore>,
    path: Arc<PathBuf>,
    pub metrics: Arc<metrics::RuntimeMetrics>,
}

fn db_error(_: rusqlite::Error) -> PortError {
    PortError::Unavailable
}
fn encode<T: Serialize>(value: &T) -> Result<String, PortError> {
    serde_json::to_string(value).map_err(|_| PortError::Failed {
        code: "storage_encode",
    })
}
fn decode<T: DeserializeOwned>(value: &str) -> Result<T, PortError> {
    serde_json::from_str(value).map_err(|_| PortError::Failed {
        code: "storage_decode",
    })
}
fn millis(time: SystemTime) -> Result<i64, PortError> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .ok_or(PortError::InvalidInput {
            code: "invalid_timestamp",
        })
}
impl SqliteStore {
    pub(crate) fn database_path(&self) -> &Path {
        &self.path
    }
    /// Bind a database to immutable routing IDs/target. Secret rotation is independent.
    pub async fn bind_pipeline(&self, binding: serde_json::Value) -> Result<(), PortError> {
        self.run(move |c| {
            let record = encode(&binding)?;
            let tx = c
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            let previous: Option<String> = tx
                .query_row(
                    "SELECT record FROM pipeline_binding WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if let Some(previous) = previous {
                if previous != record {
                    return Err(PortError::InvalidInput {
                        code: "pipeline_binding_changed",
                    });
                }
            } else {
                tx.execute(
                    "INSERT INTO pipeline_binding(singleton,record) VALUES(1,?1)",
                    [record],
                )
                .map_err(db_error)?;
            }
            tx.execute("INSERT OR IGNORE INTO config_releases(record,created_ms) SELECT record,?1 FROM pipeline_binding", [millis(SystemTime::now())?]).map_err(db_error)?;
            tx.execute("INSERT OR IGNORE INTO active_release SELECT 1,MAX(revision) FROM config_releases",[]).map_err(db_error)?;
            tx.commit().map_err(|_| PortError::OutcomeUnknown)
        })
        .await
    }
    /// Activate a sanitized runtime snapshot after immutable identity/target binding validation.
    /// Reverting to a known snapshot reuses its revision; no historical jobs are rewritten.
    pub async fn activate_release(&self, runtime: serde_json::Value) -> Result<u64, PortError> {
        self.run_critical(move|c| {
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db_error)?;
            let binding:String=tx.query_row("SELECT record FROM pipeline_binding WHERE singleton=1",[],|r|r.get(0)).optional().map_err(db_error)?.ok_or(PortError::Conflict)?;
            let binding:serde_json::Value=decode(&binding)?;
            // Never silently orphan queued events by removing a previously active ingress.
            let previous:Option<String>=tx.query_row("SELECT record FROM config_releases JOIN active_release USING(revision) WHERE singleton=1",[],|r|r.get(0)).optional().map_err(db_error)?;
            if let Some(previous)=previous {
                let previous:serde_json::Value=decode(&previous)?;
                if let Some(sources)=previous.pointer("/runtime/sources").and_then(|v|v.as_array()) {
                    let current=runtime.get("sources").and_then(|v|v.as_array());
                    if sources.iter().any(|old| !current.is_some_and(|new|new.iter().any(|s|s["connection_id"]==old["connection_id"]))) {
                        return Err(PortError::InvalidInput{code:"ingress_removal_requires_drain"});
                    }
                }
            }
            let record=capacity::encode_bounded(&serde_json::json!({"binding":binding,"runtime":runtime}))?;
            tx.execute("INSERT OR IGNORE INTO config_releases(record,created_ms) VALUES(?1,?2)",params![record,millis(SystemTime::now())?]).map_err(db_error)?;
            let revision=tx.query_row("SELECT revision FROM config_releases WHERE record=?1",[record],|r|r.get::<_,u64>(0)).map_err(db_error)?;
            tx.execute("INSERT INTO active_release VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision",[revision]).map_err(db_error)?;
            tx.commit().map_err(|_|PortError::OutcomeUnknown)?;
            Ok(revision)
        }).await
    }
    /// Opens an existing parent directory; caller controls filesystem ownership/permissions.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let path = path.as_ref().to_owned();
        if path.as_os_str().is_empty()
            || path == Path::new(":memory:")
            || path.to_string_lossy().starts_with("file:")
        {
            return Err(PortError::InvalidInput {
                code: "persistent_database_path_required",
            });
        }
        tokio::task::spawn_blocking(move || {
            // Newly created files contain private event payloads. Existing ownership is not changed.
            let mut options = std::fs::OpenOptions::new();
            options.create(true).write(true).truncate(false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&path).map_err(|_| PortError::Unavailable)?;
            let mut connection = Connection::open(&path).map_err(db_error)?;
            connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(db_error)?;
            connection
                .execute_batch(
                    "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
                )
                .map_err(db_error)?;
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            let version: i64 = tx
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .map_err(db_error)?;
            match version {
                0 => {
                    tx.execute_batch(include_str!("schema.sql"))
                        .map_err(db_error)?;
                    tx.execute_batch(include_str!("migration_v2.sql"))
                        .map_err(db_error)?;
                }
                1 => tx
                    .execute_batch(include_str!("migration_v2.sql"))
                    .map_err(db_error)?,
                2..=20 => {}
                _ => return Err(PortError::Unsupported),
            }
            if version < 3 {
                tx.execute_batch(include_str!("migration_v3.sql"))
                    .map_err(db_error)?;
            }
            if version < 4 {
                tx.execute_batch(include_str!("migration_v4.sql"))
                    .map_err(db_error)?;
            }
            if version < 5 {
                tx.execute_batch(include_str!("migration_v5.sql"))
                    .map_err(db_error)?;
            }
            if version < 6 {
                tx.execute_batch(include_str!("migration_v6.sql"))
                    .map_err(db_error)?;
            }
            if version < 7 {
                tx.execute_batch(include_str!("migration_v7.sql"))
                    .map_err(db_error)?;
            }
            if version < 8 {
                tx.execute_batch(include_str!("migration_v8.sql"))
                    .map_err(db_error)?;
            }
            if version < 9 {
                tx.execute_batch(include_str!("migration_v9.sql"))
                    .map_err(db_error)?;
            }
            if version < 10 {
                tx.execute_batch(include_str!("migration_v10.sql"))
                    .map_err(db_error)?;
            }
            if version < 11 {
                tx.execute_batch(include_str!("migration_v11.sql"))
                    .map_err(db_error)?;
            }
            if version < 12 {
                tx.execute_batch(include_str!("migration_v12.sql"))
                    .map_err(db_error)?;
            }
            if version < 13 {
                tx.execute_batch(include_str!("migration_v13.sql"))
                    .map_err(db_error)?;
            }
            if version < 14 {
                tx.execute_batch(include_str!("migration_v14.sql"))
                    .map_err(db_error)?;
            }
            if version < 15 {
                tx.execute_batch(include_str!("migration_v15.sql"))
                    .map_err(db_error)?;
            }
            if version < 16 {
                tx.execute_batch(include_str!("migration_v16.sql"))
                    .map_err(db_error)?;
            }
            if version < 17 {
                tx.execute_batch(include_str!("migration_v17.sql"))
                    .map_err(db_error)?;
            }
            tx.execute(
                "INSERT OR IGNORE INTO capacity_policy(singleton,record) VALUES(1,?1)",
                [encode(&capacity::CapacityLimits::default())?],
            )
            .map_err(db_error)?;
            if version < 18 {
                tx.execute_batch(include_str!("migration_v18.sql"))
                    .map_err(db_error)?;
            }
            if version < 19 {
                tx.execute_batch(include_str!("migration_v19.sql"))
                    .map_err(db_error)?;
            }
            if version < 20 {
                tx.execute_batch(include_str!("migration_v20.sql"))
                    .map_err(db_error)?;
            }
            tx.commit().map_err(db_error)?;
            Ok(Self {
                device_io: device_state::gate(&path),
                connection: Arc::new(Mutex::new(connection)),
                scheduler: Arc::new(scheduling::Scheduler::default()),
                admission: Arc::new(Semaphore::new(DATABASE_ADMISSION_LIMIT)),
                critical_admission: Arc::new(Semaphore::new(DATABASE_CRITICAL_LIMIT)),
                path: Arc::new(path),
                metrics: Arc::new(metrics::RuntimeMetrics::default()),
            })
        })
        .await
        .map_err(|_| PortError::Unavailable)?
    }
    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection) -> Result<T, PortError> + Send + 'static,
    ) -> Result<T, PortError> {
        self.run_with_gate(false, work).await
    }
    async fn run_critical<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection) -> Result<T, PortError> + Send + 'static,
    ) -> Result<T, PortError> {
        self.run_with_gate(true, work).await
    }
    async fn run_with_gate<T: Send + 'static>(
        &self,
        critical: bool,
        work: impl FnOnce(&mut Connection) -> Result<T, PortError> + Send + 'static,
    ) -> Result<T, PortError> {
        // Bound retained closures/payloads before waiting for the connection. Waiting
        // uses async tasks, not blocking-pool threads parked on the same mutex.
        let admission = (if critical {
            &self.critical_admission
        } else {
            &self.admission
        })
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            self.metrics
                .db_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            PortError::RateLimited {
                retry_after: Duration::from_secs(1),
            }
        })?;
        let queued_at = Instant::now();
        let (turn, mut connection) = tokio::time::timeout(Duration::from_secs(5), async {
            let turn = self.scheduler.acquire(critical).await;
            (turn, self.connection.clone().lock_owned().await)
        })
        .await
        .map_err(|_| PortError::Unavailable)?;
        let queue_wait = queued_at.elapsed();
        let metrics = self.metrics.clone();
        let span = tracing::Span::current();
        tokio::task::spawn_blocking(move || {
            // Own both permits until SQL finishes, even if the awaiting caller is
            // dropped. Cancellation must not release capacity for a live commit.
            let _admission = admission;
            let _turn = turn;
            let _span = span.enter();
            let started = Instant::now();
            let result = work(&mut connection);
            metrics.sql_wait.observe(queue_wait.as_micros() as u64);
            metrics
                .sql_execution
                .observe(started.elapsed().as_micros() as u64);
            tracing::debug!(
                event = "storage.operation",
                critical,
                queue_wait_us = queue_wait.as_micros() as u64,
                execution_us = started.elapsed().as_micros() as u64,
                "SQLite operation completed"
            );
            result
        })
        .await
        .map_err(|_| PortError::OutcomeUnknown)?
    }
    /// Operational inspection; this is not an unauthenticated API endpoint.
    pub async fn delivery_state(&self, workspace: Uuid, id: Uuid) -> Result<String, PortError> {
        self.run(move |c| {
            c.query_row(
                "SELECT state FROM deliveries WHERE workspace=?1 AND delivery_id=?2",
                params![workspace.to_string(), id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PortError::NotFound)
        })
        .await
    }
}

mod connection_secrets;
pub mod device_state;
mod message_connections;

pub mod conversations;
