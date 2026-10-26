//! Contract fixtures only: the store is in memory and no real protocol I/O occurs.
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use synquay_server::{
    application::{
        connectors::{ConnectorRegistration, ConnectorRegistry, IngressService},
        ports::{
            OperationContext, PortError, PortFuture, RetryAdvice,
            events::{
                EventIngress, EventSource, EventStore, InboundDecoder, RawInbound, SourceExit,
            },
            lifecycle::ConnectorLifecycle,
            messages::MessageSink,
            tools::ToolExecutor,
        },
    },
    domain::{
        connector::{Capability, ConnectionScope, ConnectorDescriptor, ConnectorKind},
        event::{EventEnvelope, EventPayload, IncomingEvent, IngestDisposition, IngestReceipt},
        message::{ContentPart, DeliveryReceipt, DeliveryRequest, DeliveryStatus, Message},
        tool::{ToolCall, ToolDefinition, ToolEffect, ToolResult},
    },
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn scope() -> ConnectionScope {
    ConnectionScope {
        workspace_id: Uuid::new_v4(),
        connection_id: Uuid::new_v4(),
    }
}

fn context(scope: ConnectionScope) -> OperationContext {
    OperationContext {
        scope,
        principal_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        route_trace: vec![],
        deadline: None,
        cancellation: CancellationToken::new(),
    }
}

fn incoming() -> IncomingEvent {
    IncomingEvent {
        route_context: None,
        event_type: "message.received".into(),
        schema_version: 1,
        occurred_at: None,
        source_event_id: Some("upstream-1".into()),
        payload: EventPayload::Json(json!({"text": "private test content"})),
    }
}

#[derive(Default)]
struct StoreFixture {
    events: Mutex<Vec<EventEnvelope>>,
    fail: AtomicBool,
}

impl EventStore for StoreFixture {
    fn append(&self, event: EventEnvelope) -> PortFuture<'_, IngestReceipt> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(PortError::Unavailable);
            }
            let mut events = self.events.lock().unwrap();
            if event.source_event_id.is_some()
                && let Some(original) = events.iter().find(|stored| {
                    stored.scope == event.scope && stored.source_event_id == event.source_event_id
                })
            {
                return Ok(IngestReceipt {
                    event_id: original.event_id,
                    disposition: IngestDisposition::Duplicate,
                });
            }
            let receipt = IngestReceipt {
                event_id: event.event_id,
                disposition: IngestDisposition::Stored,
            };
            events.push(event);
            Ok(receipt)
        })
    }
}

struct HttpDecoderFixture;
impl InboundDecoder for HttpDecoderFixture {
    fn decode(
        &self,
        context: OperationContext,
        input: RawInbound,
    ) -> PortFuture<'_, IncomingEvent> {
        Box::pin(async move {
            context.check_active()?;
            let payload =
                serde_json::from_slice(&input.body).map_err(|_| PortError::InvalidInput {
                    code: "invalid_json",
                })?;
            Ok(IncomingEvent {
                payload: EventPayload::Json(payload),
                ..incoming()
            })
        })
    }
}

struct CliFixture;
impl ToolExecutor for CliFixture {
    fn discover(&self, context: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>> {
        Box::pin(async move {
            context.check_active()?;
            Ok(vec![ToolDefinition {
                id: "version".into(),
                description: "Configured version command".into(),
                input_schema: json!({"type": "object", "additionalProperties": false}),
                output_schema: None,
                effect: ToolEffect::ReadOnly,
                documentation: None,
            }])
        })
    }
    fn invoke(&self, context: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult> {
        Box::pin(async move {
            context.check_active()?;
            if call.tool_id != "version" {
                return Err(PortError::Unsupported);
            }
            Ok(ToolResult {
                content: vec![ContentPart::Text("fixture-version".into())],
                structured_content: None,
                is_error: false,
            })
        })
    }
}

struct MqttFixture {
    acknowledged: Arc<AtomicBool>,
}
impl EventSource for MqttFixture {
    fn run(
        &self,
        context: OperationContext,
        ingress: Arc<dyn EventIngress>,
    ) -> PortFuture<'_, SourceExit> {
        Box::pin(async move {
            context.check_active()?;
            ingress.accept(context.clone(), incoming()).await?;
            self.acknowledged.store(true, Ordering::SeqCst);
            Ok(if context.cancellation.is_cancelled() {
                SourceExit::Cancelled
            } else {
                SourceExit::Completed
            })
        })
    }
}
impl MessageSink for MqttFixture {
    fn deliver(
        &self,
        context: OperationContext,
        request: DeliveryRequest,
    ) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            context.check_active()?;
            Ok(DeliveryReceipt {
                delivery_id: request.delivery_id,
                external_message_id: None,
                status: DeliveryStatus::Accepted,
            })
        })
    }
}

