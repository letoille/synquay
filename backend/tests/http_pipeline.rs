mod support;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::*;
use synquay_server::{
    AppState,
    application::ports::{
        OperationContext, PortError, delivery::DeliveryOutbox, messages::MessageSink,
        routing::EventReader,
    },
    domain::{
        delivery::DispatchKey,
        message::{ContentPart, DeliveryRequest, Message},
    },
    infrastructure::{connectors::http_sink::HttpSink, pipeline::Pipeline, storage::SqliteStore},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

async fn serve(router: Router) -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap();
    });
    (url, stop, task)
}

#[tokio::test]
async fn failed_destination_backpressure_survives_restart_and_duplicates_still_ack() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("overload.db");
    let count = Arc::new(AtomicUsize::new(0));
    let (url, sink_stop, sink_task) = serve(Router::new().route(
        "/",
        post({
            let count = count.clone();
            move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    StatusCode::SERVICE_UNAVAILABLE
                }
            }
        }),
    ))
    .await;
    let make_config = || {
        let mut cfg = config(&path, &url);
        cfg.capacity.max_pending_items = 2;
        cfg
    };
    let pipeline = Arc::new(
        Pipeline::build(make_config(), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let make_router = |pipeline: Arc<Pipeline>| {
        synquay_server::interfaces::http::router(AppState {
            pipeline: Some(pipeline),
            ..Default::default()
        })
    };
    let request = |id: &str| {
        Request::post("/api/webhooks/00000000-0000-0000-0000-000000000002")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"source_event_id":id,"event_type":"test","payload":{}}).to_string(),
            ))
            .unwrap()
    };
    for id in ["first", "second"] {
        assert_eq!(
            make_router(pipeline.clone())
                .oneshot(request(id))
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
    }
    pipeline.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let rejected = make_router(pipeline.clone())
        .oneshot(request("new"))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(rejected.headers()["retry-after"], "10");
    drop(pipeline);
    let pipeline = Arc::new(
        Pipeline::build(make_config(), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    assert_eq!(
        make_router(pipeline.clone())
            .oneshot(request("first"))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        make_router(pipeline.clone())
            .oneshot(request("new"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    pipeline.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "unknown sends must not repeat"
    );
    let store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        store
            .backlog_status(pipeline.source.workspace_id)
            .await
            .unwrap()
            .pending_items,
        2
    );
    sink_stop.cancel();
    sink_task.await.unwrap();
}

#[tokio::test]
async fn blocked_destination_does_not_block_routing_and_shutdown_drains_send() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("isolated.db");
    let release = Arc::new(tokio::sync::Notify::new());
    let (sent, mut received) = tokio::sync::mpsc::channel(4);
    let router = Router::new().route(
        "/",
        post({
            let release = release.clone();
            move |Json(body): Json<Value>| {
                let sent = sent.clone();
                let release = release.clone();
                async move {
                    sent.send(body).await.unwrap();
                    release.notified().await;
                    StatusCode::ACCEPTED
                }
            }
        }),
    );
    let (url, sink_stop, sink_task) = serve(router).await;
    let pipeline = Arc::new(
        Pipeline::build(config(&path, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let input = |id: &str| synquay_server::domain::event::IncomingEvent {
        route_context: None,
        source_event_id: Some(id.into()),
        event_type: "test".into(),
        schema_version: 1,
        occurred_at: None,
        payload: synquay_server::domain::event::EventPayload::Json(json!({"text":id})),
    };
    pipeline
        .accept(Uuid::new_v4(), input("first"))
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let worker = tokio::spawn(pipeline.clone().run(stop.clone()));
    let first = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap();
    let second = pipeline
        .accept(Uuid::new_v4(), input("second"))
        .await
        .unwrap();
    let store = SqliteStore::open(&path).await.unwrap();
    // The first HTTP request remains held by the receiver, while the second event
    // must leave the unrouted set. No latency race against a sleeping receiver.
    let routed = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if !store
                .unrouted(pipeline.source.workspace_id, 100)
                .await
                .unwrap()
                .iter()
                .any(|(_, id)| *id == second.event_id)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    stop.cancel();
    tokio::task::yield_now().await;
    let drained_early = worker.is_finished();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap();
    sink_stop.cancel();
    sink_task.await.unwrap();
    assert!(
        routed.is_ok(),
        "routing was blocked by an in-flight delivery"
    );
    assert!(!drained_early, "shutdown dropped a live send");
    assert!(
        received.try_recv().is_err(),
        "single-target sends must remain serial"
    );
    let delivery = Uuid::parse_str(first["delivery_id"].as_str().unwrap()).unwrap();
    assert_eq!(
        store
            .delivery_state(pipeline.source.workspace_id, delivery)
            .await
            .unwrap(),
        "submitted"
    );
}

#[tokio::test]
async fn saturated_ingress_rejects_before_body_read_and_recovers() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let pipeline = Arc::new(
        Pipeline::build(
            config(&dir.path().join("admission.db"), "http://127.0.0.1:9"),
            TOKEN.into(),
            None,
        )
        .await
        .unwrap(),
    );
    let permits: Vec<_> = (0..64)
        .map(|_| pipeline.try_admit_webhook().unwrap())
        .collect();
    assert!(pipeline.try_admit_webhook().is_err());
    let router = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(pipeline.clone()),
        ..Default::default()
    });
    let request = |body: &str| {
        Request::post(format!("/api/webhooks/{}", pipeline.source.connection_id))
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    };
    let busy = router
        .clone()
        .oneshot(request("invalid JSON is never parsed while busy"))
        .await
        .unwrap();
    assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(busy.headers()["retry-after"], "1");
    drop(permits);
    let accepted = router
        .oneshot(request(
            r#"{"source_event_id":"one","event_type":"test","payload":{}}"#,
        ))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn real_http_ingress_restarts_then_worker_forwards_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipeline.db");
    let count = Arc::new(AtomicUsize::new(0));
    let (sent, mut received) = tokio::sync::mpsc::channel(4);
    let sink = Router::new().route(
        "/messages",
        post({
            let count = count.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let count = count.clone();
                let sent = sent.clone();
                async move {
                    assert_eq!(
                        headers["idempotency-key"],
                        body["delivery_id"].as_str().unwrap()
                    );
                    assert_eq!(headers["authorization"], "Bearer receiver-test-secret");
                    count.fetch_add(1, Ordering::SeqCst);
                    sent.send(body).await.unwrap();
                    StatusCode::ACCEPTED
                }
            }
        }),
    );
    let (sink_url, sink_stop, sink_task) = serve(sink).await;
    let destination = format!("{sink_url}/messages");
    let pipeline = Arc::new(
        Pipeline::build(
            config(&path, &destination),
            TOKEN.into(),
            Some("receiver-test-secret".into()),
        )
        .await
        .unwrap(),
    );
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        pipeline: Some(pipeline.clone()),
        ..Default::default()
    }))
    .await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/webhooks/{}", pipeline.source.connection_id);
    let body = json!({"source_event_id":"chat-42","event_type":"chat.message","payload":{"text":"message for relay"}});
    let unauthorized = client.post(&url).json(&body).send().await.unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert!(unauthorized.headers().contains_key("x-request-id"));
    let unknown=client.post(&url).bearer_auth(TOKEN).json(&json!({"source_event_id":"bad","event_type":"chat.message","payload":{},"workspace_id":Uuid::new_v4()})).send().await.unwrap();
    assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);
    let large=client.post(&url).bearer_auth(TOKEN).json(&json!({"source_event_id":"large","event_type":"chat.message","payload":"a".repeat(70000)})).send().await.unwrap();
    assert_eq!(large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let reply = client
        .post(&url)
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status(), StatusCode::ACCEPTED);
    let first: Value = reply.json().await.unwrap();
    assert_eq!(first["disposition"], "stored");
    // Stop before running the worker: acknowledged but unrouted events must survive.
    stop.cancel();
    task.await.unwrap();
    drop(pipeline);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let pipeline = Arc::new(
        Pipeline::build(
            config(&path, &destination),
            TOKEN.into(),
            Some("receiver-test-secret".into()),
        )
        .await
        .unwrap(),
    );
    let (base, stop, task) = serve(synquay_server::interfaces::http::router(AppState {
        pipeline: Some(pipeline.clone()),
        ..Default::default()
    }))
    .await;
    let repeated: Value = client
        .post(format!(
            "{base}/api/webhooks/{}",
            pipeline.source.connection_id
        ))
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(repeated["event_id"], first["event_id"]);
    assert_eq!(repeated["disposition"], "duplicate");
    let worker_stop = CancellationToken::new();
    let worker = tokio::spawn(pipeline.clone().run(worker_stop.clone()));
    let delivered = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivered["parts"][0]["value"]["payload"], body["payload"]);
    assert_eq!(delivered["parts"][0]["value"]["source_event_id"], "chat-42");
    worker_stop.cancel();
    worker.await.unwrap();
    stop.cancel();
    task.await.unwrap();
    drop(pipeline);
    let store = SqliteStore::open(&path).await.unwrap();
    let id = Uuid::parse_str(delivered["delivery_id"].as_str().unwrap()).unwrap();
    assert_eq!(
        store.delivery_state(Uuid::from_u128(1), id).await.unwrap(),
        "submitted"
    );
    assert!(
        store
            .unrouted(Uuid::from_u128(1), 100)
            .await
            .unwrap()
            .is_empty()
    );
    let pipeline = Pipeline::build(
        config(&path, &destination),
        TOKEN.into(),
        Some("receiver-test-secret".into()),
    )
    .await
    .unwrap();
    pipeline.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    sink_stop.cancel();
    sink_task.await.unwrap();
}

