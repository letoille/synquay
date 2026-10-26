use super::PortError;
use crate::domain::{
    connection_routing::{RouteBinding, RoutingDescriptor},
    event::EventEnvelope,
    message::Message,
};

/// Verified external actor plus the connection's current receive permission.
pub struct ConnectionSender {
    pub namespace: String,
    pub id: String,
    pub allowed: bool,
}

/// Implement alongside a connector. Validation, matching and dispatch use the same contract.
/// Context is supplied by the authenticated adapter, never extracted from message content.
pub trait ConnectionRoutingAdapter: Send + Sync {
    /// Account namespace used for durable, connection-scoped discovery.
    fn discovery_account(&self) -> Option<String> {
        None
    }
    fn supports_reply(&self) -> bool {
        false
    }
    fn validate_command_senders(&self, _senders: &[String]) -> Result<(), PortError> {
        Err(PortError::Unsupported)
    }
    fn authenticated_sender(
        &self,
        _context: Option<&RouteBinding>,
    ) -> Result<ConnectionSender, PortError> {
        Err(PortError::Unsupported)
    }
    /// Reply formatting and passive-send quotas remain protocol-owned.
    fn command_reply(
        &self,
        _event: &EventEnvelope,
        _text: String,
        _consumed_target: Option<&RouteBinding>,
    ) -> Result<Message, PortError> {
        Err(PortError::Unsupported)
    }
    fn source_descriptor(&self) -> RoutingDescriptor;
    fn target_descriptor(&self) -> RoutingDescriptor;
    fn validate_source(&self, binding: Option<&RouteBinding>) -> Result<(), PortError>;
    fn validate_target(
        &self,
        binding: Option<&RouteBinding>,
        same_connection: bool,
    ) -> Result<(), PortError>;
    fn matches(&self, binding: Option<&RouteBinding>, context: Option<&RouteBinding>) -> bool;
    fn resolve_target(
        &self,
        binding: Option<&RouteBinding>,
        event: &EventEnvelope,
        same_connection: bool,
    ) -> Result<Option<RouteBinding>, PortError>;
}
