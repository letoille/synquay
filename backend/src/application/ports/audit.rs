use super::PortFuture;
use crate::domain::audit::AuditRecord;

pub trait AuditLog: Send + Sync {
    /// Durable append, independent of tracing levels. Protect integrity and retention.
    fn append(&self, record: AuditRecord) -> PortFuture<'_, ()>;
}
