use axum::{Json, Router, routing::post};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{CallToolRequestParams, ClientConfig, ProtocolVersion},
    transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use synquay_server::{
    AppState,
    application::{
        ports::{
            PortError,
            invocations::{Admission, InvocationKey, InvocationLedger, SavedOutcome},
            profiles::ProfileRepository,
        },
        services::tools::PublishedToolCall,
    },
    domain::{
        audit::{AuditPhase, AuditRecord},
        identity::{Action, Resource},
        tool::{ToolDefinition, ToolEffect},
    },
    infrastructure::{
        storage::SqliteStore,
        tool_runtime::{
            ToolRuntime,
            config::{HttpToolConfig, ToolsConfig},
        },
    },
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const TOKEN: &str = "test-only-token-with-at-least-32-characters";
fn config(path: std::path::PathBuf, url: String) -> ToolsConfig {
    let tool = HttpToolConfig {
        public_name: "echo".into(),
        connection_id: Uuid::from_u128(4),
        definition: ToolDefinition {
            id: "echo-operation".into(),
            description: "Echo a validated value".into(),
            input_schema: json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"],"additionalProperties":false}),
            output_schema: Some(
                json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]}),
            ),
            effect: ToolEffect::SideEffecting,
            documentation: None,
        },
        url,
        bearer_env: None,
        exposed: true,
        allow_loopback_http: true,
    };
    let mut hidden = tool.clone();
    hidden.public_name = "hidden".into();
    hidden.connection_id = Uuid::from_u128(5);
    hidden.exposed = false;
    ToolsConfig {
        mcp_content: Default::default(),
        database: path,
        workspace_id: Uuid::from_u128(1),
        profile_id: Uuid::from_u128(2),
        principal_id: Uuid::from_u128(3),
        token_env: "TEST_TOKEN".into(),
        revision: 1,
        tools: vec![tool, hidden],
    }
}
async fn build(config: ToolsConfig) -> Arc<ToolRuntime> {
    let n = config.tools.len();
    ToolRuntime::build_with_secrets(config, TOKEN.into(), vec![None; n])
        .await
        .unwrap()
}
async fn serve(router: Router) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let token = stop.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(token.cancelled_owned())
            .await
            .unwrap();
    });
    (url, stop, task)
}
fn call(id: Uuid, name: &str, value: i64) -> PublishedToolCall {
    PublishedToolCall {
        profile_id: Uuid::from_u128(2),
        revision: 1,
        public_name: name.into(),
        invocation_id: id,
        arguments: json!({"value":value}),
    }
}
fn mcp_call(id: Uuid, name: &str, value: i64) -> CallToolRequestParams {
    let mut call = CallToolRequestParams::new(name.to_owned())
        .with_arguments(json!({"value":value}).as_object().unwrap().clone());
    call.meta = Some(serde_json::from_value(json!({"synquay/invocation-id":id})).unwrap());
    call
}

