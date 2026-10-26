use std::time::SystemTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct RouteRule {
    pub workspace_id: Uuid,
    pub rule_id: Uuid,
    pub revision: u64,
    pub destination_id: Uuid,
}

#[derive(Clone)]
pub struct ReplyRoute {
    pub workspace_id: Uuid,
    pub route_id: Uuid,
    pub owner_principal_id: Uuid,
    pub destination_id: Uuid,
    pub original_event_id: Uuid,
    pub correlation_id: Uuid,
    pub expires_at: SystemTime,
}
