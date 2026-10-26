use super::{OperationContext, PortFuture};
use crate::domain::connector::HealthReport;

pub trait ConnectorLifecycle: Send + Sync {
    fn health(&self, context: OperationContext) -> PortFuture<'_, HealthReport>;
    /// Use a fresh cleanup deadline/token after cancelling the source's run token.
    /// Shutdown must be safe to repeat; it is distinct from cancellation of a single call.
    fn shutdown(&self, context: OperationContext) -> PortFuture<'_, ()>;
}
