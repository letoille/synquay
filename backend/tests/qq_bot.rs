mod support;
use axum::{Json, Router, body::Body, http::Request};
use ring::signature::Ed25519KeyPair;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use support::*;
use synquay_server::{
    AppState,
    domain::managed_route::*,
    infrastructure::{
        connectors::{message_config::*, qq::QqBotConfig},
        pipeline::{Pipeline, message_connections::ConnectionInput},
    },
};
use tower::ServiceExt;
use uuid::Uuid;
#[tokio::test]
async fn qq_callbacks_route_commands_deduplicate_and_persist() {
    scenario().await;
}
async fn scenario() {
    bot_level_discovery().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("qq.db");
    let (sent, mut recv) = tokio::sync::mpsc::channel::<Value>(8);
    let app = Router::new().route(
        "/agent",
        axum::routing::post(move |Json(v): Json<Value>| {
            let sent = sent.clone();
            async move {
                sent.send(v).await.unwrap();
                axum::http::StatusCode::OK
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/agent", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let p = Arc::new(
        Pipeline::build(config(&db, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let actor = Uuid::from_u128(10);
    let c = MessageConnection {
        id: Uuid::new_v4(),
        name: "QQ test".into(),
        description: String::new(),
        enabled: true,
        config: ConnectionConfig::QqBot(QqBotConfig {
            resolved_secret: None,
            app_id: "123456".into(),
            group_openid: "group_A".into(),
            allowed_senders: vec!["member_A".into()],
            allow_proactive: false,
        }),
    };
    p.change_connection(
        actor,
        ConnectionInput {
            secret: Some(TOKEN.into()),
            operation_id: Uuid::new_v4(),
            expected_revision: 0,
            connection_id: c.id,
            connection: Some(c.clone()),
        },
    )
    .await
    .unwrap();
    let view = p.routes_view().await.unwrap();
    assert!(
        view.sources
            .iter()
            .any(|s| s.connection_id == c.id && s.protocol == "QQ")
    );
    assert!(
        view.targets
            .iter()
            .any(|t| t.destination_id == c.send_id() && t.protocol == "QQ")
    );
    let dest = view
        .targets
        .iter()
        .find(|t| t.protocol == "HTTP")
        .unwrap()
        .destination_id;
    let change=serde_json::from_value(json!({"kind":"save","name":"QQ → Agent","enabled":true,"source_id":c.id,"event_type":"qq.group.message","processor":{"kind":"agent_http","name":"Test Agent"},"destination_ids":[dest],"command":{"text_pointer":"/content","trigger":"/助手","mode":"prefix","output":{"kind":"json","event_type":"assistant.request","template":{"request":{"$command":"arguments"},"msg_id":{"$payload":"/msg_id"},"group_openid":{"$payload":"/group_openid"}}}}})).unwrap();
    p.change_route(
        actor,
        RouteInput {
            operation_id: Uuid::new_v4(),
            expected_revision: view.snapshot.revision,
            route_id: Uuid::new_v4(),
            change,
        },
    )
    .await
    .unwrap();
    let router = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let key =
        Ed25519KeyPair::from_seed_unchecked(&TOKEN.bytes().cycle().take(32).collect::<Vec<_>>())
            .unwrap();
    let make = |body: Value, ts: &str, good: bool| {
        let bytes = serde_json::to_vec(&body).unwrap();
        let mut msg = ts.as_bytes().to_vec();
        msg.extend_from_slice(&bytes);
        let sig: String = key
            .sign(&msg)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Request::post(format!("/api/qq/{}/webhook", c.id))
            .header("content-type", "application/json")
            .header("x-bot-appid", "123456")
            .header("x-signature-timestamp", ts)
            .header(
                "x-signature-ed25519",
                if good { sig } else { "00".repeat(64) },
            )
            .body(Body::from(bytes))
            .unwrap()
    };
    let body = json!({"op":0,"t":"GROUP_AT_MESSAGE_CREATE","d":{"id":"message_A","group_openid":"group_A","author":{"member_openid":"member_A"},"content":" /助手 查询设备状态 ","timestamp":"2026-10-26T12:00:00+08:00"}});
    assert_eq!(
        router
            .clone()
            .oneshot(make(body.clone(), &now, false))
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        router
            .clone()
            .oneshot(make(body.clone(), "1", true))
            .await
            .unwrap()
            .status(),
        401
    );
    for value in [
        json!({"op":13,"d":{"plain_token":"sampleToken","event_ts":now}}),
        body.clone(),
        body.clone(),
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(make(value, &now, true))
                .await
                .unwrap()
                .status(),
            200
        );
    }
    let mut duplicate = body.clone();
    duplicate["t"] = json!("GROUP_MESSAGE_CREATE");
    assert_eq!(
        router
            .clone()
            .oneshot(make(duplicate, &now, true))
            .await
            .unwrap()
            .status(),
        200
    );
    let mut outside = body.clone();
    outside["d"]["group_openid"] = json!("group_B");
    assert_eq!(
        router
            .clone()
            .oneshot(make(outside, &now, true))
            .await
            .unwrap()
            .status(),
        200
    );
    let mut denied = body.clone();
    denied["d"]["id"] = json!("denied");
    denied["d"]["author"]["member_openid"] = json!("member_B");
    assert_eq!(
        router
            .clone()
            .oneshot(make(denied, &now, true))
            .await
            .unwrap()
            .status(),
        200
    );
    let mut edited = body.clone();
    edited["d"]["content"] = json!("different");
    assert_eq!(
        router
            .clone()
            .oneshot(make(edited, &now, true))
            .await
            .unwrap()
            .status(),
        409
    );
    drop(router);
    drop(p);
    let p = Arc::new(
        Pipeline::build(config(&db, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    assert_eq!(p.connection_snapshot().unwrap().connections.len(), 1);
    p.tick(&tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), recv.recv())
            .await
            .unwrap()
            .unwrap()["parts"][0]["value"]["payload"],
        json!({"request":"查询设备状态","msg_id":"message_A","group_openid":"group_A"})
    );
    assert!(recv.try_recv().is_err());
    let router = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    assert_eq!(
        router
            .clone()
            .oneshot(make(body, &now, true))
            .await
            .unwrap()
            .status(),
        200
    );
    p.change_connection(
        actor,
        ConnectionInput {
            secret: None,
            operation_id: Uuid::new_v4(),
            expected_revision: 1,
            connection_id: c.id,
            connection: Some(MessageConnection {
                enabled: false,
                ..c.clone()
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        router
            .oneshot(make(
                json!({"op":13,"d":{"plain_token":"sampleToken","event_ts":now}}),
                &now,
                true
            ))
            .await
            .unwrap()
            .status(),
        404
    );
    server.abort();
}

async fn bot_level_discovery() {
    use synquay_server::infrastructure::storage::conversations::ConversationQuery;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("directory.db");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Value>(8);
    let app = Router::new().route(
        "/agent",
        axum::routing::post(move |Json(v): Json<Value>| {
            let tx = tx.clone();
            async move {
                tx.send(v).await.unwrap();
                axum::http::StatusCode::OK
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/agent", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let p = Arc::new(
        Pipeline::build(config(&db, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let c = MessageConnection {
        id: Uuid::new_v4(),
        name: "Bot".into(),
        description: String::new(),
        enabled: true,
        config: ConnectionConfig::QqBot(QqBotConfig {
            resolved_secret: None,
            app_id: "123456".into(),
            group_openid: String::new(),
            allowed_senders: vec![],
            allow_proactive: false,
        }),
    };
    let actor = Uuid::from_u128(10);
    p.change_connection(
        actor,
        ConnectionInput {
            secret: Some(TOKEN.into()),
            operation_id: Uuid::new_v4(),
            expected_revision: 0,
            connection_id: c.id,
            connection: Some(c.clone()),
        },
    )
    .await
    .unwrap();
    let v = p.routes_view().await.unwrap();
    let dest = v
        .targets
        .iter()
        .find(|t| t.protocol == "HTTP")
        .unwrap()
        .destination_id;
    let route_id = Uuid::new_v4();
    let save = |revision, routing| {
        RouteInput{operation_id:Uuid::new_v4(),expected_revision:revision,route_id,change:serde_json::from_value(json!({"kind":"save","name":"Selected conversations","enabled":true,"source_id":c.id,"event_type":null,"destination_ids":[dest],"routing":routing})).unwrap()}
    };
    assert!(
        p.change_route(
            actor,
            save(v.snapshot.revision, json!({"source":null,"targets":{}}))
        )
        .await
        .is_err()
    );
    let selected = json!({"source":{"adapter":"qq","version":1,"config":{"mode":"groups","ids":["group_A"]}},"targets":{}});
    p.change_route(actor, save(v.snapshot.revision, selected))
        .await
        .unwrap();
    // Legacy edits omit routing: they must preserve the selected-group restriction.
    let revision = p.routes_view().await.unwrap().snapshot.revision;
    let old=RouteInput{operation_id:Uuid::new_v4(),expected_revision:revision,route_id,change:serde_json::from_value(json!({"kind":"save","name":"Renamed","enabled":true,"event_type":null,"destination_ids":[dest]})).unwrap()};
    p.change_route(actor, old).await.unwrap();
    let router = synquay_server::interfaces::http::router(AppState {
        pipeline: Some(p.clone()),
        ..Default::default()
    });
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let key =
        Ed25519KeyPair::from_seed_unchecked(&TOKEN.bytes().cycle().take(32).collect::<Vec<_>>())
            .unwrap();
    for (id, group, direct, good) in [
        ("a", "group_A", false, true),
        ("b", "group_B", false, true),
        ("c", "user_A", true, true),
        ("d", "forged_group", false, false),
    ] {
        let author = if direct {
            json!({"user_openid":group})
        } else {
            json!({"member_openid":"member_A"})
        };
        let body = json!({"op":0,"t":if direct{"C2C_MESSAGE_CREATE"}else{"GROUP_AT_MESSAGE_CREATE"},"d":{"id":id,"group_openid":group,"author":author,"content":id}});
        let bytes = serde_json::to_vec(&body).unwrap();
        let mut signed = now.as_bytes().to_vec();
        signed.extend_from_slice(&bytes);
        let sig = if good {
            key.sign(&signed)
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        } else {
            "00".repeat(64)
        };
        let r = Request::post(format!("/api/qq/{}/webhook", c.id))
            .header("content-type", "application/json")
            .header("x-bot-appid", "123456")
            .header("x-signature-timestamp", &now)
            .header("x-signature-ed25519", sig)
            .body(Body::from(bytes))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(r).await.unwrap().status(),
            if good { 200 } else { 401 }
        );
    }
    let page = p
        .connection_conversations(
            c.id,
            ConversationQuery {
                page_size: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 1);
    let other = p
        .connection_conversations(
            c.id,
            ConversationQuery {
                page: Some(2),
                page_size: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_ne!(page.items[0].id, other.items[0].id);
    let direct = p
        .connection_conversations(
            c.id,
            ConversationQuery {
                kind: Some("direct".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(direct.total, 1);
    assert_eq!(direct.items[0].id, "user_A");
    assert_eq!(
        p.connection_conversations(
            c.id,
            ConversationQuery {
                search: Some("group_B".into()),
                ..Default::default()
            }
        )
        .await
        .unwrap()
        .total,
        1
    );
    assert!(
        p.connection_conversations(Uuid::new_v4(), Default::default())
            .await
            .is_err()
    );
    drop(router);
    drop(p);
    let p = Arc::new(
        Pipeline::build(config(&db, &url), TOKEN.into(), None)
            .await
            .unwrap(),
    );
    assert_eq!(
        p.connection_conversations(c.id, Default::default())
            .await
            .unwrap()
            .total,
        2
    );
    p.tick(&tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["parts"][0]["value"]["payload"]["content"], "a");
    assert!(rx.try_recv().is_err());
    p.change_connection(
        actor,
        ConnectionInput {
            secret: None,
            operation_id: Uuid::new_v4(),
            expected_revision: 1,
            connection_id: c.id,
            connection: None,
        },
    )
    .await
    .unwrap();
    let sql = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        sql.query_row("SELECT count(*) FROM connection_conversations", [], |r| r
            .get::<_, u64>(
            0
        ))
        .unwrap(),
        0
    );
    server.abort();
}
