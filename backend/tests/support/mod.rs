#![allow(dead_code)]
use std::{path::Path, time::SystemTime};
use synquay_server::{
    domain::{
        connector::ConnectionScope,
        delivery::{Destination, DispatchKey, DispatchPlan, PendingDelivery, ReplaySafety},
        event::{EventEnvelope, EventPayload},
        message::{ContentPart, Message},
    },
    infrastructure::pipeline::config::PipelineConfig,
};
use uuid::Uuid;
pub fn scope() -> ConnectionScope {
    ConnectionScope {
        workspace_id: Uuid::new_v4(),
        connection_id: Uuid::new_v4(),
    }
}
pub fn event(scope: ConnectionScope) -> EventEnvelope {
    EventEnvelope {
        route_context: None,
        event_id: Uuid::new_v4(),
        scope,
        event_type: "chat.message".into(),
        schema_version: 1,
        occurred_at: None,
        received_at: SystemTime::now(),
        source_event_id: Some("upstream-1".into()),
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        route_trace: vec![scope],
        payload: EventPayload::Json(serde_json::json!({"text":"hello"})),
    }
}
pub fn job(scope: ConnectionScope) -> PendingDelivery {
    PendingDelivery {
        delivery_id: Uuid::new_v4(),
        route_rule: None,
        destination: Destination {
            id: Uuid::new_v4(),
            scope,
            replay_safety: ReplaySafety::NonIdempotent,
        },
        message: Message {
            destination_binding: None,
            parts: vec![ContentPart::Text("hello".into())],
            reply_route_id: None,
        },
        correlation_id: Uuid::new_v4(),
        causation_id: Uuid::new_v4(),
        route_trace: vec![],
    }
}
pub fn plan(job: PendingDelivery) -> DispatchPlan {
    DispatchPlan {
        workspace_id: job.destination.scope.workspace_id,
        key: DispatchKey::Reply {
            workspace_id: job.destination.scope.workspace_id,
            reply_id: Uuid::new_v4(),
        },
        deliveries: vec![job],
    }
}
pub const TOKEN: &str = "test-only-webhook-token-0123456789abcdef";
pub fn config(path: &Path, url: &str) -> PipelineConfig {
    PipelineConfig {
        bootstrap_routes: true,
        agent: None,
        webhook: Default::default(),
        content: None,
        retention: None,
        capacity: Default::default(),
        scheduler: Default::default(),
        additional_targets: vec![],
        additional_sources: vec![],
        source_name: None,
        admin_token_env: None,
        operator_principal_id: None,
        database: path.to_owned(),
        workspace_id: Uuid::from_u128(1),
        source_connection_id: Uuid::from_u128(2),
        target_connection_id: Uuid::from_u128(3),
        destination_id: Uuid::from_u128(4),
        principal_id: Uuid::from_u128(5),
        rule_id: Uuid::from_u128(6),
        webhook_token_env: "TEST_WEBHOOK_TOKEN".into(),
        destination_url: url.into(),
        destination_token_env: None,
        allow_loopback_http: true,
    }
}
