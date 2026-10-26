mod support;
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use synquay_server::{
    AppState,
    application::ports::{PortError, agent::AgentJobStore},
    domain::{
        agent::*,
        event::{EventPayload, IncomingEvent},
    },
    infrastructure::{
        pipeline::Pipeline,
        storage::SqliteStore,
        tool_runtime::{ToolRuntime, config::ToolsConfig},
    },
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use uuid::Uuid;
const MCP_TOKEN: &str = "test-agent-mcp-token-at-least-32-characters";
async fn setup(path: &Path, url: &str) -> (Arc<Pipeline>, Arc<ToolRuntime>) {
    setup_command(path, url, None).await
}
async fn setup_command(
    path: &Path,
    url: &str,
    command: Option<synquay_server::domain::command::CommandMapping>,
) -> (Arc<Pipeline>, Arc<ToolRuntime>) {
    let mut cfg = support::config(path, url);
    cfg.agent = Some(AgentConfig {
        command,
        profile_id: Uuid::from_u128(20),
        lease_seconds: 30,
        deadline_seconds: 600,
        max_attempts: 3,
        max_in_flight: 2,
    });
    let p = Arc::new(
        Pipeline::build(cfg, support::TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let mut c: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    c.database = path.into();
    c.profile_id = Uuid::from_u128(20);
    c.principal_id = Uuid::from_u128(5);
    c.tools.clear();
    c.agent_jobs = true;
    let t = ToolRuntime::build_with_secrets(c, MCP_TOKEN.into(), vec![])
        .await
        .unwrap();
    (p, t)
}
async fn enqueue(p: &Pipeline, id: &str) -> Uuid {
    let r = p
        .accept(
            Uuid::new_v4(),
            IncomingEvent {
                route_context: None,
                source_event_id: Some(id.into()),
                event_type: "chat.message".into(),
                schema_version: 1,
                occurred_at: None,
                payload: EventPayload::Json(
                    json!({"text":"请总结这条消息","destination":"https://attacker.invalid"}),
                ),
            },
        )
        .await
        .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    r.event_id
}
async fn serve(router: Router) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let s = stop.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(s.cancelled_owned())
            .await
            .unwrap()
    });
    (url, stop, task)
}
#[tokio::test]
async fn webhook_to_official_mcp_client_to_original_adapter_is_durable_and_deduplicated() {
    use rmcp::{
        ServiceExt,
        model::{CallToolRequestParams, ClientConfig},
        transport::streamable_http_client::{
            StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
        },
    };
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sink = received.clone();
    let (destination, ds, dt) = serve(Router::new().route(
        "/messages",
        post(move |Json(v): Json<Value>| {
            let sink = sink.clone();
            async move {
                sink.lock().unwrap().push(v);
                StatusCode::ACCEPTED
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let (p, t) = setup(&path, &format!("{destination}/messages")).await;
    let app = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        tools: Some(t.clone()),
        ..Default::default()
    });
    let body=json!({"source_event_id":"chat-42","event_type":"chat.message","payload":{"text":"summarize","target_url":"https://attacker.invalid"}}).to_string();
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/webhooks/00000000-0000-0000-0000-000000000002")
                    .header("authorization", format!("Bearer {}", support::TOKEN))
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        let _: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    }
    p.tick(&CancellationToken::new()).await.unwrap();
    assert!(received.lock().unwrap().is_empty());
    let (endpoint, stop, server) = serve(app).await;
    let client = ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(format!("{endpoint}/mcp"))
                .auth_header(MCP_TOKEN),
        ))
        .await
        .unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 6);
    let claim_id = Uuid::new_v4();
    let request = CallToolRequestParams::new("synquay_jobs_claim__v1")
        .with_arguments(json!({"claim_id":claim_id}).as_object().unwrap().clone());
    let first = client.call_tool(request.clone()).await.unwrap();
    assert_ne!(first.is_error, Some(true));
    let claim: ClaimedJob =
        serde_json::from_value(first.structured_content.unwrap()["claim"].clone()).unwrap();
    let retried = client.call_tool(request).await.unwrap();
    assert_eq!(
        retried.structured_content.unwrap()["claim"]["token"]["job_id"],
        claim.token.job_id.to_string()
    );
    let mut args = json!({"token":claim.token,"text":"已分析：这是一条总结请求。"});
    args["destination_url"] = json!("https://attacker.invalid");
    let bad = client
        .call_tool(
            CallToolRequestParams::new("synquay_jobs_complete__v1")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(bad.is_error, Some(true));
    args.as_object_mut().unwrap().remove("destination_url");
    let req = CallToolRequestParams::new("synquay_jobs_complete__v1")
        .with_arguments(args.as_object().unwrap().clone());
    let completed = client.call_tool(req.clone()).await.unwrap();
    assert_ne!(completed.is_error, Some(true));
    let result = completed.structured_content.unwrap();
    let duplicate = client
        .call_tool(req)
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(result["delivery_id"], duplicate["delivery_id"]);
    assert_eq!(duplicate["duplicate"], true);
    p.tick(&CancellationToken::new()).await.unwrap();
    let messages = received.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["parts"][0]["value"]["source_event_id"],
        "chat-42"
    );
    assert_eq!(messages[0]["parts"][0]["value"]["type"], "agent.reply");
    client.cancel().await.unwrap();
    stop.cancel();
    server.await.unwrap();
    drop(t);
    drop(p);
    let (p, t) = setup(&path, &format!("{destination}/messages")).await;
    let ctx = t.context(CancellationToken::new());
    let agent = t.agent.as_ref().unwrap();
    let again = agent
        .complete(
            &ctx,
            claim.token.clone(),
            "已分析：这是一条总结请求。".into(),
        )
        .await
        .unwrap();
    assert!(again.duplicate);
    assert!(matches!(
        agent.complete(&ctx, claim.token, "different".into()).await,
        Err(PortError::Conflict)
    ));
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(received.lock().unwrap().len(), 1);
    let jobs = agent.list(&ctx, None).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].delivery_state.as_deref(), Some("submitted"));
    ds.cancel();
    dt.await.unwrap();
}
#[tokio::test]
async fn leases_are_exclusive_retryable_fenced_and_deadline_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let (p, t) = setup(&path, "http://127.0.0.1:1/").await;
    let id = enqueue(&p, "one").await;
    let ctx = t.context(CancellationToken::new());
    let a = t.agent.as_ref().unwrap();
    let (x, y) = tokio::join!(a.claim(&ctx, Uuid::new_v4()), a.claim(&ctx, Uuid::new_v4()));
    let mut claims = vec![x.unwrap(), y.unwrap()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(claims.len(), 1);
    let first = claims.pop().unwrap();
    let renewed = a.renew(&ctx, first.token.clone()).await.unwrap();
    assert!(renewed.lease_until_ms >= first.lease_until_ms);
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute("UPDATE agent_jobs SET lease_until_ms=1", [])
        .unwrap();
    assert!(matches!(
        a.complete(&ctx, first.token.clone(), "stale".into()).await,
        Err(PortError::Conflict)
    ));
    let next = a.claim(&ctx, Uuid::new_v4()).await.unwrap().unwrap();
    assert_eq!(next.token.generation, 2);
    assert_eq!(next.token.job_id, id);
    assert!(a.renew(&ctx, first.token).await.is_err());
    a.fail(&ctx, next.token, true).await.unwrap();
    assert!(a.claim(&ctx, Uuid::new_v4()).await.unwrap().is_none());
    sql.execute("UPDATE agent_jobs SET due_ms=0", []).unwrap();
    let final_claim = a.claim(&ctx, Uuid::new_v4()).await.unwrap().unwrap();
    assert_eq!(final_claim.token.generation, 3);
    a.fail(&ctx, final_claim.token, true).await.unwrap();
    assert_eq!(a.read(&ctx, id).await.unwrap().summary.state, "failed");
    assert!(a.claim(&ctx, Uuid::new_v4()).await.unwrap().is_none());
    let id2 = enqueue(&p, "two").await;
    sql.execute(
        "UPDATE agent_jobs SET deadline_ms=1 WHERE job_id=?1",
        [id2.to_string()],
    )
    .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(a.read(&ctx, id2).await.unwrap().summary.state, "expired");
    let store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        store
            .backlog_status(Uuid::from_u128(1))
            .await
            .unwrap()
            .pending_items,
        0
    );
}
#[tokio::test]
async fn completion_audit_is_atomic_and_membership_rechecked_inside_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let (p, t) = setup(&path, "http://127.0.0.1:1/").await;
    enqueue(&p, "one").await;
    let ctx = t.context(CancellationToken::new());
    let a = t.agent.as_ref().unwrap();
    let claim = a.claim(&ctx, Uuid::new_v4()).await.unwrap().unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER audit_failure BEFORE INSERT ON audit BEGIN SELECT RAISE(ABORT,'test audit unavailable'); END").unwrap();
    assert!(
        a.complete(&ctx, claim.token.clone(), "reply".into())
            .await
            .is_err()
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM deliveries", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row("SELECT state FROM agent_jobs", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "claimed"
    );
    sql.execute_batch("DROP TRIGGER audit_failure; UPDATE workspace_members SET state='revoked'")
        .unwrap();
    assert!(matches!(
        a.complete(&ctx, claim.token.clone(), "reply".into()).await,
        Err(PortError::PermissionDenied)
    ));
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(matches!(
        AgentJobStore::complete(&store, ctx.clone(), claim.token.clone(), "reply".into()).await,
        Err(PortError::PermissionDenied)
    ));
    sql.execute_batch("UPDATE workspace_members SET state='active',role='viewer'")
        .unwrap();
    assert!(a.read(&ctx, claim.token.job_id).await.is_ok());
    assert!(matches!(
        a.renew(&ctx, claim.token.clone()).await,
        Err(PortError::PermissionDenied)
    ));
    let mut cross = ctx.clone();
    cross.identity.workspace_id = Uuid::new_v4();
    cross.operation.scope.workspace_id = cross.identity.workspace_id;
    assert!(a.read(&cross, claim.token.job_id).await.is_err());
    sql.execute_batch("UPDATE workspace_members SET role='owner'")
        .unwrap();
    a.complete(&ctx, claim.token, "reply".into()).await.unwrap();
}
#[tokio::test]
async fn schema_upgrade_rolls_back_and_queue_queries_use_indexes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let pipeline = synquay_server::infrastructure::pipeline::config::PipelineConfig::load(
        &root.join("pipeline-agent.json"),
    )
    .unwrap();
    let tools = ToolsConfig::load(&root.join("tools-agent.json")).unwrap();
    tools.validate().unwrap();
    assert_eq!(pipeline.agent.unwrap().profile_id, tools.profile_id);
    assert!(tools.agent_jobs);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = SqliteStore::open(&path).await.unwrap();
    drop(store);
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("DROP TABLE connection_secrets; DROP TABLE connection_conversations; DROP INDEX routing_unresolved_source; DROP TABLE message_connection_operations; DROP TABLE message_connections; DROP TABLE managed_routes; DROP TABLE route_operations; DROP INDEX deliveries_by_route_state; ALTER TABLE deliveries DROP COLUMN route_rule_id; DROP TABLE tool_connection_changes; DROP TABLE tool_connection_overrides; DROP TABLE mcp_server_changes; DROP TABLE mcp_servers; DROP TABLE agent_claims; DROP TABLE agent_jobs; PRAGMA user_version=10;")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        10
    );
    assert!(c.prepare("SELECT * FROM agent_jobs").is_err());
    c.execute_batch("DROP TABLE agent_binding").unwrap();
    SqliteStore::open(&path).await.unwrap();
    for query in [
        "SELECT job_id FROM agent_jobs WHERE workspace='w' AND profile='p' AND state='queued' AND due_ms<2 ORDER BY due_ms,job_id LIMIT 1",
        "SELECT job_id FROM agent_jobs WHERE workspace='w' AND profile='p' AND job_id>'a' ORDER BY job_id LIMIT 50",
        "SELECT job_id FROM agent_jobs WHERE workspace='w' AND state='claimed' AND deadline_ms<2 ORDER BY deadline_ms LIMIT 100",
    ] {
        let mut q = c.prepare(&format!("EXPLAIN QUERY PLAN {query}")).unwrap();
        let plan = q
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(plan.contains("INDEX"), "{plan}");
        assert!(!plan.contains("SCAN agent_jobs"), "{plan}");
    }
}