#[tokio::test]
async fn official_client_discovers_calls_and_replays_without_another_http_effect() {
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    let (upstream, upstop, uptask) = serve(Router::new().route(
        "/echo",
        post(move |Json(value): Json<Value>| {
            let hits = hits.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                Json(value)
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path().join("tools.db"), format!("{upstream}/echo"));
    cfg.tools[0].definition.documentation = Some(serde_json::from_value(json!({
        "when_to_use":["Check a configured echo service"],
        "preconditions":["Supply an integer"],
        "examples":[{"description":"Echo seven", "arguments":{"value":7}}],
        "errors":[{"code":"outcome_unknown","description":"Result uncertain","recovery":"Reconcile before retry"}]
    })).unwrap());
    let runtime = build(cfg.clone()).await;
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime.clone()),
        ..Default::default()
    }))
    .await;
    let endpoint = format!("{base}/mcp");
    let http = reqwest::Client::new();
    assert_eq!(http.post(&endpoint).send().await.unwrap().status(), 401);
    assert_eq!(
        http.post(&endpoint)
            .bearer_auth(TOKEN)
            .header("origin", "https://evil.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        http.post(&endpoint)
            .bearer_auth(TOKEN)
            .body(vec![b'x'; 65537])
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    let client = ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint.clone()).auth_header(TOKEN),
        ))
        .await
        .unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo__r1");
    let guidance = tools[0].description.as_deref().unwrap();
    assert!(guidance.contains("Check a configured echo service"));
    assert!(guidance.contains("Echo seven"));
    assert!(guidance.contains("Reconcile before retry"));
    assert!(!guidance.contains(&upstream));
    let id = Uuid::new_v4();
    let response = client.call_tool(mcp_call(id, "echo__r1", 7)).await.unwrap();
    assert_eq!(response.is_error, Some(false));
    assert_eq!(response.structured_content, Some(json!({"value":7})));
    let meta = serde_json::to_value(response.meta).unwrap();
    assert_eq!(meta["synquay/invocation-id"], id.to_string());
    assert_eq!(meta["synquay/audit-recorded"], true);
    let response = client.call_tool(mcp_call(id, "echo__r1", 7)).await.unwrap();
    assert_eq!(response.is_error, Some(false));
    assert_eq!(
        client
            .call_tool(mcp_call(id, "echo__r1", 8))
            .await
            .unwrap()
            .is_error,
        Some(true)
    );
    assert_eq!(
        client
            .call_tool(mcp_call(Uuid::new_v4(), "hidden__r1", 7))
            .await
            .unwrap()
            .is_error,
        Some(true)
    );
    assert_eq!(
        client
            .call_tool(mcp_call(Uuid::new_v4(), "echo__r0", 7))
            .await
            .unwrap()
            .is_error,
        Some(true)
    );
    let bad = CallToolRequestParams::new("echo__r1")
        .with_arguments(json!({"value":"bad"}).as_object().unwrap().clone());
    assert_eq!(client.call_tool(bad).await.unwrap().is_error, Some(true));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    client.cancel().await.unwrap();
    let modern = ClientConfig::default()
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(endpoint.clone()).auth_header(TOKEN),
            ),
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    assert_eq!(modern.list_all_tools().await.unwrap().len(), 1);
    modern.cancel().await.unwrap();
    let legacy = ClientConfig::default()
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(TOKEN),
        ))
        .await
        .unwrap();
    assert_eq!(
        legacy.peer_info().unwrap().protocol_version,
        ProtocolVersion::V_2025_11_25
    );
    assert_eq!(legacy.list_all_tools().await.unwrap().len(), 1);
    assert_eq!(
        legacy
            .call_tool(mcp_call(id, "echo__r1", 7))
            .await
            .unwrap()
            .is_error,
        Some(false)
    );
    legacy.cancel().await.unwrap();
    stop.cancel();
    task.await.unwrap();
    drop(runtime);
    let runtime = build(cfg.clone()).await;
    let report = runtime
        .service
        .invoke(
            &runtime.context(CancellationToken::new()),
            call(id, "echo", 7),
        )
        .await
        .unwrap();
    assert!(report.outcome.is_ok());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let c = rusqlite::Connection::open(&cfg.database).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM tool_invocations WHERE state='completed'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM audit WHERE json_extract(record, '$.action')='InvokeTool'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM tool_publications", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    let data: String = c
        .query_row("SELECT record FROM tool_releases", [], |r| r.get(0))
        .unwrap();
    assert!(!data.contains(TOKEN));
    upstop.cancel();
    uptask.await.unwrap();
}

