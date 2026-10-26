use crate::application::ports::PortError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SchedulerConfig {
    pub max_in_flight: usize,
    pub min_interval_ms: u64,
    pub failure_threshold: u32,
    pub cooldown_ms: u64,
    /// Conservative whole-target FIFO, including unresolved failed/unknown work.
    pub strict_order: bool,
}
impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 8,
            min_interval_ms: 100,
            failure_threshold: 3,
            cooldown_ms: 30_000,
            strict_order: false,
        }
    }
}
impl SchedulerConfig {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.max_in_flight == 0
            || self.max_in_flight > 32
            || self.min_interval_ms > 60_000
            || self.failure_threshold == 0
            || self.failure_threshold > 100
            || self.cooldown_ms == 0
            || self.cooldown_ms > 3_600_000
        {
            Err(PortError::InvalidInput {
                code: "scheduler_config",
            })
        } else {
            Ok(())
        }
    }
}
pub(super) struct TargetLane {
    pub id: Uuid,
    pub gate: Mutex<()>,
    pub state: Mutex<TargetState>,
}
pub(super) struct TargetState {
    pub failures: u32,
    pub open_until: Option<Instant>,
    pub next_start: Instant,
    pub starts: u64,
}
impl TargetLane {
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
            gate: Mutex::new(()),
            state: Mutex::new(TargetState {
                failures: 0,
                open_until: None,
                next_start: Instant::now(),
                starts: 0,
            }),
        }
    }
}
impl TargetState {
    pub fn complete(&mut self, success: bool, config: &SchedulerConfig) {
        if success {
            self.failures = 0;
            self.open_until = None;
        } else {
            self.failures = self.failures.saturating_add(1);
            if self.failures >= config.failure_threshold {
                self.open_until = Some(Instant::now() + Duration::from_millis(config.cooldown_ms));
            }
        }
    }
}
