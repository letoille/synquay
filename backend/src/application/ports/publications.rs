//! Profile exposure drafts and atomic, idempotent publication. Connection changes
//! have a separate trust boundary; these operations only select existing tools.
use super::PortFuture;
use crate::domain::profile::PublishedProfile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone)]
pub struct PublicationScope {
    pub workspace_id: Uuid,
    pub profile_id: Uuid,
    pub actor_id: Uuid,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftInput {
    pub expected_generation: u64,
    pub base_revision: u64,
    pub exposed_tools: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ExposureDraft {
    pub generation: u64,
    pub base_revision: u64,
    pub exposed_tools: Vec<String>,
    pub actor_id: Uuid,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishInput {
    pub operation_id: Uuid,
    pub expected_revision: u64,
    pub draft_generation: u64,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PublicationReceipt {
    pub operation_id: Uuid,
    pub revision: u64,
    pub replayed: bool,
}
pub struct PublicationPlan {
    pub scope: PublicationScope,
    pub request: PublishInput,
    pub profile: PublishedProfile,
    pub snapshot: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PublicationEntry {
    pub revision: u64,
    pub created_ms: i64,
    pub actor_id: Uuid,
    pub exposed_tools: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PublicationHistory {
    pub items: Vec<PublicationEntry>,
    pub next_before_revision: Option<u64>,
}
pub trait ToolPublicationStore: Send + Sync {
    fn history(
        &self,
        scope: PublicationScope,
        before_revision: Option<u64>,
        limit: u32,
    ) -> PortFuture<'_, PublicationHistory>;
    fn draft(&self, scope: PublicationScope) -> PortFuture<'_, Option<ExposureDraft>>;
    fn save_draft(
        &self,
        scope: PublicationScope,
        input: DraftInput,
    ) -> PortFuture<'_, ExposureDraft>;
    fn published_operation(
        &self,
        scope: PublicationScope,
        request: PublishInput,
    ) -> PortFuture<'_, Option<PublicationReceipt>>;
    fn publish_draft(&self, plan: PublicationPlan) -> PortFuture<'_, PublicationReceipt>;
}
