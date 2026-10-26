use super::{SqliteStore, db_error, decode, encode, millis};
use crate::application::ports::PortError;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path, time::SystemTime};
use uuid::Uuid;

pub const MAX_RECORD_BYTES: usize = 256 * 1024;
pub const MAX_DISPATCH_TARGETS: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CapacityLimits {
    pub max_pending_items: u64,
    pub max_pending_bytes: u64,
    pub resume_percent: u8,
    pub min_free_disk_bytes: u64,
    pub resume_free_disk_bytes: u64,
}
impl Default for CapacityLimits {
    fn default() -> Self {
        Self {
            max_pending_items: 10_000,
            max_pending_bytes: 256 * 1024 * 1024,
            resume_percent: 80,
            min_free_disk_bytes: 256 * 1024 * 1024,
            resume_free_disk_bytes: 512 * 1024 * 1024,
        }
    }
}
impl CapacityLimits {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.max_pending_items == 0
            || self.max_pending_items > i64::MAX as u64
            || self.max_pending_bytes == 0
            || self.max_pending_bytes > i64::MAX as u64
            || self.resume_percent == 0
            || self.resume_percent >= 100
            || self.resume_free_disk_bytes < self.min_free_disk_bytes
        {
            return Err(PortError::InvalidInput {
                code: "capacity_limits",
            });
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct BacklogStatus {
    pub pending_items: u64,
    pub pending_bytes: u64,
    pub paused: bool,
    pub oldest_admitted_ms: Option<i64>,
    pub blocked_routes: u64,
}

/// Bounded output allocation even when an internal adapter supplies a large Value.
pub(super) fn encode_bounded<T: Serialize>(value: &T) -> Result<String, PortError> {
    struct Buffer(Vec<u8>);
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_RECORD_BYTES - self.0.len() {
                return Err(std::io::Error::other("record size limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer(Vec::new());
    serde_json::to_writer(&mut buffer, value).map_err(|e| {
        if e.is_io() {
            PortError::PayloadTooLarge
        } else {
            PortError::Failed {
                code: "storage_encode",
            }
        }
    })?;
    String::from_utf8(buffer.0).map_err(|_| PortError::Failed {
        code: "storage_encode",
    })
}

pub(super) fn available_space(path: &Path) -> Result<u64, PortError> {
    #[cfg(unix)]
    {
        let stat = rustix::fs::statvfs(path).map_err(|_| PortError::Unavailable)?;
        Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(PortError::Unsupported)
    }
}

pub(super) fn reserve(
    tx: &Transaction<'_>,
    path: &Path,
    workspace: &str,
    key: &str,
    bytes: usize,
    admitted_ms: i64,
) -> Result<(), PortError> {
    let record: String = tx
        .query_row(
            "SELECT record FROM capacity_policy WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    let limits: CapacityLimits = decode(&record)?;
    limits.validate()?;
    if bytes as u64 > limits.max_pending_bytes {
        return Err(PortError::PayloadTooLarge);
    }
    tx.execute(
        "INSERT OR IGNORE INTO capacity_state(workspace) VALUES(?1)",
        [workspace],
    )
    .map_err(db_error)?;
    let (items, used, paused): (u64, u64, bool) = tx
        .query_row(
            "SELECT items,bytes,paused FROM capacity_state WHERE workspace=?1",
            [workspace],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db_error)?;
    let free = available_space(path)?;
    let low = |high: u64| ((high as u128 * limits.resume_percent as u128) / 100) as u64;
    let reason = if free < limits.min_free_disk_bytes {
        Some("disk_low")
    } else if paused
        && (items > low(limits.max_pending_items)
            || used > low(limits.max_pending_bytes)
            || free < limits.resume_free_disk_bytes)
    {
        Some("waiting_for_low_watermark")
    } else if items >= limits.max_pending_items
        || used.saturating_add(bytes as u64) > limits.max_pending_bytes
    {
        Some("backlog_full")
    } else {
        None
    };
    tx.execute(
        "UPDATE capacity_state SET paused=?2 WHERE workspace=?1",
        params![workspace, reason.is_some()],
    )
    .map_err(db_error)?;
    if let Some(reason) = reason {
        return Err(PortError::CapacityExceeded { reason });
    }
    tx.execute(
        "INSERT INTO pending_work(workspace,work_key,bytes,admitted_ms) VALUES(?1,?2,?3,?4)",
        params![workspace, key, bytes as i64, admitted_ms],
    )
    .map_err(db_error)?;
    Ok(())
}

pub(super) fn release_completed(
    tx: &Transaction<'_>,
    workspace: &str,
    key: &str,
) -> Result<(), PortError> {
    tx.execute("DELETE FROM pending_work WHERE workspace=?1 AND work_key=?2 AND NOT EXISTS(SELECT 1 FROM deliveries WHERE workspace=?1 AND dispatch_key=?2 AND state<>'submitted' AND resolution IS NULL)", params![workspace,key]).map_err(db_error)?;
    tx.execute("INSERT OR IGNORE INTO retired_work(workspace,work_key,retired_ms) SELECT ?1,?2,?3 WHERE NOT EXISTS(SELECT 1 FROM pending_work WHERE workspace=?1 AND work_key=?2)",params![workspace,key,millis(SystemTime::now())?]).map_err(db_error)?;
    Ok(())
}

impl SqliteStore {
    /// Operator startup configuration, shared by every connection via the database.
    /// Updates limits without discarding existing work; lower limits may pause new input.
    pub async fn configure_capacity(&self, limits: CapacityLimits) -> Result<(), PortError> {
        limits.validate()?;
        self.run_critical(move |c| {
            c.execute(
                "UPDATE capacity_policy SET record=?1 WHERE singleton=1",
                [encode(&limits)?],
            )
            .map_err(db_error)?;
            Ok(())
        })
        .await
    }
    pub async fn backlog_status(&self, workspace: Uuid) -> Result<BacklogStatus, PortError> {
        self.run(move |c| status(c, workspace)).await
    }
    pub async fn record_routing_failure(
        &self,
        scope: crate::domain::connector::ConnectionScope,
        id: Uuid,
        error: PortError,
    ) -> Result<(), PortError> {
        if error == PortError::Cancelled {
            return Ok(());
        }
        self.run_critical(move |c| {
            let transient = matches!(error,PortError::Unavailable|PortError::RateLimited {..}|PortError::CapacityExceeded {..}|PortError::OutcomeUnknown|PortError::DeadlineExceeded|PortError::Conflict);
            let now = millis(SystemTime::now())?;
            // State guard prevents late failure reports from reopening a completed dispatch.
            c.execute("UPDATE routing_tasks SET attempt=attempt+1,state=CASE WHEN ?4 AND attempt<7 THEN 'retry_wait' ELSE 'blocked' END,due_ms=?5+MIN(300000,1000*(1 << MIN(attempt+1,8))),error_code=?6 WHERE workspace=?1 AND source=?2 AND event_id=?3 AND state IN ('ready','retry_wait')",
                params![scope.workspace_id.to_string(),scope.connection_id.to_string(),id.to_string(),transient,now,if transient {"routing_transient"} else {"routing_invalid"}]).map_err(db_error)?;
            Ok(())
        }).await
    }
}
fn status(c: &Connection, workspace: Uuid) -> Result<BacklogStatus, PortError> {
    let w = workspace.to_string();
    let (items, bytes, paused) = c
        .query_row(
            "SELECT items,bytes,paused FROM capacity_state WHERE workspace=?1",
            [&w],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?
        .unwrap_or((0, 0, false));
    Ok(BacklogStatus {
        pending_items: items,
        pending_bytes: bytes,
        paused,
        oldest_admitted_ms: c
            .query_row(
                "SELECT MIN(admitted_ms) FROM pending_work WHERE workspace=?1",
                [&w],
                |r| r.get(0),
            )
            .map_err(db_error)?,
        blocked_routes: c
            .query_row(
                "SELECT COUNT(*) FROM routing_tasks WHERE workspace=?1 AND state='blocked' AND resolution IS NULL",
                [&w],
                |r| r.get(0),
            )
            .map_err(db_error)?,
    })
}
