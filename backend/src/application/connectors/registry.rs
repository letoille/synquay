use crate::{
    application::ports::{
        PortError,
        events::{EventSource, InboundDecoder},
        lifecycle::ConnectorLifecycle,
        messages::MessageSink,
        tools::ToolExecutor,
    },
    domain::connector::{Capability, ConnectionScope, ConnectorDescriptor},
};
use std::{
    collections::{HashMap, hash_map::Entry},
    sync::{Arc, RwLock},
};

/// Optional capability facets: a source-only connector need not fake tool execution.
pub struct ConnectorRegistration {
    pub descriptor: ConnectorDescriptor,
    pub decoder: Option<Arc<dyn InboundDecoder>>,
    pub source: Option<Arc<dyn EventSource>>,
    pub tools: Option<Arc<dyn ToolExecutor>>,
    pub sink: Option<Arc<dyn MessageSink>>,
    pub lifecycle: Option<Arc<dyn ConnectorLifecycle>>,
}

impl ConnectorRegistration {
    pub fn new(descriptor: ConnectorDescriptor) -> Self {
        Self {
            descriptor,
            decoder: None,
            source: None,
            tools: None,
            sink: None,
            lifecycle: None,
        }
    }

    /// Derived from actual implementations so declaration and dispatch cannot diverge.
    pub fn capabilities(&self) -> Vec<Capability> {
        [
            (self.decoder.is_some(), Capability::DecodeInbound),
            (self.source.is_some(), Capability::ReceiveEvents),
            (self.tools.is_some(), Capability::ExecuteTools),
            (self.sink.is_some(), Capability::DeliverMessages),
            (self.lifecycle.is_some(), Capability::Lifecycle),
        ]
        .into_iter()
        .filter_map(|(present, capability)| present.then_some(capability))
        .collect()
    }
}

/// Immutable instances with additive registration; not an authorization service.
#[derive(Default)]
pub struct ConnectorRegistry {
    connections: RwLock<HashMap<ConnectionScope, Arc<ConnectorRegistration>>>,
}

impl ConnectorRegistry {
    pub fn register(&mut self, registration: ConnectorRegistration) -> Result<(), PortError> {
        self.register_shared(registration)
    }

    /// Add an immutable instance. Callers coordinate publication separately.
    pub fn register_shared(&self, registration: ConnectorRegistration) -> Result<(), PortError> {
        if registration.capabilities().is_empty() {
            return Err(PortError::InvalidInput {
                code: "no_connector_capabilities",
            });
        }
        match self
            .connections
            .write()
            .map_err(|_| PortError::Unavailable)?
            .entry(registration.descriptor.scope)
        {
            Entry::Vacant(entry) => {
                entry.insert(Arc::new(registration));
                Ok(())
            }
            Entry::Occupied(_) => Err(PortError::AlreadyRegistered),
        }
    }

    /// Replace or remove a registration under the caller's exclusive publication gate.
    /// Existing Arc holders finish on their admitted instance.
    pub fn replace_shared(
        &self,
        scope: ConnectionScope,
        next: Option<ConnectorRegistration>,
    ) -> Result<(), PortError> {
        if next
            .as_ref()
            .is_some_and(|r| r.descriptor.scope != scope || r.capabilities().is_empty())
        {
            return Err(PortError::ScopeMismatch);
        }
        let mut connections = self
            .connections
            .write()
            .map_err(|_| PortError::Unavailable)?;
        if !connections.contains_key(&scope) {
            return Err(PortError::NotFound);
        }
        if let Some(next) = next {
            connections.insert(scope, Arc::new(next));
        } else {
            connections.remove(&scope);
        }
        Ok(())
    }
    pub fn get(&self, scope: ConnectionScope) -> Result<Arc<ConnectorRegistration>, PortError> {
        self.connections
            .read()
            .map_err(|_| PortError::Unavailable)?
            .get(&scope)
            .cloned()
            .ok_or(PortError::NotFound)
    }
}
