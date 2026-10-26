use super::connector::ConnectionScope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Server-issued reference, never a caller-provided filesystem path or fetch URL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRef {
    pub id: Uuid,
    pub scope: ConnectionScope,
    pub size: u64,
    pub sha256: String,
    pub media_type: String,
}
