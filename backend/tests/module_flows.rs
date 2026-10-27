//! In-memory contract fixtures only. Nothing here is installed in the live application.
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use synquay_server::{
    application::{
        connectors::{ConnectorRegistration, ConnectorRegistry, IngressService},
        modules::{BackendModules, ModuleDependencies},
        ports::{
            OperationContext, PortError, PortFuture,
            audit::AuditLog,
            delivery::*,
            events::{EventIngress, EventStore},
            identity::*,
            messages::MessageSink,
            profiles::*,
            routing::*,
            tools::ToolExecutor,
        },
        services::tools::PublishedToolCall,
    },
    domain::{
        audit::*, connector::*, delivery::*, event::*, identity::*, message::*, profile::*,
        routing::*, tool::*,
    },
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Queue {
    plans: HashMap<DispatchKey, (DispatchReceipt, Vec<PendingDelivery>)>,
    pending: VecDeque<PendingDelivery>,
    leases: HashMap<Uuid, (LeaseToken, bool)>,
    finished: Vec<DeliveryOutcome>,
}
struct Fixture {
    source: ConnectionScope,
    target: ConnectionScope,
    principal: Uuid,
    profile: Mutex<PublishedProfile>,
    destination: Mutex<Destination>,
    rule: Mutex<RouteRule>,
    reply: Mutex<ReplyRoute>,
    events: Mutex<Vec<EventEnvelope>>,
    queue: Mutex<Queue>,
    audit: Mutex<Vec<AuditRecord>>,
    deny_tool: AtomicBool,
    deny_delivery: AtomicBool,
    fail_audit_start: AtomicBool,
    fail_audit_end: AtomicBool,
    fail_finish: AtomicBool,
    sink_unknown: AtomicBool,
    sink_error: Mutex<Option<PortError>>,
    delivery_attempt: AtomicUsize,
    sink_unavailable: AtomicBool,
    calls: AtomicUsize,
    sends: AtomicUsize,
    sent: Mutex<Vec<DeliveryRequest>>,
    order: Mutex<Vec<&'static str>>,
}
impl Fixture {
    fn new() -> Arc<Self> {
        let workspace = Uuid::new_v4();
        let source = ConnectionScope {
            workspace_id: workspace,
            connection_id: Uuid::new_v4(),
        };
        let target = ConnectionScope {
            workspace_id: workspace,
            connection_id: Uuid::new_v4(),
        };
        let principal = Uuid::new_v4();
        let destination = Destination {
            id: Uuid::new_v4(),
            scope: target,
            replay_safety: ReplaySafety::NonIdempotent,
        };
        let rule = RouteRule {
            workspace_id: workspace,
            rule_id: Uuid::new_v4(),
            revision: 1,
            destination_id: destination.id,
        };
        let reply = ReplyRoute {
            workspace_id: workspace,
            route_id: Uuid::new_v4(),
            owner_principal_id: principal,
            destination_id: destination.id,
            original_event_id: Uuid::new_v4(),
            correlation_id: Uuid::new_v4(),
            expires_at: SystemTime::now() + Duration::from_secs(300),
        };
        Arc::new(Self {
            source,
            target,
            principal,
            profile: Mutex::new(PublishedProfile {
                workspace_id: workspace,
                profile_id: Uuid::new_v4(),
                revision: 1,
                tools: vec![PublishedTool {
                    public_name: "public_version".into(),
                    scope: target,
                    definition: ToolDefinition {
                        id: "internal_version".into(),
                        description: "Version".into(),
                        input_schema: json!({"type":"object"}),
                        output_schema: None,
                        effect: ToolEffect::ReadOnly,
                        documentation: None,
                    },
                }],
            }),
            destination: Mutex::new(destination),
            rule: Mutex::new(rule),
            reply: Mutex::new(reply),
            events: Mutex::new(vec![]),
            queue: Mutex::new(Queue {
                plans: HashMap::new(),
                pending: VecDeque::new(),
                leases: HashMap::new(),
                finished: vec![],
            }),
            audit: Mutex::new(vec![]),
            deny_tool: AtomicBool::new(false),
            deny_delivery: AtomicBool::new(false),
            fail_audit_start: AtomicBool::new(false),
            fail_audit_end: AtomicBool::new(false),
            fail_finish: AtomicBool::new(false),
            sink_unknown: AtomicBool::new(false),
            sink_error: Mutex::new(None),
            delivery_attempt: AtomicUsize::new(1),
            sink_unavailable: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            sends: AtomicUsize::new(0),
            sent: Mutex::new(vec![]),
            order: Mutex::new(vec![]),
        })
    }
    fn identity(&self) -> Identity {
        Identity {
            mcp_endpoint: None,
            workspace_id: self.source.workspace_id,
            principal_id: self.principal,
            client_id: None,
            scopes: BTreeSet::from(["fixture".into()]),
            expires_at: None,
        }
    }
    fn context(&self) -> AccessContext {
        AccessContext {
            identity: self.identity(),
            operation: OperationContext {
                scope: self.source,
                principal_id: self.principal,
                request_id: Uuid::new_v4(),
                correlation_id: Uuid::new_v4(),
                causation_id: None,
                route_trace: vec![],
                deadline: None,
                cancellation: CancellationToken::new(),
            },
        }
    }
    fn modules(self: &Arc<Self>) -> BackendModules {
        let mut registry = ConnectorRegistry::default();
        let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
            scope: self.target,
            kind: ConnectorKind::Cli,
            display_name: "Fixture".into(),
        });
        registration.tools = Some(self.clone());
        registration.sink = Some(self.clone());
        registry.register(registration).unwrap();
        BackendModules::new(ModuleDependencies {
            authenticator: self.clone(),
            policy: self.clone(),
            audit: self.clone(),
            profiles: self.clone(),
            validator: self.clone(),
            events: self.clone(),
            rules: self.clone(),
            renderer: self.clone(),
            destinations: self.clone(),
            outbox: self.clone(),
            reply_routes: self.clone(),
            connectors: Arc::new(registry),
        })
    }
    fn call(&self) -> PublishedToolCall {
        PublishedToolCall {
            profile_id: self.profile.lock().unwrap().profile_id,
            revision: 1,
            public_name: "public_version".into(),
            invocation_id: Uuid::new_v4(),
            arguments: json!({}),
        }
    }
    async fn ingest(self: &Arc<Self>) -> Uuid {
        IngressService::new(self.source, self.clone())
            .accept(
                self.context().operation,
                IncomingEvent {
                    route_context: None,
                    event_type: "message.received".into(),
                    schema_version: 1,
                    occurred_at: None,
                    source_event_id: None,
                    payload: EventPayload::Message(Message {
                        destination_binding: None,
                        parts: vec![ContentPart::Text("private".into())],
                        reply_route_id: None,
                    }),
                },
            )
            .await
            .unwrap()
            .event_id
    }
}
impl Authenticator for Fixture {
    fn authenticate(&self, credentials: Credentials) -> PortFuture<'_, Identity> {
        Box::pin(async move {
            if credentials.bearer_token != "fixture-token" {
                return Err(PortError::PermissionDenied);
            }
            Ok(self.identity())
        })
    }
}
impl PolicyEngine for Fixture {
    fn authorize(
        &self,
        identity: Identity,
        action: Action,
        resource: Resource,
    ) -> PortFuture<'_, ()> {
        Box::pin(async move {
            if identity.workspace_id != resource.workspace_id()
                || !identity.scopes.contains("fixture")
                || (action == Action::InvokeTool && self.deny_tool.load(Ordering::SeqCst))
                || (action == Action::DeliverMessage && self.deny_delivery.load(Ordering::SeqCst))
            {
                return Err(PortError::PermissionDenied);
            }
            self.order.lock().unwrap().push("authorize");
            Ok(())
        })
    }
}
impl AuditLog for Fixture {
    fn append(&self, record: AuditRecord) -> PortFuture<'_, ()> {
        Box::pin(async move {
            let start = record.phase == AuditPhase::Started;
            if (start && self.fail_audit_start.load(Ordering::SeqCst))
                || (!start && self.fail_audit_end.load(Ordering::SeqCst))
            {
                return Err(PortError::Unavailable);
            }
            self.order
                .lock()
                .unwrap()
                .push(if start { "audit_start" } else { "audit_end" });
            self.audit.lock().unwrap().push(record);
            Ok(())
        })
    }
}
impl ProfileRepository for Fixture {
    fn published(&self, workspace: Uuid, profile: Uuid) -> PortFuture<'_, PublishedProfile> {
        Box::pin(async move {
            let value = self.profile.lock().unwrap().clone();
            if value.workspace_id != workspace || value.profile_id != profile {
                return Err(PortError::NotFound);
            }
            Ok(value)
        })
    }
}
impl SchemaValidator for Fixture {
    fn validate<'a>(&'a self, _schema: &'a Value, value: &'a Value) -> PortFuture<'a, ()> {
        Box::pin(async move {
            if !value.is_object() {
                return Err(PortError::InvalidInput {
                    code: "fixture_expected_object",
                });
            }
            Ok(())
        })
    }
}
impl ToolExecutor for Fixture {
    fn discover(&self, _context: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>> {
        Box::pin(async move { Ok(vec![]) })
    }
    fn invoke(&self, context: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult> {
        Box::pin(async move {
            assert_eq!(context.scope, self.target);
            assert_eq!(call.tool_id, "internal_version");
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.order.lock().unwrap().push("invoke");
            Ok(ToolResult {
                content: vec![ContentPart::Text("version".into())],
                structured_content: Some(json!({"version":1})),
                is_error: false,
            })
        })
    }
}
impl EventStore for Fixture {
    fn append(&self, event: EventEnvelope) -> PortFuture<'_, IngestReceipt> {
        Box::pin(async move {
            let receipt = IngestReceipt {
                event_id: event.event_id,
                disposition: IngestDisposition::Stored,
            };
            self.events.lock().unwrap().push(event);
            Ok(receipt)
        })
    }
}
impl EventReader for Fixture {
    fn load(&self, scope: ConnectionScope, id: Uuid) -> PortFuture<'_, EventEnvelope> {
        Box::pin(async move {
            self.events
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.scope == scope && e.event_id == id)
                .cloned()
                .ok_or(PortError::NotFound)
        })
    }
    fn unrouted(
        &self,
        workspace: Uuid,
        limit: usize,
    ) -> PortFuture<'_, Vec<(ConnectionScope, Uuid)>> {
        Box::pin(async move {
            let events = self.events.lock().unwrap();
            let queue = self.queue.lock().unwrap();
            Ok(events
                .iter()
                .filter(|e| {
                    e.scope.workspace_id == workspace
                        && !queue.plans.contains_key(&DispatchKey::Event {
                            scope: e.scope,
                            event_id: e.event_id,
                        })
                })
                .take(limit)
                .map(|e| (e.scope, e.event_id))
                .collect())
        })
    }
}
impl RoutingRules for Fixture {
    fn matching<'a>(&'a self, _event: &'a EventEnvelope) -> PortFuture<'a, Vec<RouteRule>> {
        Box::pin(async move { Ok(vec![self.rule.lock().unwrap().clone()]) })
    }
}
impl MessageRenderer for Fixture {
    fn render<'a>(
        &'a self,
        event: &'a EventEnvelope,
        _rule: &'a RouteRule,
    ) -> PortFuture<'a, Message> {
        Box::pin(async move {
            match &event.payload {
                EventPayload::Message(message) => Ok(message.clone()),
                _ => Err(PortError::Unsupported),
            }
        })
    }
}
impl DestinationRepository for Fixture {
    fn resolve(&self, workspace: Uuid, id: Uuid) -> PortFuture<'_, Destination> {
        Box::pin(async move {
            let value = self.destination.lock().unwrap().clone();
            if value.id != id || value.scope.workspace_id != workspace {
                return Err(PortError::NotFound);
            }
            Ok(value)
        })
    }
}
impl ReplyRouteStore for Fixture {
    fn resolve(&self, workspace: Uuid, id: Uuid) -> PortFuture<'_, ReplyRoute> {
        Box::pin(async move {
            let value = self.reply.lock().unwrap().clone();
            if value.workspace_id != workspace || value.route_id != id {
                return Err(PortError::NotFound);
            }
            Ok(value)
        })
    }
}
impl DeliveryOutbox for Fixture {
    fn enqueue(&self, plan: DispatchPlan) -> PortFuture<'_, DispatchReceipt> {
        Box::pin(async move {
            let mut queue = self.queue.lock().unwrap();
            if let Some((original, jobs)) = queue.plans.get(&plan.key) {
                if matches!(plan.key, DispatchKey::Reply { .. })
                    && (jobs.len() != plan.deliveries.len()
                        || jobs.iter().zip(&plan.deliveries).any(|(a, b)| {
                            a.message != b.message || a.destination.id != b.destination.id
                        }))
                {
                    return Err(PortError::Conflict);
                }
                return Ok(DispatchReceipt {
                    duplicate: true,
                    ..original.clone()
                });
            }
            assert!(
                plan.deliveries
                    .iter()
                    .all(|d| d.destination.scope.workspace_id == plan.workspace_id)
            );
            let receipt = DispatchReceipt {
                delivery_ids: plan.deliveries.iter().map(|d| d.delivery_id).collect(),
                duplicate: false,
            };
            queue.pending.extend(plan.deliveries.clone());
            queue
                .plans
                .insert(plan.key, (receipt.clone(), plan.deliveries));
            Ok(receipt)
        })
    }
    fn claim(
        &self,
        workspace: Uuid,
        now: SystemTime,
        lease_for: Duration,
    ) -> PortFuture<'_, Option<LeasedDelivery>> {
        Box::pin(async move {
            let mut queue = self.queue.lock().unwrap();
            let Some(index) = queue
                .pending
                .iter()
                .position(|d| d.destination.scope.workspace_id == workspace)
            else {
                return Ok(None);
            };
            let job = queue.pending.remove(index).unwrap();
            let token = LeaseToken {
                workspace_id: workspace,
                delivery_id: job.delivery_id,
                lease_id: Uuid::new_v4(),
                expires_at: now + lease_for,
            };
            queue.leases.insert(job.delivery_id, (token.clone(), false));
            Ok(Some(LeasedDelivery {
                token,
                job,
                attempt: self.delivery_attempt.load(Ordering::SeqCst) as u32,
            }))
        })
    }
    fn mark_sending(&self, token: LeaseToken) -> PortFuture<'_, ()> {
        Box::pin(async move {
            let mut queue = self.queue.lock().unwrap();
            let (current, sending) = queue
                .leases
                .get_mut(&token.delivery_id)
                .ok_or(PortError::Conflict)?;
            if current.lease_id != token.lease_id
                || current.workspace_id != token.workspace_id
                || current.expires_at <= SystemTime::now()
            {
                return Err(PortError::Conflict);
            }
            *sending = true;
            self.order.lock().unwrap().push("mark_sending");
            Ok(())
        })
    }
    fn finish(&self, token: LeaseToken, outcome: DeliveryOutcome) -> PortFuture<'_, ()> {
        Box::pin(async move {
            if self.fail_finish.load(Ordering::SeqCst) {
                return Err(PortError::Unavailable);
            }
            let mut queue = self.queue.lock().unwrap();
            let (current, _) = queue
                .leases
                .get(&token.delivery_id)
                .ok_or(PortError::Conflict)?;
            if current.lease_id != token.lease_id
                || current.workspace_id != token.workspace_id
                || current.expires_at <= SystemTime::now()
            {
                return Err(PortError::Conflict);
            }
            queue.leases.remove(&token.delivery_id);
            queue.finished.push(outcome);
            self.order.lock().unwrap().push("finish");
            Ok(())
        })
    }
}
impl MessageSink for Fixture {
    fn deliver(
        &self,
        context: OperationContext,
        request: DeliveryRequest,
    ) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            assert_eq!(context.scope, self.target);
            let remaining = context
                .deadline
                .unwrap()
                .saturating_duration_since(std::time::Instant::now());
            assert!(remaining > Duration::ZERO && remaining <= Duration::from_secs(60));
            assert!(
                self.queue
                    .lock()
                    .unwrap()
                    .leases
                    .get(&request.delivery_id)
                    .unwrap()
                    .1
            );
            self.sends.fetch_add(1, Ordering::SeqCst);
            self.order.lock().unwrap().push("send");
            let receipt = DeliveryReceipt {
                delivery_id: request.delivery_id,
                external_message_id: Some("provider-1".into()),
                status: DeliveryStatus::Accepted,
            };
            self.sent.lock().unwrap().push(request);
            if let Some(error) = self.sink_error.lock().unwrap().clone() {
                return Err(error);
            }
            if self.sink_unknown.load(Ordering::SeqCst) {
                return Err(PortError::OutcomeUnknown);
            }
            if self.sink_unavailable.load(Ordering::SeqCst) {
                return Err(PortError::Unavailable);
            }
            Ok(receipt)
        })
    }
}