#[tokio::test]
async fn concurrent_duplicate_and_unknown_outcome_do_not_reexecute() {
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    let (url, stop, task) = serve(Router::new().route(
        "/echo",
        post(move || {
            let hits = hits.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(100)).await;
                Json(json!({"value":1}))
            }
        }),
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path().join("tools.db"), format!("{url}/echo"));
    let runtime = build(cfg.clone()).await;
    let id = Uuid::new_v4();
    let ctx = runtime.context(CancellationToken::new());
    let (a, b) = tokio::join!(
        runtime.service.invoke(&ctx, call(id, "echo", 1)),
        runtime.service.invoke(&ctx, call(id, "echo", 1))
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert!(a.outcome.is_ok() || b.outcome.is_ok());
    assert!(
        matches!(a.outcome, Err(PortError::OutcomeUnknown))
            || matches!(b.outcome, Err(PortError::OutcomeUnknown))
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let id2 = Uuid::new_v4();
    let runtime2 = runtime.clone();
    let pending = tokio::spawn(async move {
        runtime2
            .service
            .invoke(
                &runtime2.context(CancellationToken::new()),
                call(id2, "echo", 1),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while count.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    pending.abort();
    let _ = pending.await;
    drop(runtime);
    let runtime = build(cfg).await;
    let result = runtime
        .service
        .invoke(
            &runtime.context(CancellationToken::new()),
            call(id2, "echo", 1),
        )
        .await
        .unwrap();
    assert!(matches!(result.outcome, Err(PortError::OutcomeUnknown)));
    assert_eq!(count.load(Ordering::SeqCst), 2);
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn publication_is_cas_atomic_and_revocation_takes_effect_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(
        dir.path().join("tools.db"),
        "http://127.0.0.1:1/echo".into(),
    );
    let runtime = build(cfg.clone()).await;
    assert!(matches!(
        ToolRuntime::build_with_secrets(cfg.clone(), TOKEN.into(), vec![None; 2]).await,
        Err(PortError::Conflict)
    ));
    drop(runtime);
    let mut invalid = cfg.clone();
    invalid.revision = 2;
    invalid.tools[0].definition.input_schema = json!({"$ref":"http://127.0.0.1:1/schema"});
    assert!(
        ToolRuntime::build_with_secrets(invalid, TOKEN.into(), vec![None; 2])
            .await
            .is_err()
    );
    cfg.tools[0].exposed = false;
    assert!(matches!(
        ToolRuntime::build_with_secrets(cfg.clone(), TOKEN.into(), vec![None; 2]).await,
        Err(PortError::Conflict)
    ));
    cfg.revision = 2;
    let runtime = build(cfg.clone()).await;
    let ctx = runtime.context(CancellationToken::new());
    assert!(
        runtime
            .service
            .list(&ctx, cfg.profile_id)
            .await
            .unwrap()
            .tools
            .is_empty()
    );
    let mut request = call(Uuid::new_v4(), "echo", 1);
    request.revision = 2;
    assert!(matches!(
        runtime.service.invoke(&ctx, request).await,
        Err(PortError::NotFound)
    ));
    drop(runtime);
    cfg.revision = 1;
    assert!(matches!(
        ToolRuntime::build_with_secrets(cfg.clone(), TOKEN.into(), vec![None; 2]).await,
        Err(PortError::Conflict)
    ));
    let store = SqliteStore::open(&cfg.database).await.unwrap();
    assert_eq!(
        store
            .published(cfg.workspace_id, cfg.profile_id)
            .await
            .unwrap()
            .revision,
        2
    );
    let mut wrong = cfg.clone();
    wrong.revision = 3;
    wrong.tools[0].url = "http://example.com/unsafe".into();
    assert!(wrong.validate().is_err());
}

#[tokio::test]
async fn ledger_terminal_result_and_audit_rollback_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tools.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let key = InvocationKey {
        workspace_id: Uuid::new_v4(),
        principal_id: Uuid::new_v4(),
        invocation_id: Uuid::new_v4(),
    };
    assert!(matches!(
        store
            .begin(key.clone(), "fingerprint".into())
            .await
            .unwrap(),
        Admission::Fresh
    ));
    let audit = AuditRecord {
        record_id: Uuid::new_v4(),
        operation_id: key.invocation_id,
        request_id: Uuid::new_v4(),
        workspace_id: key.workspace_id,
        principal_id: key.principal_id,
        action: Action::InvokeTool,
        resource: Resource::Profile {
            workspace_id: key.workspace_id,
            profile_id: Uuid::new_v4(),
        },
        phase: AuditPhase::Failed,
        occurred_at: std::time::SystemTime::now(),
    };
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(
        "CREATE TRIGGER audit_block BEFORE INSERT ON audit BEGIN SELECT RAISE(ABORT,'test'); END;",
    )
    .unwrap();
    assert!(
        store
            .finish(key.clone(), SavedOutcome::Failed, audit.clone())
            .await
            .is_err()
    );
    assert!(matches!(
        store
            .begin(key.clone(), "fingerprint".into())
            .await
            .unwrap(),
        Admission::Cached(SavedOutcome::Unknown)
    ));
    c.execute_batch("DROP TRIGGER audit_block").unwrap();
    store
        .finish(key.clone(), SavedOutcome::Failed, audit)
        .await
        .unwrap();
    assert!(matches!(
        store.begin(key, "fingerprint".into()).await.unwrap(),
        Admission::Cached(SavedOutcome::Failed)
    ));
}

#[tokio::test]
async fn migration_v5_failure_rolls_back_to_v4() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let c = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../src/infrastructure/storage/schema.sql"),
        include_str!("../src/infrastructure/storage/migration_v2.sql"),
        include_str!("../src/infrastructure/storage/migration_v3.sql"),
        include_str!("../src/infrastructure/storage/migration_v4.sql"),
    ] {
        c.execute_batch(sql).unwrap();
    }
    c.execute_batch("CREATE TABLE tool_invocations(collision INTEGER)")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='tool_releases'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn oversized_invalid_and_redirected_responses_never_trigger_automatic_retry() {
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    let router = Router::new()
        .route(
            "/target",
            post(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"value":1}))
                }
            }),
        )
        .route(
            "/redirect",
            post(|| async { axum::response::Redirect::temporary("/target") }),
        )
        .route(
            "/large",
            post(|| async { Json(json!({"value":1,"padding":"x".repeat(65536)})) }),
        )
        .route("/invalid", post(|| async { "invalid-json" }))
        .route("/schema", post(|| async { Json(json!({"value":"wrong"})) }));
    let (url, stop, task) = serve(router).await;
    let dir = tempfile::tempdir().unwrap();
    for path in ["redirect", "large", "invalid", "schema"] {
        let runtime = build(config(
            dir.path().join(format!("{path}.db")),
            format!("{url}/{path}"),
        ))
        .await;
        let ctx = runtime.context(CancellationToken::new());
        let id = Uuid::new_v4();
        let first = runtime
            .service
            .invoke(&ctx, call(id, "echo", 1))
            .await
            .unwrap();
        assert!(first.audit_recorded);
        assert!(first.outcome.is_err());
        let second = runtime
            .service
            .invoke(&ctx, call(id, "echo", 1))
            .await
            .unwrap();
        assert!(second.outcome.is_err());
        let mut wrong = runtime.context(CancellationToken::new());
        wrong.identity.principal_id = Uuid::new_v4();
        wrong.operation.principal_id = wrong.identity.principal_id;
        assert!(matches!(
            runtime
                .service
                .list(&wrong, runtime.config.profile_id)
                .await,
            Err(PortError::PermissionDenied)
        ));
    }
    assert_eq!(count.load(Ordering::SeqCst), 0);
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn ledger_quota_is_reserved_before_execution_and_completion_can_release_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = SqliteStore::open(&path).await.unwrap();
    let key = InvocationKey {
        workspace_id: Uuid::new_v4(),
        principal_id: Uuid::new_v4(),
        invocation_id: Uuid::new_v4(),
    };
    store
        .begin(key.clone(), "fingerprint".into())
        .await
        .unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("SELECT bytes FROM tool_invocation_usage", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        263168
    );
    // Simulate capacity consumed by other calls; replay must still work when full.
    c.execute("UPDATE tool_invocation_usage SET bytes=1073741824", [])
        .unwrap();
    let mut next = key.clone();
    next.invocation_id = Uuid::new_v4();
    assert!(matches!(
        store.begin(next, "fingerprint".into()).await,
        Err(PortError::CapacityExceeded { .. })
    ));
    assert!(matches!(
        store
            .begin(key.clone(), "fingerprint".into())
            .await
            .unwrap(),
        Admission::Cached(SavedOutcome::Unknown)
    ));
    let audit = AuditRecord {
        record_id: Uuid::new_v4(),
        operation_id: key.invocation_id,
        request_id: Uuid::new_v4(),
        workspace_id: key.workspace_id,
        principal_id: key.principal_id,
        action: Action::InvokeTool,
        resource: Resource::Profile {
            workspace_id: key.workspace_id,
            profile_id: Uuid::new_v4(),
        },
        phase: AuditPhase::Failed,
        occurred_at: std::time::SystemTime::now(),
    };
    store
        .finish(key, SavedOutcome::Failed, audit)
        .await
        .unwrap();
    assert!(
        c.query_row("SELECT bytes FROM tool_invocation_usage", [], |r| r
            .get::<_, u64>(0))
            .unwrap()
            < 1073741824
    );
    let plan:String=c.query_row("EXPLAIN QUERY PLAN SELECT fingerprint,outcome FROM tool_invocations WHERE workspace='w' AND principal='p' AND invocation='i'",[],|r|r.get(3)).unwrap();
    assert!(plan.contains("SEARCH") && plan.contains("INDEX"));
}

#[tokio::test]
async fn invalid_documentation_examples_are_rejected_before_runtime_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(
        dir.path().join("tools.db"),
        "http://127.0.0.1:8091/echo".into(),
    );
    cfg.tools[0].definition.documentation = Some(
        serde_json::from_value(json!({
            "examples":[{"description":"Incorrect example", "arguments":{"value":"not an integer"}}]
        }))
        .unwrap(),
    );
    let count = cfg.tools.len();
    assert!(matches!(
        ToolRuntime::build_with_secrets(cfg, TOKEN.into(), vec![None; count]).await,
        Err(PortError::InvalidInput {
            code: "schema_validation"
        })
    ));
}

#[tokio::test]
async fn documented_http_example_builds_with_the_existing_config_format() {
    let mut cfg: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools-documented.json")).unwrap();
    let directory = tempfile::tempdir().unwrap();
    cfg.database = directory.path().join("tools.db");
    let count = cfg.tools.len();
    assert!(
        ToolRuntime::build_with_secrets(cfg, TOKEN.into(), vec![None; count])
            .await
            .is_ok()
    );
}
