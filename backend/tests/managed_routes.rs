mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::*;
use synquay_server::{
    AppState,
    application::ports::PortError,
    domain::{
        event::{EventPayload, IncomingEvent},
        managed_route::*,
    },
    infrastructure::{
        pipeline::{Pipeline, config::TargetConfig},
        tool_runtime::{ToolRuntime, config::ToolsConfig, security::SecurityConfig},
    },
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use uuid::Uuid;

async fn accept(p: &Pipeline, kind: &str) {
    p.accept(
        Uuid::new_v4(),
        IncomingEvent {
            route_context: None,
            event_type: kind.into(),
            schema_version: 1,
            occurred_at: None,
            source_event_id: Some(Uuid::new_v4().to_string()),
            payload: EventPayload::Json(json!({"value":42})),
        },
    )
    .await
    .unwrap();
}
fn save(revision: u64, id: Uuid, enabled: bool, targets: Vec<Uuid>) -> RouteInput {
    RouteInput {
        operation_id: Uuid::new_v4(),
        expected_revision: revision,
        route_id: id,
        change: RouteChange::Save {
            response: None,
            routing: None,
            source_id: None,
            processor: None,
            command: None,
            name: "设备消息".into(),
            enabled,
            event_type: Some("device.command".into()),
            destination_ids: targets,
        },
    }
}
#[tokio::test]
async fn managed_paths_filter_fanout_stop_resume_delete_and_restart_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipeline.db");
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let sink = Router::new().route(
        "/",
        axum::routing::post(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                axum::http::StatusCode::ACCEPTED
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, sink).await.unwrap() });
    let make_config = || {
        let mut cfg = config(&path, &url);
        cfg.additional_targets.push(TargetConfig {
            connection_id: Uuid::from_u128(30),
            destination_id: Uuid::from_u128(40),
            rule_id: Uuid::from_u128(60),
            url: url.clone(),
            token_env: None,
            allow_loopback_http: true,
        });
        cfg
    };
    let p = Arc::new(
        Pipeline::build(make_config(), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let actor = Uuid::from_u128(10);
    let id = Uuid::from_u128(6);
    let targets = vec![Uuid::from_u128(4), Uuid::from_u128(40)];
    let input = save(1, id, true, targets.clone());
    let mut omitted = serde_json::to_value(&input).unwrap();
    omitted["change"].as_object_mut().unwrap().remove("enabled");
    assert!(matches!(
        serde_json::from_value::<RouteInput>(omitted)
            .unwrap()
            .change,
        RouteChange::Save { enabled: true, .. }
    ));

    p.change_route(actor, input.clone()).await.unwrap();
    accept(&p, "other").await;
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    accept(&p, "device.command").await;
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(
        p.routes_view().await.unwrap().matched[&id],
        1,
        "fanout counts once per route"
    );
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(
        p.routes_view().await.unwrap().matched[&id],
        1,
        "repeat dispatch cannot increment"
    );
    assert_eq!(
        p.routes_view()
            .await
            .unwrap()
            .counts
            .values()
            .map(|c| c.get("submitted").copied().unwrap_or(0))
            .sum::<u64>(),
        2
    );
    // Rebuild a v18 fixture with retained history, then verify aggregate backfill.
    drop(p);
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("DROP TRIGGER count_received; DROP TRIGGER count_submitted; DROP TABLE message_totals; ALTER TABLE deliveries DROP COLUMN counted_submitted; PRAGMA user_version=18;").unwrap();
    let p = Arc::new(
        Pipeline::build(make_config(), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    assert_eq!(p.routes_view().await.unwrap().matched[&id], 1);
    for (kind, expected) in [("received", 2), ("sent", 2), ("matched", 1)] {
        let total: u64 = sql
            .query_row(
                "SELECT SUM(total) FROM message_totals WHERE kind=?1",
                [kind],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, expected, "backfilled {kind}");
    }
    let plan: String = sql.query_row("EXPLAIN QUERY PLAN SELECT total FROM message_totals WHERE workspace='w' AND kind='matched' AND id='r'", [], |r| r.get(3)).unwrap();
    assert!(plan.contains("PRIMARY KEY"), "{plan}");
    p.change_route(actor, save(2, id, false, targets.clone()))
        .await
        .unwrap();
    accept(&p, "device.command").await;
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    p.change_route(actor, save(3, id, true, targets))
        .await
        .unwrap();
    // Replaying an old receipt must not reactivate its historical snapshot.
    assert_eq!(p.change_route(actor, input).await.unwrap().revision, 2);
    assert_eq!(p.routes_view().await.unwrap().snapshot.revision, 4);
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    p.change_route(
        actor,
        RouteInput {
            operation_id: Uuid::new_v4(),
            expected_revision: 4,
            route_id: id,
            change: RouteChange::Delete,
        },
    )
    .await
    .unwrap();
    drop(p);
    let p = Arc::new(
        Pipeline::build(make_config(), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    assert!(p.routes_view().await.unwrap().snapshot.routes.is_empty());
    accept(&p, "device.command").await;
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    task.abort();
}
#[tokio::test]
async fn route_changes_are_atomic_scoped_idempotent_and_compare_and_swap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let cfg = config(&path, "http://127.0.0.1:1/");
    let p = Arc::new(
        Pipeline::build(config(&path, "http://127.0.0.1:1/"), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let actor = Uuid::new_v4();
    let input = save(1, cfg.rule_id, false, vec![cfg.destination_id]);
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_route BEFORE INSERT ON route_operations BEGIN SELECT RAISE(ABORT,'test'); END").unwrap();
    assert!(p.change_route(actor, input.clone()).await.is_err());
    assert!(p.routes_view().await.unwrap().snapshot.routes[0].enabled);
    sql.execute_batch("DROP TRIGGER reject_route").unwrap();
    let (a, b) = tokio::join!(
        p.change_route(actor, input.clone()),
        p.change_route(actor, save(1, cfg.rule_id, true, vec![cfg.destination_id]))
    );
    assert!(a.is_ok() != b.is_ok());
    assert_eq!(p.routes_view().await.unwrap().snapshot.revision, 2);
    let good = save(2, cfg.rule_id, false, vec![cfg.destination_id]);
    p.change_route(actor, good.clone()).await.unwrap();
    let mut changed = good.clone();
    changed.change = RouteChange::Delete;
    assert!(matches!(
        p.change_route(actor, changed).await,
        Err(PortError::Conflict)
    ));
    assert!(
        p.route_operation(Uuid::new_v4(), good.operation_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        p.change_route(Uuid::new_v4(), good).await,
        Err(PortError::Conflict)
    ));
    let invalid = save(3, cfg.rule_id, true, vec![Uuid::new_v4()]);
    assert!(matches!(
        p.change_route(actor, invalid).await,
        Err(PortError::InvalidInput { .. })
    ));
    drop(p);
    let p = Pipeline::build(cfg, TOKEN.into(), None).await.unwrap();
    assert!(!p.routes_view().await.unwrap().snapshot.routes[0].enabled);
}
#[tokio::test]
async fn routes_api_enforces_origin_workspace_and_reports_missing_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let mut tools: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    tools.database = dir.path().join("tools");
    let runtime = ToolRuntime::build_configured(
        tools,
        TOKEN.into(),
        vec![None],
        SecurityConfig::default(),
        None,
    )
    .await
    .unwrap();
    let app = synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime.clone()),
        ..Default::default()
    });
    let response = app
        .clone()
        .oneshot(Request::get("/api/routes").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["reason"], "pipeline_not_configured");
    assert_eq!(
        app.oneshot(
            Request::post("/api/routes")
                .header("origin", "https://evil.test")
                .header("host", "local.test")
                .body(Body::from("{}"))
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        403
    );
    let mut cfg = config(&dir.path().join("pipeline"), "http://127.0.0.1:1/");
    cfg.workspace_id = Uuid::new_v4();
    let p = Arc::new(Pipeline::build(cfg, TOKEN.into(), None).await.unwrap());
    let app = synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime),
        pipeline: Some(p),
        ..Default::default()
    });
    assert_ne!(
        app.oneshot(Request::get("/api/routes").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn independent_webhook_sources_isolate_identical_event_ids_and_agent_drafts_cannot_run() {
    use synquay_server::infrastructure::pipeline::config::SourceConfig;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multi.db");
    let second = Uuid::from_u128(200);
    let actor = Uuid::from_u128(10);
    let make_config = || {
        let mut cfg = config(&path, "http://127.0.0.1:1/");
        cfg.additional_sources = vec![SourceConfig {
            connection_id: second,
            name: "状态上报".into(),
        }];
        cfg
    };
    let p = Arc::new(
        Pipeline::build(make_config(), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    // Stop bootstrap rule, then route only events received on the second endpoint.
    p.change_route(
        actor,
        save(1, Uuid::from_u128(6), false, vec![Uuid::from_u128(4)]),
    )
    .await
    .unwrap();
    let mut input = save(2, Uuid::new_v4(), true, vec![Uuid::from_u128(4)]);
    if let RouteChange::Save { source_id, .. } = &mut input.change {
        *source_id = Some(second);
    }
    p.change_route(actor, input.clone()).await.unwrap();
    let app = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    let body=json!({"source_event_id":"same-upstream-id","event_type":"device.command","payload":{"value":1}}).to_string();
    let post = |source: Uuid| {
        Request::post(format!("/api/webhooks/{source}"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap()
    };
    let first = app
        .clone()
        .oneshot(post(p.source.connection_id))
        .await
        .unwrap();
    assert_eq!(first.status(), 202);
    let second_response = app.clone().oneshot(post(second)).await.unwrap();
    assert_eq!(second_response.status(), 202);
    let a: Value =
        serde_json::from_slice(&to_bytes(first.into_body(), 65536).await.unwrap()).unwrap();
    let b: Value =
        serde_json::from_slice(&to_bytes(second_response.into_body(), 65536).await.unwrap())
            .unwrap();
    assert_ne!(a["event_id"], b["event_id"]);
    let dup = app.clone().oneshot(post(second)).await.unwrap();
    let dup: Value =
        serde_json::from_slice(&to_bytes(dup.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(dup["event_id"], b["event_id"]);
    assert_eq!(dup["disposition"], "duplicate");
    assert_eq!(
        app.oneshot(post(Uuid::new_v4())).await.unwrap().status(),
        404
    );
    p.tick(&CancellationToken::new()).await.unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM deliveries", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        1,
        "only the matching source may dispatch"
    );
    // A legacy client omitting source must retain an existing route's source.
    let legacy = save(3, input.route_id, false, vec![Uuid::from_u128(4)]);
    p.change_route(actor, legacy).await.unwrap();
    assert_eq!(
        p.routes_view()
            .await
            .unwrap()
            .snapshot
            .routes
            .iter()
            .find(|r| r.id == input.route_id)
            .unwrap()
            .source_id,
        Some(second)
    );
    let mut draft = save(4, Uuid::new_v4(), false, vec![Uuid::from_u128(4)]);
    if let RouteChange::Save {
        source_id,
        processor,
        ..
    } = &mut draft.change
    {
        *source_id = Some(second);
        *processor = Some(RouteProcessor::AgentDraft {
            name: "分析助手".into(),
        });
    }
    p.change_route(actor, draft.clone()).await.unwrap();
    let enable = save(5, draft.route_id, true, vec![Uuid::from_u128(4)]);
    assert!(matches!(
        p.change_route(actor, enable).await,
        Err(PortError::InvalidInput {
            code: "agent_executor_not_configured"
        })
    ));
    drop(p);
    let p = Pipeline::build(make_config(), TOKEN.into(), None)
        .await
        .unwrap();
    assert_eq!(p.routes_view().await.unwrap().sources.len(), 2);
    assert!(
        Pipeline::build(config(&path, "http://127.0.0.1:1/"), TOKEN.into(), None)
            .await
            .is_err(),
        "cannot orphan a removed source's events"
    );
}

#[tokio::test]
async fn additional_hmac_source_signature_is_bound_to_its_url() {
    use synquay_server::infrastructure::pipeline::{
        config::SourceConfig,
        webhook::{WebhookAuth, WebhookFormat},
    };
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(&dir.path().join("signed"), "http://127.0.0.1:1/");
    let id = Uuid::new_v4();
    cfg.additional_sources = vec![SourceConfig {
        connection_id: id,
        name: "签名回调".into(),
    }];
    cfg.webhook.authentication = WebhookAuth::HmacSha256;
    cfg.webhook.format = WebhookFormat::Json {
        event_type: "test".into(),
        id_header: "x-delivery-id".into(),
    };
    let p = Pipeline::build(cfg, TOKEN.into(), None).await.unwrap();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let body = b"{}";
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, TOKEN.as_bytes());
    let wire = format!("v1\n{id}\n{timestamp}\nmessage\n{{}}");
    let signature = format!(
        "v1={}",
        ring::hmac::sign(&key, wire.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    assert!(p.verifies_webhook_from(id, &timestamp, "message", &signature, body));
    assert!(!p.verifies_webhook(&timestamp, "message", &signature, body));
}

#[tokio::test]
async fn route_api_filters_before_paging_and_preserves_global_revision_and_catalog() {
    use synquay_server::infrastructure::pipeline::config::SourceConfig;
    let dir = tempfile::tempdir().unwrap();
    let mut tools: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    tools.database = dir.path().join("tools");
    let mut cfg = config(&dir.path().join("pipeline"), "http://127.0.0.1:1/");
    cfg.workspace_id = tools.workspace_id;
    let source = cfg.source_connection_id;
    let second = Uuid::from_u128(200);
    let empty = Uuid::from_u128(201);
    cfg.additional_sources = vec![
        SourceConfig {
            connection_id: second,
            name: "App".into(),
        },
        SourceConfig {
            connection_id: empty,
            name: "Empty".into(),
        },
    ];
    let target = cfg.destination_id;
    let p = Arc::new(Pipeline::build(cfg, TOKEN.into(), None).await.unwrap());
    let mut expected = Vec::new();
    for index in 0..6 {
        let id = Uuid::new_v4();
        let mut input = save(index + 1, id, false, vec![target]);
        if let RouteChange::Save { source_id, .. } = &mut input.change {
            *source_id = Some(if index % 2 == 0 { second } else { source });
        }
        if index % 2 == 0 {
            expected.push(id.to_string());
        }
        p.change_route(Uuid::from_u128(10), input).await.unwrap();
    }
    let runtime = ToolRuntime::build_configured(
        tools,
        TOKEN.into(),
        vec![None],
        SecurityConfig::default(),
        None,
    )
    .await
    .unwrap();
    let app = synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime),
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    async fn get(app: &Router, path: String) -> (u16, Value) {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        (status, value)
    }
    let (status, first) = get(
        &app,
        format!("/api/routes?source_ids={second}&page=1&page_size=2"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        first["pagination"],
        json!({"page":1,"page_size":2,"total":3,"total_pages":2})
    );
    assert_eq!(first["snapshot"]["revision"], 7);
    assert_eq!(
        first["sources"].as_array().unwrap().len(),
        3,
        "catalog must include unselected/empty sources"
    );
    let (_, last) = get(
        &app,
        format!("/api/routes?source_ids={second}&page=2&page_size=2"),
    )
    .await;
    let ids: Vec<_> = first["snapshot"]["routes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(last["snapshot"]["routes"].as_array().unwrap())
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        ids, expected,
        "filter before paging; stable order with no overlap"
    );
    let (_, union) = get(
        &app,
        format!("/api/routes?source_ids={source},{second}&page_size=64"),
    )
    .await;
    assert_eq!(union["pagination"]["total"], 7);
    let (_, none) = get(&app, format!("/api/routes?source_ids={empty}")).await;
    assert_eq!(none["pagination"]["total"], 0);
    assert_eq!(none["snapshot"]["routes"], json!([]));
    let (_, beyond) = get(
        &app,
        format!("/api/routes?source_ids={second}&page=99&page_size=2"),
    )
    .await;
    assert_eq!(beyond["pagination"]["total"], 3);
    assert_eq!(beyond["snapshot"]["routes"], json!([]));
    let (_, all) = get(&app, "/api/routes".into()).await;
    assert_eq!(
        all["snapshot"]["routes"].as_array().unwrap().len(),
        7,
        "legacy unfiltered request still returns full snapshot"
    );
    for query in [
        "page=0".into(),
        "page_size=0".into(),
        "page_size=65".into(),
        "page=-1".into(),
        "page=no".into(),
        "page=18446744073709551615&page_size=64".into(),
        "page=1&page=2".into(),
        "source_ids=".into(),
        "source_ids=bad".into(),
        format!("source_ids={second},{second}"),
        format!("source_ids={}", Uuid::new_v4()),
        "other=1".into(),
    ] {
        let (status, body) = get(&app, format!("/api/routes?{query}")).await;
        assert_eq!(status, 400, "{query}: {body}");
        assert_eq!(body["code"], "invalid_route_query");
    }
    assert_eq!(
        p.routes_view().await.unwrap().snapshot.routes.len(),
        7,
        "reading pages must not shrink active routing snapshot"
    );
}

#[tokio::test]
async fn command_routes_map_payloads_deduplicate_and_survive_restart_and_legacy_edits() {
    use std::sync::Mutex;
    use synquay_server::{application::ports::routing::EventReader, domain::command::*};
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = received.clone();
    let sink = Router::new().route(
        "/",
        axum::routing::post(move |axum::Json(v): axum::Json<Value>| {
            capture.lock().unwrap().push(v);
            async { axum::http::StatusCode::ACCEPTED }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, sink).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("commands.db");
    let actor = Uuid::new_v4();
    let p = Arc::new(
        Pipeline::build(config(&db, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let id = Uuid::from_u128(6);
    let mut input = save(1, id, true, vec![Uuid::from_u128(4)]);
    if let RouteChange::Save { command, .. } = &mut input.change {
        *command = Some(Some(Box::new(CommandMapping {
            trigger: "/on".into(),
            text_pointer: "/text".into(),
            mode: CommandMode::Prefix,
            output: CommandOutput::Json {
                event_type: "device.control".into(),
                template: json!({"action":"on","device":{"$command":"arguments"},"level":{"$payload":"/level"}}),
            },
        })));
    }
    p.change_route(actor, input.clone()).await.unwrap();
    // Explicit omission in a legacy save must not silently remove a command restriction.
    p.change_route(actor, save(2, id, true, vec![Uuid::from_u128(4)]))
        .await
        .unwrap();
    assert!(
        p.routes_view().await.unwrap().snapshot.routes[0]
            .command
            .is_some()
    );
    drop(p);
    let p = Arc::new(
        Pipeline::build(config(&db, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let event = |text: &str, upstream: &str| IncomingEvent {
        route_context: None,
        source_event_id: Some(upstream.into()),
        event_type: "device.command".into(),
        schema_version: 1,
        occurred_at: None,
        payload: EventPayload::Json(json!({"text":text,"level":2})),
    };
    p.accept(Uuid::new_v4(), event("/online", "miss"))
        .await
        .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    assert!(received.lock().unwrap().is_empty());
    let receipt = p
        .accept(Uuid::new_v4(), event("/on heater", "one"))
        .await
        .unwrap();
    p.accept(Uuid::new_v4(), event("/on heater", "one"))
        .await
        .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    {
        let values = received.lock().unwrap();
        assert_eq!(values.len(), 1);
        let out = &values[0]["parts"][0]["value"];
        assert_eq!(out["event_type"], "device.control");
        assert_eq!(
            out["payload"],
            json!({"action":"on","device":"heater","level":2})
        );
        assert_eq!(out["source_event_id"], "one");
    }
    let store = synquay_server::infrastructure::storage::SqliteStore::open(&db)
        .await
        .unwrap();
    let original = store.load(p.source, receipt.event_id).await.unwrap();
    assert_eq!(original.event_type, "device.command");
    let EventPayload::Json(original) = original.payload else {
        panic!()
    };
    assert_eq!(original["text"], "/on heater");
    // Explicit null removes it, including across serialize/deserialize of the HTTP input.
    let mut remove = save(3, id, true, vec![Uuid::from_u128(4)]);
    if let RouteChange::Save { command, .. } = &mut remove.change {
        *command = Some(None);
    }
    let remove: RouteInput = serde_json::from_value(serde_json::to_value(remove).unwrap()).unwrap();
    p.change_route(actor, remove).await.unwrap();
    assert!(
        p.routes_view().await.unwrap().snapshot.routes[0]
            .command
            .is_none()
    );
    let mut agent_input = save(4, id, true, vec![Uuid::from_u128(4)]);
    if let RouteChange::Save {
        processor, command, ..
    } = &mut agent_input.change
    {
        *processor = Some(RouteProcessor::AgentHttp {
            name: "家庭助手".into(),
        });
        *command = Some(Some(Box::new(CommandMapping {
            trigger: "/助手".into(),
            text_pointer: "/text".into(),
            mode: CommandMode::Prefix,
            output: CommandOutput::Json {
                event_type: "agent.request".into(),
                template: json!({"request":{"$command":"arguments"}}),
            },
        })));
    }
    p.change_route(actor, agent_input).await.unwrap();
    p.accept(Uuid::new_v4(), event("/助手 打开热水器", "agent-http"))
        .await
        .unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(received.lock().unwrap().len(), 2);
    assert_eq!(
        received.lock().unwrap()[1]["parts"][0]["value"]["payload"],
        json!({"request":"打开热水器"})
    );
    server.abort();
}

#[tokio::test]
async fn package_initialization_can_start_without_an_active_demo_route() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = config(
        &dir.path().join("empty.db"),
        "http://127.0.0.1:9/unconfigured",
    );
    settings.bootstrap_routes = false;
    let pipeline = Pipeline::build(settings, "w".repeat(32), None)
        .await
        .unwrap();
    assert!(
        pipeline
            .routes_view()
            .await
            .unwrap()
            .snapshot
            .routes
            .is_empty()
    );
    drop(pipeline);
    // Restart with the legacy default must not resurrect routes in an existing store.
    let config = config(
        &dir.path().join("empty.db"),
        "http://127.0.0.1:9/unconfigured",
    );
    let pipeline = Pipeline::build(config, "w".repeat(32), None).await.unwrap();
    assert!(
        pipeline
            .routes_view()
            .await
            .unwrap()
            .snapshot
            .routes
            .is_empty()
    );
}
