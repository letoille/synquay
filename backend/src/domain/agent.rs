//! Durable analysis jobs. A completed job means reply queued, not externally delivered.
use super::{connector::ConnectionScope, delivery::Destination};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<super::command::CommandMapping>,
    pub profile_id: Uuid,
    pub lease_seconds: u32,
    pub deadline_seconds: u32,
    pub max_attempts: u32,
    pub max_in_flight: u32,
}
impl AgentConfig {
    pub fn valid(&self) -> bool {
        self.command.as_ref().is_none_or(|c| c.validate().is_ok())
            && !self.profile_id.is_nil()
            && (5..=300).contains(&self.lease_seconds)
            && (self.lease_seconds..=86400).contains(&self.deadline_seconds)
            && (1..=10).contains(&self.max_attempts)
            && (1..=32).contains(&self.max_in_flight)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct AgentBinding {
    pub source: ConnectionScope,
    pub destination: Destination,
    pub executor: Uuid,
    pub config: AgentConfig,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct JobSummary {
    pub job_id: Uuid,
    pub state: String,
    pub attempts: u32,
    pub deadline_ms: i64,
    pub delivery_id: Option<Uuid>,
    pub delivery_state: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct JobInput {
    pub summary: JobSummary,
    /// Inbound data is untrusted content, never execution authority.
    pub input: Option<Value>,
    pub source_event_id: Option<String>,
    pub event_type: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimToken {
    pub job_id: Uuid,
    pub claim_id: Uuid,
    pub generation: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ClaimedJob {
    pub token: ClaimToken,
    pub lease_until_ms: i64,
    pub job: JobInput,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CompletionReceipt {
    pub job_id: Uuid,
    pub delivery_id: Uuid,
    pub state: String,
    pub duplicate: bool,
}
