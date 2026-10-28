use super::*;
use crate::{
    application::{
        connectors::{ConnectorRegistration, ConnectorRegistry},
        ports::{events::EventStore, tools::ToolExecutor},
        services::{delivery::DeliveryService, routing::RoutingService},
    },
    domain::{
        command::*,
        connector::{ConnectorDescriptor, ConnectorKind},
        event::{EventEnvelope, EventPayload},
        managed_route::{RouteSnapshot, RouteTarget},
        tool::*,
    },
    infrastructure::connectors::{message_config::MessageConnection, qq::QqBotConfig},
};
use std::sync::{
    Mutex, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Fake {
    calls: AtomicUsize,
    unknown: AtomicBool,
    fail_reply: AtomicBool,
    actors: Mutex<Vec<Uuid>>,
    replies: Mutex<Vec<Message>>,
}
impl ToolExecutor for Fake {
    fn discover(&self, _: OperationContext) -> PortFuture<'_, Vec<ToolDefinition>> {
        Box::pin(async { Ok(crate::infrastructure::connectors::qq::actions::definitions()) })
    }
    fn invoke(&self, ctx: OperationContext, call: ToolCall) -> PortFuture<'_, ToolResult> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.actors.lock().unwrap().push(ctx.principal_id);
            assert_eq!(call.arguments, json!({"content":"hello"}));
            if self.unknown.load(Ordering::SeqCst) {
                return Err(PortError::OutcomeUnknown);
            }
            Ok(ToolResult {
                content: vec![ContentPart::Text("done".into())],
                structured_content: Some(json!({"accepted":true})),
                is_error: false,
            })
        })
    }
}
impl MessageSink for Fake {
    fn validate(&self, _: &Message) -> Result<(), PortError> {
        Ok(())
    }
    fn deliver(&self, _: OperationContext, r: DeliveryRequest) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            self.replies.lock().unwrap().push(r.message);
            if self.fail_reply.load(Ordering::SeqCst) {
                return Err(PortError::Failed {
                    code: "reply_failed",
                });
            }
            Ok(DeliveryReceipt {
                delivery_id: r.delivery_id,
                external_message_id: None,
                status: DeliveryStatus::Accepted,
            })
        })
    }
}
struct Provider(Arc<Fake>);
impl ConnectionActions for Provider {
    fn definitions(&self) -> Vec<ConnectionAction> {
        crate::infrastructure::connectors::qq::actions::catalog()
    }
    fn executor(&self, b: Option<RouteBinding>) -> Result<Arc<dyn ToolExecutor>, PortError> {
        if let Some(b) = b {
            assert_eq!(b.config["id"], "group_A");
            assert_eq!(b.config["msg_id"], "trusted-message");
        }
        Ok(self.0.clone())
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    store: Arc<SqliteStore>,
    bindings: Arc<Bindings>,
    sink: Arc<ActionSink>,
    fake: Arc<Fake>,
    ctx: AccessContext,
    route: ManagedRoute,
    event: EventEnvelope,
    routing: RoutingService,
    delivery: DeliveryService,
}
impl Fixture {
    async fn new(reply: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(
            SqliteStore::open(&dir.path().join("test.db"))
                .await
                .unwrap(),
        );
        let source = ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
        };
        let c = MessageConnection {
            recent_messages: Default::default(),
            id: source.connection_id,
            name: "QQ".into(),
            description: String::new(),
            enabled: true,
            config: ConnectionConfig::QqBot(QqBotConfig {
                resolved_secret: None,
                app_id: "12345".into(),
                group_openid: String::new(),
                allowed_senders: vec![],
                allow_proactive: false,
            }),
        };
        let target = ConnectionScope {
            workspace_id: source.workspace_id,
            connection_id: c.send_id(),
        };
        let route:ManagedRoute=serde_json::from_value(json!({"id":Uuid::new_v4(),"name":"command","enabled":true,"source_id":c.id,"event_type":null,"revision":1,
            "response":{"kind":if reply{"source"}else{"none"}},"processor":{"kind":"invoke_capability","tool_id":"qq.send_text","revision":1,"allowed_senders":["member_A"]},
            "command":{"text_pointer":"/content","trigger":"/send","mode":"prefix","output":{"kind":"json","event_type":"command.invoke","template":{"content":{"$command":"arguments"}}}},
            "routing":{"source":{"adapter":"qq","version":1,"config":{"mode":"all_groups"}},"targets":{c.send_id().to_string():{"adapter":"qq","version":1,"config":{"mode":"reply"}}}},
            "targets":[{"rule_id":Uuid::new_v4(),"destination_id":c.send_id()}]})).unwrap();
        let principal = Uuid::new_v4();
        let catalog = ConnectionSnapshot {
            revision: 1,
            connections: vec![c],
        };
        super::super::bindings::validate_routing(&catalog, &route, source.connection_id).unwrap();
        let bindings = Arc::new(Bindings {
            connections: Arc::new(RwLock::new(catalog)),
            source,
            sources: vec![source],
            destinations: vec![],
            principal,
            routes: RwLock::new(RouteSnapshot {
                revision: 1,
                routes: vec![route.clone()],
            }),
        });
        let fake = Arc::new(Fake::default());
        let sink = Arc::new(ActionSink {
            runtime: Arc::new(CommandRuntime {
                bindings: bindings.clone(),
                store: store.clone(),
            }),
            inner: fake.clone(),
            actions: Arc::new(Provider(fake.clone())),
            scope: target,
            validator: Arc::new(
                Schemas::compile(
                    crate::infrastructure::connectors::qq::actions::definitions()
                        .into_iter()
                        .map(|d| d.input_schema),
                )
                .unwrap(),
            ),
        });
        let mut registry = ConnectorRegistry::default();
        let mut registration = ConnectorRegistration::new(ConnectorDescriptor {
            scope: target,
            kind: ConnectorKind::Custom("qq".into()),
            display_name: "QQ".into(),
        });
        registration.sink = Some(sink.clone());
        registry.register(registration).unwrap();
        let governance = Arc::new(Governance::new(bindings.clone(), store.clone()));
        let routing = RoutingService::new(
            store.clone(),
            bindings.clone(),
            bindings.clone(),
            bindings.clone(),
            store.clone(),
            governance.clone(),
        );
        let delivery = DeliveryService::new(
            store.clone(),
            bindings.clone(),
            Arc::new(registry),
            governance,
        );
        let event = EventEnvelope {
            state_report: false,
            route_context: Some(RouteBinding::new(
                "qq",
                json!({"app_id":"12345","kind":"group","id":"group_A","msg_id":"trusted-message","sender_openid":"member_A"}),
            )),
            event_id: Uuid::new_v4(),
            scope: source,
            event_type: "qq.group.message".into(),
            schema_version: 1,
            occurred_at: None,
            received_at: SystemTime::now(),
            source_event_id: Some("trusted-message".into()),
            correlation_id: Uuid::new_v4(),
            causation_id: None,
            route_trace: vec![source],
            payload: EventPayload::Json(
                json!({"content":"/send hello","sender_openid":"forged","group_openid":"wrong","msg_id":"wrong"}),
            ),
        };
        let ctx = AccessContext {
            identity: Identity {
                mcp_endpoint: None,
                workspace_id: source.workspace_id,
                principal_id: principal,
                client_id: None,
                scopes: Default::default(),
                expires_at: None,
            },
            operation: OperationContext {
                scope: source,
                principal_id: principal,
                request_id: Uuid::new_v4(),
                correlation_id: event.correlation_id,
                causation_id: None,
                route_trace: vec![],
                deadline: None,
                cancellation: CancellationToken::new(),
            },
        };
        Self {
            dir,
            store,
            bindings,
            sink,
            fake,
            ctx,
            route,
            event,
            routing,
            delivery,
        }
    }
    async fn queue(&self) {
        self.store.append(self.event.clone()).await.unwrap();
        self.routing
            .route(&self.ctx, self.event.event_id)
            .await
            .unwrap();
    }
    fn job(&self) -> Message {
        CommandJob {
            route_id: self.route.id,
            revision: 1,
            source: self.event.scope,
            event_id: self.event.event_id,
        }
        .message()
        .unwrap()
    }
    async fn replay(&self) -> Result<DeliveryReceipt, PortError> {
        let mut ctx = self.ctx.operation.clone();
        ctx.scope = self.sink.scope;
        self.sink
            .deliver(
                ctx,
                DeliveryRequest {
                    delivery_id: Uuid::new_v4(),
                    destination_id: self.sink.scope.connection_id,
                    message: self.job(),
                },
            )
            .await
    }
    async fn run(&self) -> DeliveryRunReport {
        self.delivery
            .run_target(&self.ctx, self.sink.scope.connection_id, false)
            .await
            .unwrap()
            .unwrap()
    }
    fn count(&self) -> usize {
        self.fake.calls.load(Ordering::SeqCst)
    }
}
#[tokio::test]
async fn command_runs_once_and_reply_failure_cannot_repeat_the_operation() {
    let f = Fixture::new(true).await;
    f.queue().await;
    let duplicate = f.routing.route(&f.ctx, f.event.event_id).await.unwrap();
    assert!(duplicate.receipt.duplicate);
    assert!(matches!(
        f.run().await.outcome,
        DeliveryOutcome::Submitted(_)
    ));
    assert_eq!(f.count(), 1);
    assert!(f.fake.replies.lock().unwrap().is_empty());
    f.fake.fail_reply.store(true, Ordering::SeqCst);
    assert!(matches!(
        f.run().await.outcome,
        DeliveryOutcome::Failed { .. }
    ));
    assert!(f.replay().await.is_ok());
    assert_eq!(f.count(), 1);
    let replies = f.fake.replies.lock().unwrap();
    assert_eq!(replies.len(), 1);
    let binding = replies[0].destination_binding.as_ref().unwrap();
    assert_eq!(binding.config["msg_id"], "trusted-message");
    assert_eq!(binding.config["id"], "group_A");
    let [ContentPart::Json(v)] = replies[0].parts.as_slice() else {
        panic!()
    };
    assert_eq!(v["msg_seq"], 2);
    let db = rusqlite::Connection::open(f.dir.path().join("test.db")).unwrap();
    let actor = f.fake.actors.lock().unwrap()[0];
    assert_ne!(actor, f.ctx.identity.principal_id);
    let (principal,audits):(String,u64)=db.query_row("SELECT principal,(SELECT COUNT(*) FROM audit WHERE json_extract(record,'$.action')='InvokeTool' AND json_extract(record,'$.phase')='Succeeded') FROM tool_invocations",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(principal, actor.to_string());
    assert_eq!(audits, 1);
}
#[tokio::test]
async fn payload_identity_cannot_authorize_a_command() {
    let mut f = Fixture::new(false).await;
    f.event.route_context.as_mut().unwrap().config["sender_openid"] = json!("intruder");
    f.event.payload =
        EventPayload::Json(json!({"content":"/send hello","sender_openid":"member_A"}));
    f.queue().await;
    assert!(matches!(
        f.run().await.outcome,
        DeliveryOutcome::Failed { .. }
    ));
    assert_eq!(f.count(), 0);
    let db = rusqlite::Connection::open(f.dir.path().join("test.db")).unwrap();
    let count:u64=db.query_row("SELECT COUNT(*) FROM audit WHERE json_extract(record,'$.action')='InvokeTool' AND json_extract(record,'$.phase')='Denied'",[],|r|r.get(0)).unwrap();
    assert_eq!(count, 1);
}
#[tokio::test]
async fn schema_validation_precedes_external_effects() {
    let f = Fixture::new(false).await;
    f.bindings.routes.write().unwrap().routes[0]
        .command
        .as_mut()
        .unwrap()
        .output = CommandOutput::Json {
        event_type: "command.invoke".into(),
        template: json!({"content":23}),
    };
    f.queue().await;
    assert!(matches!(
        f.run().await.outcome,
        DeliveryOutcome::Failed { .. }
    ));
    assert_eq!(f.count(), 0);
}
#[tokio::test]
async fn stopping_or_editing_a_route_revokes_queued_calls() {
    for edit in [false, true] {
        let f = Fixture::new(false).await;
        f.queue().await;
        if edit {
            f.bindings.routes.write().unwrap().routes[0].revision += 1;
        } else {
            f.bindings.routes.write().unwrap().routes[0].enabled = false;
        }
        assert!(matches!(
            f.run().await.outcome,
            DeliveryOutcome::Failed { .. }
        ));
        assert_eq!(f.count(), 0);
    }
}
#[tokio::test]
async fn ambiguous_call_is_not_reexecuted_and_no_reply_is_required() {
    let f = Fixture::new(false).await;
    f.fake.unknown.store(true, Ordering::SeqCst);
    f.queue().await;
    assert!(matches!(f.run().await.outcome, DeliveryOutcome::Reconcile));
    assert_eq!(f.count(), 1);
    assert!(matches!(f.replay().await, Err(PortError::OutcomeUnknown)));
    assert_eq!(f.count(), 1);
    assert!(f.fake.replies.lock().unwrap().is_empty());
}
#[tokio::test]
async fn stored_result_survives_executor_restart() {
    let f = Fixture::new(false).await;
    f.queue().await;
    assert!(matches!(
        f.run().await.outcome,
        DeliveryOutcome::Submitted(_)
    ));
    let store = Arc::new(
        SqliteStore::open(&f.dir.path().join("test.db"))
            .await
            .unwrap(),
    );
    let runtime = Arc::new(CommandRuntime {
        bindings: f.bindings.clone(),
        store,
    });
    let sink = ActionSink {
        runtime,
        inner: f.fake.clone(),
        actions: Arc::new(Provider(f.fake.clone())),
        scope: f.sink.scope,
        validator: f.sink.validator.clone(),
    };
    let mut ctx = f.ctx.operation.clone();
    ctx.scope = sink.scope;
    sink.deliver(
        ctx,
        DeliveryRequest {
            delivery_id: Uuid::new_v4(),
            destination_id: sink.scope.connection_id,
            message: f.job(),
        },
    )
    .await
    .unwrap();
    assert_eq!(f.count(), 1);
}
#[tokio::test]
async fn validation_rejects_missing_grants_versions_and_implicit_arguments() {
    let f = Fixture::new(false).await;
    let catalog = f.bindings.connections.read().unwrap();
    for processor in [
        RouteProcessor::InvokeCapability {
            tool_id: "qq.send_text".into(),
            revision: 1,
            allowed_senders: vec![],
        },
        RouteProcessor::InvokeCapability {
            tool_id: "qq.send_text".into(),
            revision: 2,
            allowed_senders: vec!["member_A".into()],
        },
    ] {
        let mut r = f.route.clone();
        r.processor = Some(processor);
        assert!(validate(&catalog, &r, f.event.scope.connection_id).is_err());
    }
    let mut r = f.route.clone();
    r.command = None;
    assert!(validate(&catalog, &r, f.event.scope.connection_id).is_err());
    let mut r = f.route.clone();
    r.targets.push(RouteTarget {
        rule_id: Uuid::new_v4(),
        destination_id: Uuid::new_v4(),
    });
    assert!(validate(&catalog, &r, f.event.scope.connection_id).is_err());
}

#[tokio::test]
async fn direct_reply_uses_trusted_source_without_invoking_a_capability() {
    let f = Fixture::new(true).await;
    {
        let mut routes = f.bindings.routes.write().unwrap();
        let route = &mut routes.routes[0];
        route.processor = Some(RouteProcessor::Respond);
        super::super::bindings::validate_routing(
            &f.bindings.connections.read().unwrap(),
            route,
            f.event.scope.connection_id,
        )
        .unwrap();
    }
    f.queue().await;
    assert!(
        f.routing
            .route(&f.ctx, f.event.event_id)
            .await
            .unwrap()
            .receipt
            .duplicate
    );
    assert!(matches!(
        f.run().await.outcome,
        DeliveryOutcome::Submitted(_)
    ));
    assert_eq!(f.count(), 0);
    let replies = f.fake.replies.lock().unwrap();
    assert_eq!(replies.len(), 1);
    let binding = replies[0].destination_binding.as_ref().unwrap();
    assert_eq!(binding.config["id"], "group_A");
    assert_eq!(binding.config["msg_id"], "trusted-message");
    let [ContentPart::Json(value)] = replies[0].parts.as_slice() else {
        panic!()
    };
    assert_eq!(value["content"], "hello");
    assert_eq!(value["msg_seq"], 1);
}

#[tokio::test]
async fn source_response_requires_a_supported_processor_and_source_destination() {
    let f = Fixture::new(true).await;
    let catalog = f.bindings.connections.read().unwrap();
    let mut route = f.route.clone();
    route.processor = Some(RouteProcessor::Forward);
    assert!(validate(&catalog, &route, f.event.scope.connection_id).is_err());
    let mut route = f.route.clone();
    route.processor = Some(RouteProcessor::Respond);
    route.targets[0].destination_id = Uuid::new_v4();
    assert!(validate(&catalog, &route, f.event.scope.connection_id).is_err());
}
