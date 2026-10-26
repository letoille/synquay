mod support;
use axum::{Json, Router, body::Body, http::Request};
use serde_json::{Value, json};
use std::sync::Arc;
use support::*;
use synquay_server::{
    AppState,
    application::ports::PortError,
    domain::managed_route::*,
    infrastructure::{
        connectors::message_config::*,
        pipeline::{Pipeline, message_connections::ConnectionInput},
    },
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use uuid::Uuid;

// Secrets are supplied before the subprocess starts, avoiding process-wide
// environment mutation while the async runtime or other tests are active.
#[test]
fn managed_connections_persist_route_and_remove_references_atomically() {
    if std::env::var_os("SYNQUAY_CONNECTION_TEST_CHILD").is_some() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(scenario());
        return;
    }
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "managed_connections_persist_route_and_remove_references_atomically",
            "--nocapture",
        ])
        .env("SYNQUAY_CONNECTION_TEST_CHILD", "1")
        .env("SYNQUAY_CONNECTION_TEST_SECRET", TOKEN)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
fn input(revision: u64, c: Option<MessageConnection>, id: Uuid) -> ConnectionInput {
    ConnectionInput {
        secret: None,
        operation_id: Uuid::new_v4(),
        expected_revision: revision,
        connection_id: id,
        connection: c,
    }
}
async fn scenario() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pipeline.db");
    let (sent, mut received) = tokio::sync::mpsc::channel::<Value>(8);
    let server = Router::new().route(
        "/messages",
        axum::routing::put(
            move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                let sent = sent.clone();
                async move {
                    assert!(headers.contains_key("idempotency-key"));
                    sent.send(body).await.unwrap();
                    axum::http::StatusCode::ACCEPTED
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/messages", listener.local_addr().unwrap());
    let http = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
    let p = Arc::new(
        Pipeline::build(config(&path, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let actor = Uuid::from_u128(10);
    let webhook = MessageConnection {
        id: Uuid::new_v4(),
        name: "Webhook".into(),
        description: "".into(),
        enabled: true,
        config: ConnectionConfig::Webhook {
            secret_env: "SYNQUAY_CONNECTION_TEST_SECRET".into(),
            policy: synquay_server::infrastructure::pipeline::webhook::WebhookConfig {
                authentication: Default::default(),
                format: synquay_server::infrastructure::pipeline::webhook::WebhookFormat::Json {
                    event_type: "sensor.message".into(),
                    id_header: "x-event-id".into(),
                },
            },
        },
    };
    let out = MessageConnection {
        id: Uuid::new_v4(),
        name: "HTTP endpoint".into(),
        description: "".into(),
        enabled: true,
        config: ConnectionConfig::HttpClient(HttpClientConfig {
            url: url.clone(),
            method: "PUT".into(),
            allow_http: true,
            auth: HttpAuth::None,
            body: BodyMode::Payload,
        }),
    };
    let first = input(0, Some(webhook.clone()), webhook.id);
    assert_eq!(p.change_connection(actor, first.clone()).await.unwrap(), 1);
    assert_eq!(
        p.change_connection(actor, input(1, Some(out.clone()), out.id))
            .await
            .unwrap(),
        2
    );
    assert_eq!(p.change_connection(actor, first).await.unwrap(), 1);
    assert_eq!(p.connection_snapshot().unwrap().revision, 2);
    let view = p.routes_view().await.unwrap();
    assert!(view.sources.iter().any(|s| s.connection_id == webhook.id));
    assert!(
        view.targets
            .iter()
            .any(|t| t.destination_id == out.send_id() && t.protocol == "HTTP")
    );
    let route_id = Uuid::new_v4();
    p.change_route(
        actor,
        RouteInput {
            operation_id: Uuid::new_v4(),
            expected_revision: view.snapshot.revision,
            route_id,
            change: RouteChange::Save {
                response: None,
                routing: None,
                name: "Webhook → HTTP".into(),
                enabled: true,
                source_id: Some(webhook.id),
                processor: None,
                command: None,
                event_type: Some("sensor.message".into()),
                destination_ids: vec![out.send_id()],
            },
        },
    )
    .await
    .unwrap();
    let app = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    let receive = |token: &str| {
        Request::post(format!("/api/webhooks/{}", webhook.id))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .header("x-event-id", "event-1")
            .body(Body::from(r#"{"temperature":24}"#))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(receive("wrong"))
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        app.clone().oneshot(receive(TOKEN)).await.unwrap().status(),
        202
    );
    assert_eq!(
        app.clone().oneshot(receive(TOKEN)).await.unwrap().status(),
        202
    );
    assert!(matches!(
        p.change_connection(actor, input(2, None, webhook.id)).await,
        Err(PortError::InvalidInput {
            code: "connection_has_pending_work"
        })
    ));
    p.tick(&CancellationToken::new()).await.unwrap();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
            .await
            .expect("HTTP message not delivered")
            .unwrap(),
        json!({"temperature":24})
    );
    assert!(received.try_recv().is_err());
    let paused = MessageConnection {
        enabled: false,
        ..webhook.clone()
    };
    p.change_connection(actor, input(2, Some(paused), webhook.id))
        .await
        .unwrap();
    assert_eq!(
        app.clone().oneshot(receive(TOKEN)).await.unwrap().status(),
        404
    );
    drop(app);
    drop(p);
    let p = Arc::new(
        Pipeline::build(config(&path, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    assert_eq!(p.connection_snapshot().unwrap().connections.len(), 2);
    assert!(!p.connection_snapshot().unwrap().connections[0].enabled);
    let deletion = input(3, None, out.id);
    let failed_commit = rusqlite::Connection::open(&path).unwrap();
    failed_commit.execute_batch("CREATE TRIGGER reject_connection_commit BEFORE INSERT ON message_connection_operations BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(p.change_connection(actor, deletion.clone()).await.is_err());
    assert_eq!(p.connection_snapshot().unwrap().connections.len(), 2);
    assert!(
        p.routes_view()
            .await
            .unwrap()
            .snapshot
            .routes
            .iter()
            .any(|r| r.id == route_id)
    );
    failed_commit
        .execute_batch("DROP TRIGGER reject_connection_commit")
        .unwrap();

    p.change_connection(actor, deletion.clone()).await.unwrap();
    let v = p.routes_view().await.unwrap();
    assert!(!v.snapshot.routes.iter().any(|r| r.id == route_id));
    assert!(!v.targets.iter().any(|t| t.destination_id == out.send_id()));
    assert_eq!(p.change_connection(actor, deletion).await.unwrap(), 4);
    // CAS and receipt actor binding prevent a stale editor or a different operator replay.
    assert!(matches!(
        p.change_connection(actor, input(3, Some(out.clone()), out.id))
            .await,
        Err(PortError::Conflict)
    ));
    let db = rusqlite::Connection::open(&path).unwrap();
    let count: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM message_connection_operations",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 4);
    for query in [
        "SELECT 1 FROM routing_tasks WHERE workspace='w' AND source='s' AND state<>'completed'",
        "SELECT 1 FROM deliveries WHERE workspace='w' AND target_id='t' AND state<>'submitted' AND resolution IS NULL",
    ] {
        let plan: String = db
            .query_row(&format!("EXPLAIN QUERY PLAN {query}"), [], |r| r.get(3))
            .unwrap();
        assert!(plan.contains("INDEX"), "{plan}");
    }
    http.abort();
}

async fn mqtt_packet<S: tokio::io::AsyncRead + Unpin>(s: &mut S) -> (u8, Vec<u8>) {
    use tokio::io::AsyncReadExt;
    let head = s.read_u8().await.unwrap();
    let mut len = 0;
    let mut shift = 0;
    loop {
        let b = s.read_u8().await.unwrap();
        len += ((b & 127) as usize) << shift;
        if b < 128 {
            break;
        }
        shift += 7;
        assert!(shift < 28);
    }
    assert!(len < 70000);
    let mut body = vec![0; len];
    s.read_exact(&mut body).await.unwrap();
    (head, body)
}
#[tokio::test]
async fn managed_mqtt_subscription_and_publish_share_configuration_and_route_both_ways() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = tempfile::tempdir().unwrap();
    let actor = Uuid::from_u128(10);
    let (http_send, mut http_receive) = tokio::sync::mpsc::channel(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = Router::new().route(
        "/",
        axum::routing::post(move |Json(body): Json<Value>| {
            let tx = http_send.clone();
            async move {
                tx.send(body).await.unwrap();
                axum::http::StatusCode::ACCEPTED
            }
        }),
    );
    let http = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (mqtt_send, mut mqtt_receive) = tokio::sync::mpsc::channel(4);
    let broker_stop = CancellationToken::new();
    let bs = broker_stop.clone();
    let broker = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            let (mut socket, _) =
                tokio::select! {_=bs.cancelled()=>break,r=listener.accept()=>r.unwrap()};
            let tx = mqtt_send.clone();
            let stop = bs.clone();
            clients.spawn(async move {
                assert_eq!(mqtt_packet(&mut socket).await.0, 0x10);
                socket.write_all(&[0x20, 2, 0, 0]).await.unwrap();
                let (head, body) = mqtt_packet(&mut socket).await;
                match head >> 4 {
                    8 => {
                        socket
                            .write_all(&[0x90, 3, body[0], body[1], 1])
                            .await
                            .unwrap();
                        let topic = b"home/heater/state";
                        let payload = br#"{"power":"on"}"#;
                        let mut packet = vec![
                            0x32,
                            (topic.len() + 4 + payload.len()) as u8,
                            0,
                            topic.len() as u8,
                        ];
                        packet.extend_from_slice(topic);
                        packet.extend_from_slice(&[0, 7]);
                        packet.extend_from_slice(payload);
                        socket.write_all(&packet).await.unwrap();
                        assert_eq!(mqtt_packet(&mut socket).await, (0x40, vec![0, 7]));
                        tx.send("ingested").await.unwrap();
                        stop.cancelled().await;
                    }
                    3 => {
                        let len = u16::from_be_bytes([body[0], body[1]]) as usize;
                        assert_eq!(&body[2..2 + len], b"home/heater/set");
                        assert_eq!(
                            serde_json::from_slice::<Value>(&body[len + 4..]).unwrap(),
                            json!({"turn_on":true})
                        );
                        socket
                            .write_all(&[0x40, 2, body[len + 2], body[len + 3]])
                            .await
                            .unwrap();
                        tx.send("published").await.unwrap();
                        let mut one = [0];
                        let _ = socket.read(&mut one).await;
                    }
                    _ => panic!("unexpected MQTT packet"),
                }
            });
        }
        while let Some(r) = clients.join_next().await {
            r.unwrap();
        }
    });
    let p = Arc::new(
        Pipeline::build(config(&dir.path().join("db"), &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let mqtt = MessageConnection {
        id: Uuid::new_v4(),
        name: "Heater".into(),
        description: "".into(),
        enabled: true,
        config: ConnectionConfig::Mqtt(MqttConfig {
            host: "127.0.0.1".into(),
            port,
            username: None,
            password_env: None,
            tls: None,
            subscription: Some(MqttSubscription {
                include_metadata: true,
                topic: "home/+/state".into(),
                qos: 1,
                event_type: "state".into(),
            }),
            publish: Some(MqttPublish {
                topic: "home/heater/set".into(),
                qos: 1,
                retain: false,
                body: BodyMode::Payload,
            }),
        }),
    };
    let target = MessageConnection {
        id: Uuid::new_v4(),
        name: "HTTP".into(),
        description: "".into(),
        enabled: true,
        config: ConnectionConfig::HttpClient(HttpClientConfig {
            url,
            method: "POST".into(),
            allow_http: true,
            auth: HttpAuth::None,
            body: BodyMode::Payload,
        }),
    };
    p.change_connection(actor, input(0, Some(mqtt.clone()), mqtt.id))
        .await
        .unwrap();
    p.change_connection(actor, input(1, Some(target.clone()), target.id))
        .await
        .unwrap();
    p.change_route(
        actor,
        RouteInput {
            operation_id: Uuid::new_v4(),
            expected_revision: 1,
            route_id: Uuid::from_u128(6),
            change: RouteChange::Delete,
        },
    )
    .await
    .unwrap();
    for (revision, source, destination, event_type) in [
        (2, p.source.connection_id, mqtt.send_id(), "command"),
        (3, mqtt.id, target.send_id(), "state"),
    ] {
        p.change_route(
            actor,
            RouteInput {
                operation_id: Uuid::new_v4(),
                expected_revision: revision,
                route_id: Uuid::new_v4(),
                change: RouteChange::Save {
                    response: None,
                    routing: None,
                    name: event_type.into(),
                    enabled: true,
                    source_id: Some(source),
                    processor: None,
                    command: None,
                    event_type: Some(event_type.into()),
                    destination_ids: vec![destination],
                },
            },
        )
        .await
        .unwrap();
    }
    let app = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    let command=Request::post(format!("/api/webhooks/{}",p.source.connection_id)).header("authorization",format!("Bearer {TOKEN}")).header("content-type","application/json").body(Body::from(json!({"source_event_id":"command-1","event_type":"command","payload":{"turn_on":true}}).to_string())).unwrap();
    assert_eq!(app.oneshot(command).await.unwrap().status(), 202);
    let stop = CancellationToken::new();
    let run = tokio::spawn(p.clone().run(stop.clone()));
    let check = async {
        let mut states = vec![
            mqtt_receive.recv().await.unwrap(),
            mqtt_receive.recv().await.unwrap(),
        ];
        states.sort();
        assert_eq!(states, vec!["ingested", "published"]);
        assert_eq!(
            http_receive.recv().await.unwrap(),
            json!({"topic":"home/heater/state","qos":1,"retain":false,"payload":{"power":"on"}})
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(8), check)
        .await
        .unwrap();
    stop.cancel();
    run.await.unwrap();
    broker_stop.cancel();
    broker.await.unwrap();
    http.abort();
}

#[tokio::test]
async fn message_connection_api_enforces_workspace_and_origin() {
    use synquay_server::infrastructure::tool_runtime::{
        ToolRuntime, config::ToolsConfig, security::SecurityConfig,
    };
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
    let p = Arc::new(
        Pipeline::build(
            config(&dir.path().join("pipeline"), "http://127.0.0.1:1/"),
            TOKEN.into(),
            None,
        )
        .await
        .unwrap(),
    );
    let app = synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime.clone()),
        pipeline: Some(p),
        ..Default::default()
    });
    assert_eq!(
        app.clone()
            .oneshot(
                Request::get("/api/message-connections")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        app.clone()
            .oneshot(
                Request::post("/api/message-connections")
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

    let id = Uuid::new_v4();
    let connection = MessageConnection {
        id,
        name: "API HTTP".into(),
        description: String::new(),
        enabled: false,
        config: ConnectionConfig::HttpClient(HttpClientConfig {
            url: "https://example.com/messages".into(),
            method: "POST".into(),
            allow_http: false,
            auth: HttpAuth::None,
            body: BodyMode::Payload,
        }),
    };
    let request = input(0, Some(connection), id);
    for _ in 0..2 {
        assert_eq!(
            app.clone()
                .oneshot(
                    Request::post("/api/message-connections")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&request).unwrap()))
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            200
        );
    }
    let mut other = config(&dir.path().join("other"), "http://127.0.0.1:1/");
    other.workspace_id = Uuid::new_v4();
    let p = Arc::new(Pipeline::build(other, TOKEN.into(), None).await.unwrap());
    let app = synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime),
        pipeline: Some(p),
        ..Default::default()
    });
    assert_eq!(
        app.oneshot(
            Request::get("/api/message-connections")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        403
    );
}
