use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Content is deliberately not Debug: payloads may contain private messages.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentPart {
    StoredContent(super::content::ContentRef),
    Text(String),
    Json(Value),
    Attachment {
        attachment_id: Uuid,
        media_type: String,
    },
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_binding: Option<super::connection_routing::RouteBinding>,
    pub parts: Vec<ContentPart>,
    /// Opaque server-side reference binding the original channel/thread/sender.
    /// Adapters resolve this through trusted state; it is not an arbitrary URL.
    pub reply_route_id: Option<Uuid>,
}

pub struct DeliveryRequest {
    /// Stable across retries of this exact logical delivery; not a request/trace ID.
    pub delivery_id: Uuid,
    /// Authorized configured destination belonging to the current connection scope.
    pub destination_id: Uuid,
    pub message: Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryStatus {
    /// Provider/broker accepted the submission; recipient delivery is not confirmed.
    Accepted,
    Delivered,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub delivery_id: Uuid,
    pub external_message_id: Option<String>,
    pub status: DeliveryStatus,
}
