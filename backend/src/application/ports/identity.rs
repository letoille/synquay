use super::{OperationContext, PortError, PortFuture};
use crate::domain::identity::{Action, Identity, Resource};
use std::time::SystemTime;

/// Never derive Debug or include this in logs.
pub struct Credentials {
    pub bearer_token: String,
}

pub trait Authenticator: Send + Sync {
    /// Must verify issuer, audience, signature/introspection, expiry and tenant binding.
    fn authenticate(&self, credentials: Credentials) -> PortFuture<'_, Identity>;
}

pub trait PolicyEngine: Send + Sync {
    /// Default deny; combine subject grants, OAuth scopes, resource ownership and client limits.
    fn authorize(
        &self,
        identity: Identity,
        action: Action,
        resource: Resource,
    ) -> PortFuture<'_, ()>;
}

#[derive(Clone)]
pub struct AccessContext {
    pub identity: Identity,
    pub operation: OperationContext,
}
impl AccessContext {
    pub fn check(&self) -> Result<(), PortError> {
        self.operation.check_active()?;
        if self.identity.workspace_id != self.operation.scope.workspace_id
            || self.identity.principal_id != self.operation.principal_id
        {
            return Err(PortError::ScopeMismatch);
        }
        if self
            .identity
            .expires_at
            .is_some_and(|t| t <= SystemTime::now())
        {
            return Err(PortError::PermissionDenied);
        }
        Ok(())
    }
}