#[tokio::test]
async fn authentication_binds_principal_and_rejects_foreign_workspace() {
    let f = Fixture::new();
    let m = f.modules();
    let mut op = f.context().operation;
    op.principal_id = Uuid::new_v4();
    let ctx = m
        .identity
        .authenticate(
            Credentials {
                bearer_token: "fixture-token".into(),
            },
            op.clone(),
        )
        .await
        .unwrap();
    assert_eq!(ctx.operation.principal_id, f.principal);
    op.scope.workspace_id = Uuid::new_v4();
    assert!(matches!(
        m.identity
            .authenticate(
                Credentials {
                    bearer_token: "fixture-token".into()
                },
                op
            )
            .await,
        Err(PortError::ScopeMismatch)
    ));
    assert!(matches!(
        m.identity
            .authenticate(
                Credentials {
                    bearer_token: "wrong".into()
                },
                f.context().operation
            )
            .await,
        Err(PortError::PermissionDenied)
    ));
}

#[tokio::test]
async fn published_tools_share_authorization_between_list_and_invoke() {
    let f = Fixture::new();
    let m = f.modules();
    let ctx = f.context();
    let profile = f.call().profile_id;
    let list = m.tools.list(&ctx, profile).await.unwrap();
    assert_eq!(list.profile_id, profile);
    assert_eq!(list.revision, f.call().revision);
    assert_eq!(list.tools[0].id, "public_version");
    let result = m.tools.invoke(&ctx, f.call()).await.unwrap();
    assert!(result.outcome.is_ok() && result.audit_recorded);
    let order = f.order.lock().unwrap().clone();
    assert!(
        order.iter().position(|e| *e == "audit_start").unwrap()
            < order.iter().position(|e| *e == "invoke").unwrap()
    );
    f.deny_tool.store(true, Ordering::SeqCst);
    assert!(m.tools.list(&ctx, profile).await.unwrap().tools.is_empty());
    assert!(matches!(
        m.tools.invoke(&ctx, f.call()).await,
        Err(PortError::PermissionDenied)
    ));
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unknown_names_stale_versions_invalid_input_and_audit_failure_do_not_execute() {
    let f = Fixture::new();
    let m = f.modules();
    let ctx = f.context();
    let mut call = f.call();
    call.public_name = "internal_version".into();
    assert!(matches!(
        m.tools.invoke(&ctx, call).await,
        Err(PortError::NotFound)
    ));
    let mut call = f.call();
    call.revision = 2;
    assert!(matches!(
        m.tools.invoke(&ctx, call).await,
        Err(PortError::Conflict)
    ));
    let mut call = f.call();
    call.arguments = json!("bad");
    assert!(matches!(
        m.tools.invoke(&ctx, call).await,
        Err(PortError::InvalidInput { .. })
    ));
    f.fail_audit_start.store(true, Ordering::SeqCst);
    assert!(matches!(
        m.tools.invoke(&ctx, f.call()).await,
        Err(PortError::Unavailable)
    ));
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn completion_audit_failure_keeps_actual_execution_result() {
    let f = Fixture::new();
    let m = f.modules();
    f.fail_audit_end.store(true, Ordering::SeqCst);
    let report = m.tools.invoke(&f.context(), f.call()).await.unwrap();
    assert!(report.outcome.is_ok());
    assert!(!report.audit_recorded);
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn expired_identity_and_corrupt_profile_fail_closed() {
    let f = Fixture::new();
    let m = f.modules();
    let mut ctx = f.context();
    ctx.identity.expires_at = Some(SystemTime::now());
    assert!(matches!(
        m.tools.invoke(&ctx, f.call()).await,
        Err(PortError::PermissionDenied)
    ));
    f.profile.lock().unwrap().tools[0].scope.workspace_id = Uuid::new_v4();
    assert!(matches!(
        m.tools.invoke(&f.context(), f.call()).await,
        Err(PortError::InvalidInput { .. })
    ));
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn persisted_event_routes_once_and_worker_marks_before_sending() {
    let f = Fixture::new();
    let m = f.modules();
    let event = f.ingest().await;
    let ctx = f.context();
    assert_eq!(
        f.unrouted(f.source.workspace_id, 10).await.unwrap().len(),
        1
    );
    let first = m.routing.route(&ctx, event).await.unwrap();
    let repeated = m.routing.route(&ctx, event).await.unwrap();
    assert!(!first.receipt.duplicate);
    assert!(repeated.receipt.duplicate);
    assert_eq!(first.receipt.delivery_ids, repeated.receipt.delivery_ids);
    assert!(
        f.unrouted(f.source.workspace_id, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.sends.load(Ordering::SeqCst), 0);
    let report = m.delivery.run_one(&ctx).await.unwrap().unwrap();
    assert!(report.state_recorded && report.audit_recorded);
    assert!(matches!(
        report.outcome,
        DeliveryOutcome::Submitted(DeliveryReceipt {
            status: DeliveryStatus::Accepted,
            ..
        })
    ));
    assert!(m.delivery.run_one(&ctx).await.unwrap().is_none());
    assert_eq!(f.sends.load(Ordering::SeqCst), 1);
    let order = f.order.lock().unwrap();
    assert!(
        order.iter().position(|e| *e == "mark_sending").unwrap()
            < order.iter().position(|e| *e == "send").unwrap()
    );
}

#[tokio::test]
async fn routing_and_delivery_recheck_permissions() {
    let f = Fixture::new();
    let m = f.modules();
    let event = f.ingest().await;
    f.deny_delivery.store(true, Ordering::SeqCst);
    assert!(matches!(
        m.routing.route(&f.context(), event).await,
        Err(PortError::PermissionDenied)
    ));
    assert!(f.queue.lock().unwrap().pending.is_empty());
    f.deny_delivery.store(false, Ordering::SeqCst);
    m.routing.route(&f.context(), event).await.unwrap();
    f.deny_delivery.store(true, Ordering::SeqCst);
    assert!(matches!(
        m.delivery
            .run_one(&f.context())
            .await
            .unwrap()
            .unwrap()
            .outcome,
        DeliveryOutcome::Failed { .. }
    ));
    assert_eq!(f.sends.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn loops_and_foreign_rules_do_not_enqueue_partial_batches() {
    let f = Fixture::new();
    let m = f.modules();
    let event = f.ingest().await;
    f.rule.lock().unwrap().workspace_id = Uuid::new_v4();
    assert!(matches!(
        m.routing.route(&f.context(), event).await,
        Err(PortError::ScopeMismatch)
    ));
    f.rule.lock().unwrap().workspace_id = f.source.workspace_id;
    f.destination.lock().unwrap().scope = f.source;
    assert!(matches!(
        m.routing.route(&f.context(), event).await,
        Err(PortError::InvalidInput { .. })
    ));
    assert!(f.queue.lock().unwrap().pending.is_empty());
}

#[tokio::test]
async fn ambiguous_send_and_failed_state_save_never_look_like_unexecuted_calls() {
    let f = Fixture::new();
    let m = f.modules();
    let event = f.ingest().await;
    m.routing.route(&f.context(), event).await.unwrap();
    f.sink_unknown.store(true, Ordering::SeqCst);
    f.fail_finish.store(true, Ordering::SeqCst);
    let report = m.delivery.run_one(&f.context()).await.unwrap().unwrap();
    assert!(matches!(report.outcome, DeliveryOutcome::Reconcile));
    assert!(!report.state_recorded);
    assert_eq!(f.sends.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn only_trusted_idempotent_destinations_get_retry_outcomes() {
    for safe in [ReplaySafety::Idempotent, ReplaySafety::NonIdempotent] {
        let f = Fixture::new();
        f.destination.lock().unwrap().replay_safety = safe;
        let m = f.modules();
        let event = f.ingest().await;
        m.routing.route(&f.context(), event).await.unwrap();
        f.sink_unavailable.store(true, Ordering::SeqCst);
        let report = m.delivery.run_one(&f.context()).await.unwrap().unwrap();
        assert_eq!(
            matches!(report.outcome, DeliveryOutcome::RetryAfter(_)),
            safe == ReplaySafety::Idempotent
        );
    }
}

#[tokio::test]
async fn replies_use_bound_destination_owner_and_idempotency_key() {
    let f = Fixture::new();
    let m = f.modules();
    let ctx = f.context();
    let route = f.reply.lock().unwrap().route_id;
    let reply_id = Uuid::new_v4();
    let parts = vec![ContentPart::Text("AI response".into())];
    let first = m
        .replies
        .reply(&ctx, route, reply_id, parts.clone())
        .await
        .unwrap();
    let again = m.replies.reply(&ctx, route, reply_id, parts).await.unwrap();
    assert!(again.receipt.duplicate);
    assert_eq!(first.receipt.delivery_ids, again.receipt.delivery_ids);
    assert!(matches!(
        m.replies
            .reply(
                &ctx,
                route,
                reply_id,
                vec![ContentPart::Text("different".into())]
            )
            .await,
        Err(PortError::Conflict)
    ));
    m.delivery.run_one(&ctx).await.unwrap();
    {
        let sent = f.sent.lock().unwrap();
        assert_eq!(sent[0].destination_id, f.destination.lock().unwrap().id);
        assert_eq!(sent[0].message.reply_route_id, Some(route));
    }
    f.reply.lock().unwrap().owner_principal_id = Uuid::new_v4();
    assert!(matches!(
        m.replies
            .reply(
                &ctx,
                route,
                Uuid::new_v4(),
                vec![ContentPart::Text("no".into())]
            )
            .await,
        Err(PortError::PermissionDenied)
    ));
}

#[tokio::test]
async fn stale_lease_cannot_complete_a_delivery() {
    let f = Fixture::new();
    let m = f.modules();
    let event = f.ingest().await;
    m.routing.route(&f.context(), event).await.unwrap();
    let lease = f
        .claim(
            f.source.workspace_id,
            SystemTime::now(),
            Duration::from_secs(60),
        )
        .await
        .unwrap()
        .unwrap();
    let mut stale = lease.token;
    stale.lease_id = Uuid::new_v4();
    assert!(matches!(
        f.finish(stale, DeliveryOutcome::Reconcile).await,
        Err(PortError::Conflict)
    ));
}

#[tokio::test]
async fn temporary_preflight_failure_defers_non_idempotent_delivery_without_sending() {
    let f = Fixture::new();
    let m = f.modules();
    let event = f.ingest().await;
    m.routing.route(&f.context(), event).await.unwrap();
    f.fail_audit_start.store(true, Ordering::SeqCst);
    let report = m.delivery.run_one(&f.context()).await.unwrap().unwrap();
    assert!(matches!(report.outcome, DeliveryOutcome::Deferred(_)));
    assert_eq!(f.sends.load(Ordering::SeqCst), 0);
    assert!(!f.order.lock().unwrap().contains(&"mark_sending"));
}

#[tokio::test]
async fn rejected_or_unsent_deliveries_retry_with_bounds_but_unknown_sends_do_not() {
    for (error, attempt, expected) in [
        (PortError::NotSent { retry_after: None }, 1, "deferred"),
        (PortError::NotSent { retry_after: None }, 8, "failed"),
        (
            PortError::RateLimited {
                retry_after: Duration::from_secs(120),
            },
            1,
            "limited",
        ),
        (
            PortError::RateLimited {
                retry_after: Duration::from_secs(120),
            },
            8,
            "failed",
        ),
        (PortError::OutcomeUnknown, 1, "reconcile"),
        (PortError::PermissionDenied, 1, "failed"),
    ] {
        let f = Fixture::new();
        f.delivery_attempt.store(attempt, Ordering::SeqCst);
        *f.sink_error.lock().unwrap() = Some(error);
        let m = f.modules();
        let event = f.ingest().await;
        m.routing.route(&f.context(), event).await.unwrap();
        let report = m.delivery.run_one(&f.context()).await.unwrap().unwrap();
        match (expected, report.outcome) {
            ("deferred", DeliveryOutcome::Deferred(d)) => {
                assert!(d >= Duration::from_secs(1) && d <= Duration::from_secs(2))
            }
            ("limited", DeliveryOutcome::Deferred(d)) => assert!(d >= Duration::from_secs(120)),
            ("failed", DeliveryOutcome::Failed { .. })
            | ("reconcile", DeliveryOutcome::Reconcile) => {}
            (expected, _) => panic!("unexpected delivery outcome; expected {expected}"),
        }
        assert!(report.state_recorded);
    }
}
