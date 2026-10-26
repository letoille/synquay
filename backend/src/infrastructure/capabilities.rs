//! Immutable startup catalog. Persistent discovery and dynamic resource grants are separate.
use crate::{
    application::ports::{
        OperationContext, PortError, PortFuture,
        capabilities::{CapabilityCatalog, ResourceCatalog},
    },
    domain::capability::{
        CapabilityDescriptor, CapabilityRef,
        binding::BoundCapability,
        resource::{ResourceDefinition, ResourceRef},
    },
};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct StaticCapabilityCatalog {
    bindings: HashMap<CapabilityRef, Arc<BoundCapability>>,
    descriptors: HashMap<CapabilityRef, Arc<CapabilityDescriptor>>,
    resources: HashMap<ResourceRef, Arc<ResourceDefinition>>,
}
impl StaticCapabilityCatalog {
    pub fn build(
        bindings: Vec<BoundCapability>,
        resources: Vec<ResourceDefinition>,
    ) -> Result<Self, PortError> {
        if bindings.len() > 1024 || resources.len() > 1024 {
            return Err(PortError::PayloadTooLarge);
        }
        let invalid = || PortError::InvalidInput {
            code: "invalid_capability_catalog",
        };
        let mut catalog = Self::default();
        for resource in resources {
            if !resource.is_valid() {
                return Err(invalid());
            }
            if catalog
                .resources
                .insert(resource.reference.clone(), Arc::new(resource))
                .is_some()
            {
                return Err(PortError::AlreadyRegistered);
            }
        }
        for binding in bindings {
            if !binding.is_valid()
                || binding
                    .descriptor
                    .resource
                    .as_ref()
                    .is_some_and(|r| !catalog.resources.contains_key(r))
            {
                return Err(invalid());
            }
            let key = binding.descriptor.reference.clone();
            if catalog.bindings.contains_key(&key) {
                return Err(PortError::AlreadyRegistered);
            }
            catalog
                .descriptors
                .insert(key.clone(), Arc::new(binding.descriptor.clone()));
            catalog.bindings.insert(key, Arc::new(binding));
        }
        Ok(catalog)
    }
}
impl CapabilityCatalog for StaticCapabilityCatalog {
    fn descriptor(
        &self,
        ctx: OperationContext,
        key: CapabilityRef,
    ) -> PortFuture<'_, Arc<CapabilityDescriptor>> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != key.scope {
                return Err(PortError::ScopeMismatch);
            }
            self.descriptors
                .get(&key)
                .cloned()
                .ok_or(PortError::NotFound)
        })
    }
    fn binding(
        &self,
        ctx: OperationContext,
        key: CapabilityRef,
    ) -> PortFuture<'_, Arc<BoundCapability>> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != key.scope {
                return Err(PortError::ScopeMismatch);
            }
            self.bindings.get(&key).cloned().ok_or(PortError::NotFound)
        })
    }
}
impl ResourceCatalog for StaticCapabilityCatalog {
    fn resource(
        &self,
        ctx: OperationContext,
        key: ResourceRef,
    ) -> PortFuture<'_, Arc<ResourceDefinition>> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != key.scope {
                return Err(PortError::ScopeMismatch);
            }
            self.resources.get(&key).cloned().ok_or(PortError::NotFound)
        })
    }
}
