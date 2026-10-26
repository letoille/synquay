use super::{valid_id, valid_text};
use crate::domain::connector::ConnectionScope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    pub scope: ConnectionScope,
    pub id: String,
    pub revision: u64,
}
impl ResourceRef {
    pub fn is_valid(&self) -> bool {
        !self.scope.workspace_id.is_nil()
            && !self.scope.connection_id.is_nil()
            && valid_id(&self.id)
            && self.revision > 0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDefinition {
    pub reference: ResourceRef,
    pub resource_type: String,
    pub display_name: String,
    pub description: String,
}
impl ResourceDefinition {
    pub fn is_valid(&self) -> bool {
        self.reference.is_valid()
            && valid_id(&self.resource_type)
            && valid_text(&self.display_name, 256)
            && valid_text(&self.description, 8192)
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationOrigin {
    Live,
    Retained,
    Historical,
}

/// Receipt time is not proof of device freshness, especially for retained messages.
#[derive(Clone)]
pub struct StateObservation {
    pub value: Value,
    pub source_time: Option<SystemTime>,
    pub received_at: SystemTime,
    pub origin: ObservationOrigin,
}
