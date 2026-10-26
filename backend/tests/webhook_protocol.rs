mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use support::*;
use synquay_server::{
    AppState,
    application::ports::{PortError, events::EventStore},
    infrastructure::{
        pipeline::{
            Pipeline,
            webhook::{WebhookAuth, WebhookFormat},
        },
        storage::SqliteStore,
    },
};
use tower::ServiceExt;

async fn setup(path: &std::path::Path, signed: bool, raw: bool) -> Router {
    let mut cfg = config(path, "http://127.0.0.1:1/");
    if signed {
        cfg.webhook.authentication = WebhookAuth::HmacSha256;
    }
    if raw {
        cfg.webhook.format = WebhookFormat::Json {
            event_type: "chat.message".into(),
            id_header: "x-delivery-id".into(),
        };
    }
    let pipeline = Arc::new(Pipeline::build(cfg, TOKEN.into(), None).await.unwrap());
    synquay_server::interfaces::http::router(AppState {
        pipeline: Some(pipeline),
        ..Default::default()
    })
}
fn signature(timestamp: &str, id: &str, body: &str) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, TOKEN.as_bytes());
    let text = format!("v1\n00000000-0000-0000-0000-000000000002\n{timestamp}\n{id}\n{body}");
    format!(
        "v1={}",
        ring::hmac::sign(&key, text.as_bytes())
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
fn request() -> axum::http::request::Builder {
    Request::post("/api/webhooks/00000000-0000-0000-0000-000000000002")
        .header("content-type", "application/json")
}
#[tokio::test]
async fn signed_raw_json_binds_body_timestamp_and_delivery_id_and_rejects_fallback() {
    synquay_server::infrastructure::pipeline::config::PipelineConfig::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/pipeline-signed-json.json"),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let app = setup(&path, true, true).await;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let body = r#"{"text":"hello","workspace":"untrusted"}"#;
    let signed = |id: &str, wire: &str, time: &str, sig: &str| {
        request()
            .header("x-delivery-id", id)
            .header("x-synquay-timestamp", time)
            .header("x-synquay-signature", sig)
            .body(Body::from(wire.to_owned()))
            .unwrap()
    };
    let sig = signature(&timestamp, "delivery-1", body);
    let response = app
        .clone()
        .oneshot(signed("delivery-1", body, &timestamp, &sig))
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let first: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let response = app
        .clone()
        .oneshot(signed("delivery-1", body, &timestamp, &sig))
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let duplicate: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(first["event_id"], duplicate["event_id"]);
    assert_eq!(duplicate["disposition"], "duplicate");
    for bad in [
        signed("delivery-2", body, &timestamp, &sig),
        signed("delivery-1", "{}", &timestamp, &sig),
        signed("delivery-1", body, "1", &signature("1", "delivery-1", body)),
    ] {
        assert_eq!(app.clone().oneshot(bad).await.unwrap().status(), 401);
    }
    assert_eq!(
        app.clone()
            .oneshot(
                request()
                    .header("x-delivery-id", "fallback")
                    .header("authorization", format!("Bearer {TOKEN}"))
                    .body(Body::from(body))
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        401
    );
    let changed = r#"{"text":"changed"}"#;
    assert_eq!(
        app.clone()
            .oneshot(signed(
                "delivery-1",
                changed,
                &timestamp,
                &signature(&timestamp, "delivery-1", changed)
            ))
            .await
            .unwrap()
            .status(),
        409
    );
    let reopened = setup(&path, true, true).await;
    assert_eq!(
        reopened
            .oneshot(signed("delivery-1", body, &timestamp, &sig))
            .await
            .unwrap()
            .status(),
        202
    );
}
#[tokio::test]
async fn bearer_envelope_rejects_ambiguous_headers_and_semantic_id_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let app = setup(&dir.path().join("db"), false, false).await;
    let body =
        json!({"source_event_id":"id","event_type":"test","payload":{"value":7}}).to_string();
    let req = |body: String| {
        request()
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::from(body))
            .unwrap()
    };
    let mut duplicate = req(body.clone());
    duplicate
        .headers_mut()
        .append("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    assert_eq!(app.clone().oneshot(duplicate).await.unwrap().status(), 401);
    for header in ["content-type", "content-encoding"] {
        let mut ambiguous = req(body.clone());
        ambiguous
            .headers_mut()
            .append(header, "application/json".parse().unwrap());
        assert_eq!(app.clone().oneshot(ambiguous).await.unwrap().status(), 415);
    }

    assert_eq!(
        app.clone()
            .oneshot(req(body.clone()))
            .await
            .unwrap()
            .status(),
        202
    );
    let reordered =
        r#"{ "payload": { "value":7 }, "event_type":"test", "source_event_id":"id" }"#.to_owned();
    assert_eq!(
        app.clone().oneshot(req(reordered)).await.unwrap().status(),
        202
    );
    assert_eq!(
        app.clone()
            .oneshot(req(body.replace("7", "8")))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(app.oneshot(req("{".into())).await.unwrap().status(), 400);
}
#[tokio::test]
async fn migration_failure_rolls_back_fingerprint_and_legacy_duplicates_gain_durable_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let db = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        include_str!("../src/infrastructure/storage/schema.sql"),
        include_str!("../src/infrastructure/storage/migration_v2.sql"),
        include_str!("../src/infrastructure/storage/migration_v3.sql"),
        include_str!("../src/infrastructure/storage/migration_v4.sql"),
        include_str!("../src/infrastructure/storage/migration_v5.sql"),
        include_str!("../src/infrastructure/storage/migration_v6.sql"),
        include_str!("../src/infrastructure/storage/migration_v7.sql"),
        include_str!("../src/infrastructure/storage/migration_v8.sql"),
        include_str!("../src/infrastructure/storage/migration_v9.sql"),
    ] {
        db.execute_batch(sql).unwrap();
    }
    db.execute_batch("CREATE TABLE mcp_catalog_releases(collision INTEGER)")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        9
    );
    assert!(db.prepare("SELECT fingerprint FROM events").is_err());
    db.execute_batch("DROP TABLE mcp_catalog_releases").unwrap();
    let store = SqliteStore::open(&path).await.unwrap();
    let event = event(scope());
    store.append(event.clone()).await.unwrap();
    db.execute("UPDATE events SET fingerprint=NULL", [])
        .unwrap();
    assert!(store.append(event.clone()).await.is_ok());
    let hash: String = db
        .query_row("SELECT fingerprint FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(hash.len(), 64);
    db.execute("UPDATE events SET record=''", []).unwrap();
    assert!(store.append(event.clone()).await.is_ok());
    let mut oversized = event.clone();
    oversized.payload =
        synquay_server::domain::event::EventPayload::Json(json!({"large":"x".repeat(300_000)}));
    assert!(matches!(
        store.append(oversized).await,
        Err(PortError::PayloadTooLarge)
    ));
    let mut changed = event;
    changed.event_type = "other".into();
    assert!(matches!(
        store.append(changed).await,
        Err(PortError::Conflict)
    ));
    let query:String=db.query_row("EXPLAIN QUERY PLAN SELECT event_id,fingerprint,record FROM events WHERE workspace='w' AND source='s' AND source_event_id='i'",[],|r|r.get(3)).unwrap();
    assert!(query.contains("INDEX"));
}
