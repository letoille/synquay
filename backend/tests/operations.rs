mod support;
use axum::{Json, Router, http::StatusCode, routing::post};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use support::*;
use synquay_server::{
    AppState,
    domain::event::{EventPayload, IncomingEvent},
    infrastructure::pipeline::{Pipeline, config::TargetConfig},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
const ADMIN: &str = "test-operator-token-0123456789abcdef";
fn input(id: &str) -> IncomingEvent {
    IncomingEvent {
        route_context: None,
        source_event_id: Some(id.into()),
        event_type: "test".into(),
        schema_version: 1,
        occurred_at: None,
        payload: EventPayload::Json(json!({"secret_payload":"must-not-appear-in-ops"})),
    }
}
async fn serve(router: Router) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let stop = CancellationToken::new();
    let t = stop.clone();
    let h = tokio::spawn(async move {
        axum::serve(l, router)
            .with_graceful_shutdown(t.cancelled_owned())
            .await
            .unwrap()
    });
    (url, stop, h)
}
fn extra(url: &str) -> TargetConfig {
    TargetConfig {
        rule_id: Uuid::from_u128(60),
        connection_id: Uuid::from_u128(30),
        destination_id: Uuid::from_u128(40),
        url: url.into(),
        token_env: None,
        allow_loopback_http: true,
    }
}
#[tokio::test]
async fn slow_target_does_not_hold_other_target_and_shutdown_drains() {
    let dir = tempfile::tempdir().unwrap();
    let release = Arc::new(tokio::sync::Notify::new());
    let started = Arc::new(tokio::sync::Notify::new());
    let (slow, ss, sh) = serve(Router::new().route(
        "/",
        post({
            let release = release.clone();
            let started = started.clone();
            move || {
                let release = release.clone();
                let started = started.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    StatusCode::OK
                }
            }
        }),
    ))
    .await;
    let (sent, mut recv) = tokio::sync::mpsc::channel(8);
    let (fast, fs, fh) = serve(Router::new().route(
        "/",
        post(move |Json(v): Json<Value>| {
            let sent = sent.clone();
            async move {
                sent.send(v).await.unwrap();
                StatusCode::OK
            }
        }),
    ))
    .await;
    let mut cfg = config(&dir.path().join("a.db"), &slow);
    cfg.additional_targets.push(extra(&fast));
    cfg.scheduler.min_interval_ms = 0;
    let p = Arc::new(Pipeline::build(cfg, TOKEN.into(), None).await.unwrap());
    p.accept(Uuid::new_v4(), input("1")).await.unwrap();
    p.accept(Uuid::new_v4(), input("2")).await.unwrap();
    let stop = CancellationToken::new();
    let worker = tokio::spawn(p.run(stop.clone()));
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(5), recv.recv())
            .await
            .unwrap()
            .unwrap();
    }
    stop.cancel();
    tokio::task::yield_now().await;
    assert!(!worker.is_finished());
    release.notify_one();
    worker.await.unwrap();
    ss.cancel();
    fs.cancel();
    sh.await.unwrap();
    fh.await.unwrap();
}
#[tokio::test]
async fn global_concurrency_bound_and_per_target_rate_limit() {
    let dir = tempfile::tempdir().unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let times = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let (url, stop, server) = serve(Router::new().route(
        "/",
        post({
            let active = active.clone();
            let peak = peak.clone();
            let times = times.clone();
            move || {
                let active = active.clone();
                let peak = peak.clone();
                let times = times.clone();
                async move {
                    let n = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(n, Ordering::SeqCst);
                    times.lock().await.push(Instant::now());
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }
        }),
    ))
    .await;
    let mut cfg = config(&dir.path().join("a.db"), &url);
    cfg.additional_targets.push(extra(&url));
    cfg.scheduler.max_in_flight = 1;
    cfg.scheduler.min_interval_ms = 0;
    let p = Pipeline::build(cfg, TOKEN.into(), None).await.unwrap();
    for id in ["1", "2", "3"] {
        p.accept(Uuid::new_v4(), input(id)).await.unwrap();
    }
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 1);
    assert_eq!(times.lock().await.len(), 6);
    let mut cfg = config(&dir.path().join("b.db"), &url);
    cfg.scheduler.min_interval_ms = 80;
    let p = Pipeline::build(cfg, TOKEN.into(), None).await.unwrap();
    times.lock().await.clear();
    for id in ["1", "2", "3"] {
        p.accept(Uuid::new_v4(), input(id)).await.unwrap();
    }
    p.tick(&CancellationToken::new()).await.unwrap();
    let t = times.lock().await;
    assert_eq!(t.len(), 3);
    assert!(
        t.windows(2)
            .all(|x| x[1].duration_since(x[0]) >= Duration::from_millis(65))
    );
    drop(t);
    stop.cancel();
    server.await.unwrap();
}
#[tokio::test]
async fn breaker_blocks_then_probes_without_retrying_unknown_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let (url, stop, server) = serve(Router::new().route(
        "/",
        post({
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }
            }
        }),
    ))
    .await;
    let mut cfg = config(&dir.path().join("a.db"), &url);
    cfg.scheduler.failure_threshold = 1;
    cfg.scheduler.cooldown_ms = 200;
    cfg.scheduler.min_interval_ms = 0;
    let principal = Uuid::new_v4();
    let p = Pipeline::build(cfg, TOKEN.into(), None)
        .await
        .unwrap()
        .with_management(principal, ADMIN.into())
        .unwrap();
    for id in ["1", "2"] {
        p.accept(Uuid::new_v4(), input(id)).await.unwrap();
    }
    p.tick(&CancellationToken::new()).await.unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(230)).await;
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let ops = p.operations(principal).await.unwrap();
    assert_eq!(ops["runtime"]["reconcile"], 1);
    assert_eq!(ops["runtime"]["submitted"], 1);
    assert_eq!(ops["database"]["pending_items"], 1);
    assert_eq!(ops["targets"][0]["consecutive_failures"], 0);
    stop.cancel();
    server.await.unwrap();
}
#[tokio::test]
async fn management_is_separately_authorized_and_recovery_is_reviewable() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let p = Arc::new(
        Pipeline::build(
            config(&dir.path().join("a.db"), "http://127.0.0.1:9"),
            TOKEN.into(),
            None,
        )
        .await
        .unwrap()
        .with_management(Uuid::from_u128(90), ADMIN.into())
        .unwrap(),
    );
    p.accept(Uuid::new_v4(), input("1")).await.unwrap();
    p.tick(&CancellationToken::new()).await.unwrap();
    let app = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    let get = |token: &str| {
        Request::get("/api/ops")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(get(TOKEN)).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let response = app.clone().oneshot(get(ADMIN)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("must-not-appear"));
    assert!(!text.contains(ADMIN));
    let v: Value = serde_json::from_str(&text).unwrap();
    let case = &v["database"]["recovery_cases"][0];
    let cmd = json!({"command_id":Uuid::new_v4(),"target":{"kind":"delivery","delivery_id":case["delivery_id"]},"expected_state":case["state"],"expected_revision":case["revision"],"action":"abandon","reason":"operator abandons uncertain work"});
    let request = |token: &str| {
        Request::post("/api/ops/recover")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(cmd.to_string()))
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(request(TOKEN)).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone().oneshot(request(ADMIN)).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone().oneshot(request(ADMIN)).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        p.operations(Uuid::from_u128(90)).await.unwrap()["database"]["pending_items"],
        0
    );
    let inspect = Request::get(format!(
        "/api/ops/recovery/delivery/{}",
        case["delivery_id"].as_str().unwrap()
    ))
    .header("authorization", format!("Bearer {ADMIN}"))
    .body(Body::empty())
    .unwrap();
    let response = app.oneshot(inspect).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(detail["case"]["resolution"], "abandoned");
    assert_eq!(detail["recovery_history"].as_array().unwrap().len(), 1);
    assert_eq!(detail["attempts"].as_array().unwrap().len(), 1);
    assert!(p.operations(Uuid::new_v4()).await.is_err());
}
