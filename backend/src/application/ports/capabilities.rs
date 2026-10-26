//! Internal ports: callers must authorize before lookup; a reference is not a grant.
use super::{OperationContext, PortFuture};
use crate::domain::capability::{
    CapabilityDescriptor, CapabilityRef,
    binding::BoundCapability,
    resource::{ResourceDefinition, ResourceRef, StateObservation},
};
use std::sync::Arc;

pub trait CapabilityCatalog: Send + Sync {
    /// Exact immutable revision, never silently replaced by the newest version.
    fn descriptor(
        &self,
        context: OperationContext,
        reference: CapabilityRef,
    ) -> PortFuture<'_, Arc<CapabilityDescriptor>>;
    /// Execution-only configuration; never forward to an agent or public discovery API.
    fn binding(
        &self,
        context: OperationContext,
        reference: CapabilityRef,
    ) -> PortFuture<'_, Arc<BoundCapability>>;
}

pub trait ResourceCatalog: Send + Sync {
    fn resource(
        &self,
        context: OperationContext,
        reference: ResourceRef,
    ) -> PortFuture<'_, Arc<ResourceDefinition>>;
}

pub trait StateReader: Send + Sync {
    /// Authorize reads separately; return source timestamps and retained/history provenance.
    fn read(
        &self,
        context: OperationContext,
        reference: CapabilityRef,
    ) -> PortFuture<'_, Option<StateObservation>>;
}
