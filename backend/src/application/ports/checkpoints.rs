use super::PortFuture;
use crate::domain::connector::ConnectionScope;
use serde::{Deserialize, Serialize};

/// Opaque source cursor. Only the source adapter may interpret or order it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub partition: String,
    pub generation: u64,
    pub revision: u64,
    pub cursor: Option<String>,
}
pub trait SourceCheckpoints: Send + Sync {
    /// CAS takeover fences the previous source process. None means first activation.
    fn activate(
        &self,
        scope: ConnectionScope,
        partition: String,
        expected_generation: Option<u64>,
    ) -> PortFuture<'_, Checkpoint>;
    fn checkpoint(
        &self,
        scope: ConnectionScope,
        partition: String,
    ) -> PortFuture<'_, Option<Checkpoint>>;
}