#[tokio::test]
async fn protocols_compose_as_independent_trait_objects() {
    let scope = scope();
    let store = Arc::new(StoreFixture::default());
    let ingress: Arc<dyn EventIngress> = Arc::new(IngressService::new(scope, store));
    let decoder: Arc<dyn InboundDecoder> = Arc::new(HttpDecoderFixture);
    let event = decoder
        .decode(
            context(scope),
            RawInbound {
                media_type: Some("application/json".into()),
                metadata: BTreeMap::new(),
                body: br#"{"hello":"world"}"#.to_vec(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        ingress
            .accept(context(scope), event)
            .await
            .unwrap()
            .disposition,
        IngestDisposition::Stored
    );
    let executor: Arc<dyn ToolExecutor> = Arc::new(CliFixture);
    assert_eq!(
        executor.discover(context(scope)).await.unwrap()[0].id,
        "version"
    );
    let result = executor
        .invoke(
            context(scope),
            ToolCall {
                invocation_id: Uuid::new_v4(),
                tool_id: "version".into(),
                arguments: json!({}),
            },
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    let mqtt = Arc::new(MqttFixture {
        acknowledged: Arc::new(AtomicBool::new(false)),
    });
    let source: Arc<dyn EventSource> = mqtt.clone();
    assert_eq!(
        source.run(context(scope), ingress).await.unwrap(),
        SourceExit::Completed
    );
    assert!(mqtt.acknowledged.load(Ordering::SeqCst));
    let sink: Arc<dyn MessageSink> = mqtt;
    let delivery_id = Uuid::new_v4();
    let receipt = sink
        .deliver(
            context(scope),
            DeliveryRequest {
                delivery_id,
                destination_id: Uuid::new_v4(),
                message: Message {
                    destination_binding: None,
                    parts: vec![ContentPart::Text("hello".into())],
                    reply_route_id: None,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.delivery_id, delivery_id);
    assert_eq!(receipt.status, DeliveryStatus::Accepted);
    // Compile-time object-safety assertion for lifecycle, without inventing a lifecycle implementation.
    let _: Option<Arc<dyn ConnectorLifecycle>> = None;
}

#[tokio::test]
async fn ingress_binds_scope_and_preserves_causal_metadata() {
    let scope = scope();
    let store = Arc::new(StoreFixture::default());
    let ingress = IngressService::new(scope, store.clone());
    let mut wrong = context(scope);
    wrong.scope.workspace_id = Uuid::new_v4();
    assert_eq!(
        ingress.accept(wrong, incoming()).await.unwrap_err(),
        PortError::ScopeMismatch
    );
    assert!(store.events.lock().unwrap().is_empty());
    let mut ctx = context(scope);
    ctx.causation_id = Some(Uuid::new_v4());
    let receipt = ingress.accept(ctx.clone(), incoming()).await.unwrap();
    let events = store.events.lock().unwrap();
    let event = &events[0];
    assert_eq!(event.scope, scope);
    assert_eq!(event.event_id, receipt.event_id);
    assert_eq!(event.correlation_id, ctx.correlation_id);
    assert_eq!(event.causation_id, ctx.causation_id);
    assert_eq!(event.route_trace, vec![scope]);
    assert!(event.occurred_at.is_none());
}

#[tokio::test]
async fn duplicate_keys_are_scoped_and_return_original_ids() {
    let first = scope();
    let second = ConnectionScope {
        workspace_id: Uuid::new_v4(),
        ..first
    };
    let store = Arc::new(StoreFixture::default());
    let ingress = IngressService::new(first, store.clone());
    let a = ingress.accept(context(first), incoming()).await.unwrap();
    let b = ingress.accept(context(first), incoming()).await.unwrap();
    assert_eq!(a.event_id, b.event_id);
    assert_eq!(b.disposition, IngestDisposition::Duplicate);
    let other = IngressService::new(second, store.clone());
    let c = other.accept(context(second), incoming()).await.unwrap();
    assert_ne!(a.event_id, c.event_id);
    let mut without_id = incoming();
    without_id.source_event_id = None;
    let d = ingress
        .accept(context(first), without_id.clone())
        .await
        .unwrap();
    let e = ingress.accept(context(first), without_id).await.unwrap();
    assert_ne!(d.event_id, e.event_id);
    assert_eq!(store.events.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn invalid_or_cancelled_events_never_reach_storage() {
    let scope = scope();
    let store = Arc::new(StoreFixture::default());
    let ingress = IngressService::new(scope, store.clone());
    let cancelled = context(scope);
    cancelled.cancellation.cancel();
    assert_eq!(
        ingress.accept(cancelled, incoming()).await.unwrap_err(),
        PortError::Cancelled
    );
    let mut expired = context(scope);
    expired.deadline = Some(Instant::now());
    assert_eq!(
        ingress.accept(expired, incoming()).await.unwrap_err(),
        PortError::DeadlineExceeded
    );
    let mut invalid = incoming();
    invalid.schema_version = 0;
    assert!(matches!(
        ingress.accept(context(scope), invalid).await,
        Err(PortError::InvalidInput { .. })
    ));
    let mut looped = context(scope);
    looped.route_trace.push(scope);
    assert!(matches!(
        ingress.accept(looped, incoming()).await,
        Err(PortError::InvalidInput { .. })
    ));
    assert!(store.events.lock().unwrap().is_empty());
}

struct GatedStore {
    entered: Notify,
    release: Notify,
}
impl EventStore for GatedStore {
    fn append(&self, event: EventEnvelope) -> PortFuture<'_, IngestReceipt> {
        Box::pin(async move {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(IngestReceipt {
                event_id: event.event_id,
                disposition: IngestDisposition::Stored,
            })
        })
    }
}

#[tokio::test]
async fn source_waits_for_commit_and_handles_cancellation_during_commit() {
    let scope = scope();
    let store = Arc::new(GatedStore {
        entered: Notify::new(),
        release: Notify::new(),
    });
    let ingress: Arc<dyn EventIngress> = Arc::new(IngressService::new(scope, store.clone()));
    let acknowledged = Arc::new(AtomicBool::new(false));
    let source = MqttFixture {
        acknowledged: acknowledged.clone(),
    };
    let ctx = context(scope);
    let cancel = ctx.cancellation.clone();
    let task = tokio::spawn(async move { source.run(ctx, ingress).await });
    tokio::time::timeout(Duration::from_secs(2), store.entered.notified())
        .await
        .unwrap();
    assert!(!acknowledged.load(Ordering::SeqCst));
    cancel.cancel();
    store.release.notify_one();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        SourceExit::Cancelled
    );
    assert!(acknowledged.load(Ordering::SeqCst));
}

#[tokio::test]
async fn failed_persistence_does_not_acknowledge_source() {
    let scope = scope();
    let store = Arc::new(StoreFixture::default());
    store.fail.store(true, Ordering::SeqCst);
    let source = MqttFixture {
        acknowledged: Arc::new(AtomicBool::new(false)),
    };
    let ingress = Arc::new(IngressService::new(scope, store));
    assert_eq!(
        source.run(context(scope), ingress).await.unwrap_err(),
        PortError::Unavailable
    );
    assert!(!source.acknowledged.load(Ordering::SeqCst));
}

#[tokio::test]
async fn cancellation_deadlines_and_retry_advice_are_explicit() {
    let parent = CancellationToken::new();
    let mut ctx = context(scope());
    ctx.cancellation = parent.child_token();
    let waiter = tokio::spawn(async move { ctx.stopped().await });
    parent.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap(),
        PortError::Cancelled
    );
    let mut deadline = context(scope());
    deadline.deadline = Some(Instant::now() + Duration::from_millis(5));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), deadline.stopped())
            .await
            .unwrap(),
        PortError::DeadlineExceeded
    );
    assert_eq!(
        PortError::OutcomeUnknown.retry_advice(),
        RetryAdvice::Reconcile
    );
    assert_eq!(
        PortError::DeadlineExceeded.retry_advice(),
        RetryAdvice::Reconcile
    );
    assert_eq!(
        PortError::PermissionDenied.retry_advice(),
        RetryAdvice::Never
    );
    assert_eq!(
        PortError::Unavailable.retry_advice(),
        RetryAdvice::IfIdempotent { after: None }
    );
}

#[test]
fn registry_rejects_duplicates_and_keeps_workspace_boundaries() {
    let scope = scope();
    let registration = || {
        let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
            scope,
            kind: ConnectorKind::Http,
            display_name: "Webhook".into(),
        });
        registration.decoder = Some(Arc::new(HttpDecoderFixture));
        registration
    };
    let mut registry = ConnectorRegistry::default();
    registry.register(registration()).unwrap();
    assert_eq!(
        registry.register(registration()).unwrap_err(),
        PortError::AlreadyRegistered
    );
    assert_eq!(
        registry.get(scope).unwrap().capabilities(),
        vec![Capability::DecodeInbound]
    );
    assert!(registry.get(scope).unwrap().tools.is_none());
    assert!(matches!(
        registry.get(ConnectionScope {
            workspace_id: Uuid::new_v4(),
            ..scope
        }),
        Err(PortError::NotFound)
    ));
    let empty = ConnectorRegistration::new(ConnectorDescriptor {
        scope,
        kind: ConnectorKind::Cli,
        display_name: "Empty".into(),
    });
    assert!(matches!(
        registry.register(empty),
        Err(PortError::InvalidInput { .. })
    ));
}