#[tokio::test]
async fn ambiguous_http_failure_stays_reconcile_without_automatic_repeat() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipeline.db");
    let count = Arc::new(AtomicUsize::new(0));
    let router = Router::new()
        .route(
            "/",
            post(|State(count): State<Arc<AtomicUsize>>| async move {
                count.fetch_add(1, Ordering::SeqCst);
                StatusCode::SERVICE_UNAVAILABLE
            }),
        )
        .with_state(count.clone());
    let (base, stop, task) = serve(router).await;
    let pipeline = Pipeline::build(config(&path, &base), TOKEN.into(), None)
        .await
        .unwrap();
    let receipt = pipeline
        .accept(
            Uuid::new_v4(),
            synquay_server::domain::event::IncomingEvent {
                route_context: None,
                event_type: "test".into(),
                schema_version: 1,
                source_event_id: Some("one".into()),
                occurred_at: None,
                payload: synquay_server::domain::event::EventPayload::Json(json!({})),
            },
        )
        .await
        .unwrap();
    pipeline.tick(&CancellationToken::new()).await.unwrap();
    pipeline.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let store = SqliteStore::open(&path).await.unwrap();
    let saved = store
        .enqueue(synquay_server::domain::delivery::DispatchPlan {
            workspace_id: pipeline.source.workspace_id,
            key: DispatchKey::Event {
                scope: pipeline.source,
                event_id: receipt.event_id,
            },
            deliveries: vec![],
        })
        .await
        .unwrap();
    assert!(saved.duplicate);
    assert_eq!(
        store
            .delivery_state(pipeline.source.workspace_id, saved.delivery_ids[0])
            .await
            .unwrap(),
        "reconcile"
    );
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn redirect_is_not_followed_and_targets_are_operator_restricted() {
    let count = Arc::new(AtomicUsize::new(0));
    let redirect_count = count.clone();
    let router = Router::new()
        .route(
            "/",
            post(|| async { (StatusCode::TEMPORARY_REDIRECT, [("location", "/other")]) }),
        )
        .route(
            "/other",
            post(move || {
                let count = redirect_count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        );
    let (base, stop, task) = serve(router).await;
    let scope = scope();
    let destination_id = Uuid::new_v4();
    assert!(HttpSink::new(scope, destination_id, &base, None, false).is_err());
    assert!(HttpSink::new(scope, destination_id, "http://192.168.0.1/hook", None, true).is_err());
    assert!(
        HttpSink::new(
            scope,
            destination_id,
            "https://user:pass@example.com/hook",
            None,
            false
        )
        .is_err()
    );
    let sink = HttpSink::new(scope, destination_id, &base, None, true).unwrap();
    assert!(HttpSink::new(scope, destination_id, &base, Some(""), true).is_err());
    let context = OperationContext {
        scope,
        principal_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        route_trace: vec![],
        deadline: None,
        cancellation: CancellationToken::new(),
    };
    let result = sink
        .deliver(
            context,
            DeliveryRequest {
                delivery_id: Uuid::new_v4(),
                destination_id,
                message: Message {
                    destination_binding: None,
                    parts: vec![ContentPart::Text("hello".into())],
                    reply_route_id: None,
                },
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(PortError::Failed {
            code: "http_rejected"
        })
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    stop.cancel();
    task.await.unwrap();
}
