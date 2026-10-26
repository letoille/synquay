use super::PortFuture;
use crate::domain::profile::PublishedProfile;
use serde_json::Value;
use uuid::Uuid;

pub trait ProfileRepository: Send + Sync {
    fn published(&self, workspace_id: Uuid, profile_id: Uuid) -> PortFuture<'_, PublishedProfile>;
}

pub trait SchemaValidator: Send + Sync {
    /// Enforce the supported JSON Schema dialect; unsupported schemas fail closed.
    fn validate<'a>(&'a self, schema: &'a Value, value: &'a Value) -> PortFuture<'a, ()>;
}
