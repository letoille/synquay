use super::PortError;
use crate::domain::connector::ConnectionScope;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Created by trusted orchestration AFTER authentication and authorization.
/// Carrying this context does not itself perform authorization.
#[derive(Clone)]
pub struct OperationContext {
    pub scope: ConnectionScope,
    pub principal_id: Uuid,
    pub request_id: Uuid,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub route_trace: Vec<ConnectionScope>,
    pub deadline: Option<Instant>,
    pub cancellation: CancellationToken,
}

impl OperationContext {
    pub fn check_active(&self) -> Result<(), PortError> {
        if self.cancellation.is_cancelled() {
            return Err(PortError::Cancelled);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(PortError::DeadlineExceeded);
        }
        Ok(())
    }

    /// Cooperative stop notification. Adapters must still clean up child processes,
    /// subscriptions and uncertain side effects before returning.
    pub async fn stopped(&self) -> PortError {
        if let Err(error) = self.check_active() {
            return error;
        }
        match self.deadline {
            Some(deadline) => tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => PortError::Cancelled,
                _ = tokio::time::sleep_until(deadline.into()) => PortError::DeadlineExceeded,
            },
            None => {
                self.cancellation.cancelled().await;
                PortError::Cancelled
            }
        }
    }
}
