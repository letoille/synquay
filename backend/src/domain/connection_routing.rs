//! Versioned adapter-owned routing data. Message content cannot choose a destination.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteBinding {
    pub adapter: String,
    pub version: u16,
    pub config: Value,
}
impl RouteBinding {
    pub fn new(adapter: &str, config: Value) -> Self {
        Self {
            adapter: adapter.into(),
            version: 1,
            config,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRouting {
    #[serde(default)]
    pub source: Option<RouteBinding>,
    #[serde(default)]
    pub targets: BTreeMap<Uuid, RouteBinding>,
}
/// Declarative editor contract. Only these controls are supported; no executable UI.
#[derive(Clone, Serialize)]
pub struct RoutingDescriptor {
    pub adapter: &'static str,
    pub version: u16,
    pub modes: Vec<RoutingMode>,
}
#[derive(Clone, Serialize)]
pub struct RoutingMode {
    pub id: &'static str,
    pub label: &'static str,
    pub fields: Vec<RoutingField>,
}
#[derive(Clone, Serialize)]
pub struct RoutingField {
    pub key: &'static str,
    pub label: &'static str,
    pub control: &'static str,
    pub conversation_kind: Option<&'static str>,
}
