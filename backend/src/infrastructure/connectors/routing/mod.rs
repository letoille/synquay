//! The only connector dispatch point. New protocols implement the port and register here.
mod mqtt;
mod qq;
use super::message_config::{ConnectionConfig, invalid};
use crate::{
    application::ports::{PortError, connection_routing::ConnectionRoutingAdapter},
    domain::{connection_routing::*, event::EventEnvelope},
};
use serde::de::DeserializeOwned;
use serde_json::json;

pub fn adapter(config: &ConnectionConfig) -> Box<dyn ConnectionRoutingAdapter + '_> {
    match config {
        ConnectionConfig::QqBot(c) => Box::new(qq::QqRouting(c)),
        ConnectionConfig::Mqtt(c) => Box::new(mqtt::MqttRouting(c)),
        _ => Box::new(FixedRouting),
    }
}
pub(super) fn decode<T: DeserializeOwned>(b: &RouteBinding, id: &str) -> Result<T, PortError> {
    if b.adapter != id || b.version != 1 {
        return Err(invalid("unsupported_route_binding"));
    }
    serde_json::from_value(b.config.clone()).map_err(|_| invalid("invalid_route_binding"))
}
pub(super) fn mode(
    id: &'static str,
    label: &'static str,
    fields: Vec<RoutingField>,
) -> RoutingMode {
    RoutingMode { id, label, fields }
}
pub(super) fn field(
    key: &'static str,
    label: &'static str,
    control: &'static str,
    kind: Option<&'static str>,
) -> RoutingField {
    RoutingField {
        key,
        label,
        control,
        conversation_kind: kind,
    }
}
struct FixedRouting;
impl ConnectionRoutingAdapter for FixedRouting {
    fn source_descriptor(&self) -> RoutingDescriptor {
        RoutingDescriptor {
            adapter: "fixed",
            version: 1,
            modes: vec![mode("default", "全部消息", vec![])],
        }
    }
    fn target_descriptor(&self) -> RoutingDescriptor {
        RoutingDescriptor {
            adapter: "fixed",
            version: 1,
            modes: vec![mode("default", "连接地址", vec![])],
        }
    }
    fn validate_source(&self, b: Option<&RouteBinding>) -> Result<(), PortError> {
        if b.is_some_and(|b| {
            b.adapter != "fixed" || b.version != 1 || b.config != json!({"mode":"default"})
        }) {
            return Err(invalid("unsupported_route_binding"));
        }
        Ok(())
    }
    fn validate_target(&self, b: Option<&RouteBinding>, _: bool) -> Result<(), PortError> {
        self.validate_source(b)
    }
    fn matches(&self, b: Option<&RouteBinding>, _: Option<&RouteBinding>) -> bool {
        self.validate_source(b).is_ok()
    }
    fn resolve_target(
        &self,
        b: Option<&RouteBinding>,
        _: &EventEnvelope,
        _: bool,
    ) -> Result<Option<RouteBinding>, PortError> {
        self.validate_source(b)?;
        Ok(None)
    }
}
