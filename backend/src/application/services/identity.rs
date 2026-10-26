use crate::application::ports::{
    OperationContext, PortError,
    identity::{AccessContext, Authenticator, Credentials},
};
use std::sync::Arc;

pub struct IdentityService {
    authenticator: Arc<dyn Authenticator>,
}
impl IdentityService {
    pub fn new(authenticator: Arc<dyn Authenticator>) -> Self {
        Self { authenticator }
    }

    #[tracing::instrument(skip_all, fields(request_id = %operation.request_id))]
    pub async fn authenticate(
        &self,
        credentials: Credentials,
        mut operation: OperationContext,
    ) -> Result<AccessContext, PortError> {
        operation.check_active()?;
        let identity = self.authenticator.authenticate(credentials).await?;
        if identity.workspace_id != operation.scope.workspace_id {
            return Err(PortError::ScopeMismatch);
        }
        // Principal comes from verified credentials, never a request-body field.
        operation.principal_id = identity.principal_id;
        let context = AccessContext {
            identity,
            operation,
        };
        context.check()?;
        Ok(context)
    }
}
