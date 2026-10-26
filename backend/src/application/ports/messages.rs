use super::{OperationContext, PortFuture};
use crate::domain::message::{DeliveryReceipt, DeliveryRequest};

/// For MQTT publish, HTTP callbacks, chat, email, etc. No exactly-once guarantee.
/// Keep delivery_id stable on retries and report Accepted vs Delivered honestly.
/// Non-idempotent ambiguous results must be reconciled, not blindly retried.
pub trait MessageSink: Send + Sync {
    /// Check negotiated shape before recording Sending. Default adapters support inline text/JSON only.
    fn validate(&self, message: &crate::domain::message::Message) -> Result<(), super::PortError> {
        if message.parts.iter().any(|p| {
            matches!(
                p,
                crate::domain::message::ContentPart::StoredContent(_)
                    | crate::domain::message::ContentPart::Attachment { .. }
            )
        }) {
            Err(super::PortError::Unsupported)
        } else {
            Ok(())
        }
    }
    fn deliver(
        &self,
        context: OperationContext,
        request: DeliveryRequest,
    ) -> PortFuture<'_, DeliveryReceipt>;
}
