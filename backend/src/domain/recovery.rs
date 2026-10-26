use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryTarget {
    Delivery { delivery_id: Uuid },
    Route { source_id: Uuid, event_id: Uuid },
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Abandon,
    Replay,
    ConfirmSubmitted,
}
/// Operator intent, not a claim that the provider has actually been queried.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCommand {
    pub command_id: Uuid,
    pub target: RecoveryTarget,
    pub expected_state: String,
    pub expected_revision: u64,
    pub action: RecoveryAction,
    pub reason: String,
    /// Required for unknown outcomes. Record a provider lookup/reference, never secrets.
    pub evidence: Option<String>,
    #[serde(default)]
    pub confirmed_not_submitted: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RecoveryReceipt {
    pub command_id: Uuid,
    pub new_delivery_id: Option<Uuid>,
}