#[tokio::test]
async fn full_ingress_budget_still_allows_completion_and_failed_http_is_not_blindly_resent() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    let (url, stop, server) = serve(Router::new().route(
        "/messages",
        post(move || {
            let c = counter.clone();
            async move {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let (p, t) = setup(&path, &format!("{url}/messages")).await;
    let store = SqliteStore::open(&path).await.unwrap();
    let limits = synquay_server::infrastructure::storage::capacity::CapacityLimits {
        max_pending_items: 1,
        ..Default::default()
    };
    store.configure_capacity(limits).await.unwrap();
    enqueue(&p, "one").await;
    let rejected = p
        .accept(
            Uuid::new_v4(),
            IncomingEvent {
                route_context: None,
                source_event_id: Some("two".into()),
                event_type: "chat.message".into(),
                schema_version: 1,
                occurred_at: None,
                payload: EventPayload::Json(json!({"text":"second"})),
            },
        )
        .await;
    assert!(matches!(rejected, Err(PortError::CapacityExceeded { .. })));
    let ctx = t.context(CancellationToken::new());
    let a = t.agent.as_ref().unwrap();
    let claim = a.claim(&ctx, Uuid::new_v4()).await.unwrap().unwrap();
    // Control characters expand during JSON encoding: ensure the pre-reserved reply budget covers them.
    let result = a
        .complete(&ctx, claim.token, "\u{0001}".repeat(8192))
        .await
        .unwrap();
    for _ in 0..2 {
        p.tick(&CancellationToken::new()).await.unwrap();
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        store
            .delivery_state(Uuid::from_u128(1), result.delivery_id)
            .await
            .unwrap(),
        "reconcile"
    );
    assert_eq!(
        a.read(&ctx, result.job_id).await.unwrap().summary.state,
        "completed"
    );
    stop.cancel();
    server.await.unwrap();
}

#[tokio::test]
async fn oauth_job_access_is_explicit_and_claims_bind_to_client() {
    use synquay_server::infrastructure::tool_runtime::security::{
        OAuthConfig, OAuthGrant, SecurityConfig,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let (p, t) = setup(&path, "http://127.0.0.1:1/").await;
    enqueue(&p, "one").await;
    let cfg = t.config.clone();
    drop(t);
    let grant = |client: u128, enabled: bool| OAuthGrant {
        agent_jobs: enabled,
        resources: vec![],
        prompts: vec![],
        tools: vec![],
        subject: format!("subject-{client}"),
        oauth_client_id: format!("client-{client}"),
        principal_id: Uuid::from_u128(client + 100),
        client_id: Uuid::from_u128(client),
    };
    let security = SecurityConfig {
        admin: None,
        oauth: Some(OAuthConfig {
            issuer: "https://issuer.example/".into(),
            resource: "https://synquay.example/mcp".into(),
            introspection_endpoint: "https://issuer.example/introspect".into(),
            introspection_client_id: "synquay".into(),
            introspection_secret_env: "TEST_SECRET".into(),
            required_scope: "synquay".into(),
            allow_loopback_http: false,
            grants: vec![grant(31, true), grant(32, false)],
        }),
    };
    let t =
        ToolRuntime::build_configured(cfg, String::new(), vec![], security, Some("secret".into()))
            .await
            .unwrap();
    // Existing workspace bootstrap does not automatically enroll newly configured users.
    let sql = rusqlite::Connection::open(&path).unwrap();
    for principal in [131, 132] {
        sql.execute(
            "INSERT OR IGNORE INTO workspace_members VALUES(?1,?2,'operator','active')",
            [
                Uuid::from_u128(1).to_string(),
                Uuid::from_u128(principal).to_string(),
            ],
        )
        .unwrap();
    }
    let a = t.agent.as_ref().unwrap();
    let mut ctx = t.context(CancellationToken::new());
    ctx.identity.principal_id = Uuid::from_u128(132);
    ctx.operation.principal_id = ctx.identity.principal_id;
    ctx.identity.client_id = Some(Uuid::from_u128(32));
    ctx.identity.scopes.insert("synquay".into());
    assert!(matches!(
        a.list(&ctx, None).await,
        Err(PortError::PermissionDenied)
    ));
    ctx.identity.principal_id = Uuid::from_u128(131);
    ctx.operation.principal_id = ctx.identity.principal_id;
    ctx.identity.client_id = Some(Uuid::from_u128(31));
    let claim = a.claim(&ctx, Uuid::new_v4()).await.unwrap().unwrap();
    let store = SqliteStore::open(&path).await.unwrap();
    let mut other = ctx.clone();
    other.identity.client_id = Some(Uuid::from_u128(32));
    assert!(matches!(
        AgentJobStore::complete(&store, other, claim.token.clone(), "wrong client".into()).await,
        Err(PortError::Conflict)
    ));
    a.complete(&ctx, claim.token, "authorized reply".into())
        .await
        .unwrap();
}

#[tokio::test]
async fn command_maps_into_agent_job_and_replies_via_trusted_binding() {
    use synquay_server::domain::command::*;
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sink = received.clone();
    let (url, stop, server) = serve(Router::new().route(
        "/",
        post(move |Json(v): Json<Value>| {
            sink.lock().unwrap().push(v);
            async { StatusCode::ACCEPTED }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent-command.db");
    let mapping = CommandMapping {
        trigger: "/助手".into(),
        text_pointer: "/text".into(),
        mode: CommandMode::Prefix,
        output: CommandOutput::Json {
            event_type: "assistant.request".into(),
            template: json!({"request":{"$command":"arguments"},"category":"home"}),
        },
    };
    let (p, t) = setup_command(&path, &url, Some(mapping.clone())).await;
    let event = |text: &str, key: &str| IncomingEvent {
        route_context: None,
        source_event_id: Some(key.into()),
        event_type: "chat.message".into(),
        schema_version: 1,
        occurred_at: None,
        payload: EventPayload::Json(json!({"text":text,"destination":"https://untrusted.invalid"})),
    };
    p.accept(Uuid::new_v4(), event("/助手别名", "ignored"))
        .await
        .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    let ctx = t.context(CancellationToken::new());
    assert!(
        t.agent
            .as_ref()
            .unwrap()
            .claim(&ctx, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        SqliteStore::open(&path)
            .await
            .unwrap()
            .backlog_status(p.source.workspace_id)
            .await
            .unwrap()
            .pending_items,
        0
    );
    p.accept(
        Uuid::new_v4(),
        event("/助手 今晚八点开启热水器", "request-1"),
    )
    .await
    .unwrap();
    p.accept(
        Uuid::new_v4(),
        event("/助手 今晚八点开启热水器", "request-1"),
    )
    .await
    .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    drop(t);
    drop(p);
    let (p, t) = setup_command(&path, &url, Some(mapping)).await;
    let ctx = t.context(CancellationToken::new());
    let agent = t.agent.as_ref().unwrap();
    let claim = agent.claim(&ctx, Uuid::new_v4()).await.unwrap().unwrap();
    assert_eq!(claim.job.event_type.as_deref(), Some("assistant.request"));
    assert_eq!(
        claim.job.input,
        Some(json!({"request":"今晚八点开启热水器","category":"home"}))
    );
    assert_eq!(claim.job.source_event_id.as_deref(), Some("request-1"));
    assert!(agent.claim(&ctx, Uuid::new_v4()).await.unwrap().is_none());
    agent
        .complete(&ctx, claim.token, "收到请求，等待处理。".into())
        .await
        .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    {
        let messages = received.lock().unwrap();
        assert_eq!(messages.len(), 1);
        let out = &messages[0]["parts"][0]["value"];
        assert_eq!(out["source_event_id"], "request-1");
        assert_eq!(out["text"], "收到请求，等待处理。");
    }
    stop.cancel();
    server.await.unwrap();
}
