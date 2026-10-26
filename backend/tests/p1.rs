mod support;
use std::{sync::Arc, time::Duration};
use support::*;
use synquay_server::{
    application::ports::{
        OperationContext, PortError,
        checkpoints::{Checkpoint, SourceCheckpoints},
        content::ContentStore,
        delivery::DeliveryOutbox,
        events::EventStore,
        routing::EventReader,
    },
    domain::{
        delivery::{DispatchKey, DispatchPlan},
        event::EventPayload,
    },
    infrastructure::{
        content::{ContentConfig, FileContentStore},
        storage::{SqliteStore, maintenance::RetentionPolicy},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
fn ctx(
    scope: synquay_server::domain::connector::ConnectionScope,
    principal: Uuid,
) -> OperationContext {
    OperationContext {
        scope,
        principal_id: principal,
        request_id: Uuid::new_v4(),
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        route_trace: vec![],
        deadline: None,
        cancellation: CancellationToken::new(),
    }
}
fn old(path: &std::path::Path, days: i64) {
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute(
        "UPDATE retired_work SET retired_ms=CAST(unixepoch('subsec')*1000 AS INTEGER)-?1",
        [days * 86400000],
    )
    .unwrap();
}
#[tokio::test]
async fn checkpoint_is_atomic_fenced_and_retry_is_bound_to_committed_event() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("db")).await.unwrap();
    let s = scope();
    let first = store.activate(s, "topic:0".into(), None).await.unwrap();
    let cp = Checkpoint {
        cursor: Some("offset:7".into()),
        ..first
    };
    let e = event(s);
    store
        .append_checkpoint(e.clone(), cp.clone())
        .await
        .unwrap();
    let mut duplicate = e.clone();
    duplicate.event_id = Uuid::new_v4();
    assert_eq!(
        store
            .append_checkpoint(duplicate, cp.clone())
            .await
            .unwrap()
            .event_id,
        e.event_id
    );
    let loaded = store
        .checkpoint(s, cp.partition.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.revision, 1);
    let mut other = e.clone();
    other.event_id = Uuid::new_v4();
    other.source_event_id = Some("other".into());
    assert!(matches!(
        store.append_checkpoint(other.clone(), cp.clone()).await,
        Err(PortError::Conflict)
    ));
    assert!(matches!(
        store.load(s, other.event_id).await,
        Err(PortError::NotFound)
    ));
    let next = store
        .activate(s, cp.partition.clone(), Some(1))
        .await
        .unwrap();
    assert_eq!(next.generation, 2);
    assert_eq!(next.cursor, cp.cursor);
    assert!(matches!(
        store.append_checkpoint(e, cp).await,
        Err(PortError::Conflict)
    ));
}
#[tokio::test]
async fn capacity_rejection_does_not_advance_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("db")).await.unwrap();
    let s = scope();
    store
        .configure_capacity(
            synquay_server::infrastructure::storage::capacity::CapacityLimits {
                max_pending_items: 1,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store.append(event(s)).await.unwrap();
    let cp = store.activate(s, "p".into(), None).await.unwrap();
    let mut e = event(s);
    e.source_event_id = Some("second".into());
    assert!(matches!(
        store
            .append_checkpoint(
                e,
                Checkpoint {
                    cursor: Some("next".into()),
                    ..cp.clone()
                }
            )
            .await,
        Err(PortError::CapacityExceeded { .. })
    ));
    assert_eq!(store.checkpoint(s, "p".into()).await.unwrap().unwrap(), cp);
}
#[tokio::test]
async fn streaming_content_checks_digest_scope_size_and_releases_cancelled_quota() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let db = Arc::new(SqliteStore::open(&path).await.unwrap());
    let s = scope();
    let principal = Uuid::new_v4();
    let cfg = ContentConfig {
        directory: dir.path().join("objects"),
        max_object_bytes: 8 * 1024 * 1024,
        max_total_bytes: 8 * 1024 * 1024,
        max_objects: 1,
        ..Default::default()
    };
    let content = Arc::new(
        FileContentStore::open(db.clone(), cfg, s, principal, vec![])
            .await
            .unwrap(),
    );
    let (mut writer, reader) = tokio::io::duplex(4096);
    let c = content.clone();
    let operation = ctx(s, principal);
    let cancel = operation.cancellation.clone();
    let task = tokio::spawn(async move {
        c.upload(
            operation,
            8 * 1024 * 1024,
            "application/octet-stream".into(),
            Box::pin(reader),
        )
        .await
    });
    writer.write_all(&[3; 4096]).await.unwrap();
    cancel.cancel();
    assert!(matches!(task.await.unwrap(), Err(PortError::Cancelled)));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let c = rusqlite::Connection::open(&path).unwrap();
            let count: i64 = c
                .query_row("SELECT items FROM content_usage", [], |r| r.get(0))
                .unwrap();
            if count == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let r = content
        .upload(
            ctx(s, principal),
            3,
            "text/plain".into(),
            Box::pin(std::io::Cursor::new(b"abc")),
        )
        .await
        .unwrap();
    assert_eq!(
        r.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let mut out = Vec::new();
    content
        .read(ctx(s, principal), r.clone())
        .await
        .unwrap()
        .read_to_end(&mut out)
        .await
        .unwrap();
    assert_eq!(out, b"abc");
    assert!(matches!(
        content.read(ctx(s, Uuid::new_v4()), r.clone()).await,
        Err(PortError::PermissionDenied)
    ));
    let mut forged = r.clone();
    forged.size += 1;
    assert!(matches!(
        content.read(ctx(s, principal), forged).await,
        Err(PortError::NotFound)
    ));
    let mut e = event(s);
    e.payload = EventPayload::Content(r.clone());
    db.append(e.clone()).await.unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute("UPDATE content_objects SET created_ms=0", [])
        .unwrap();
    assert_eq!(
        content.collect().await.unwrap(),
        0,
        "pending event owns blob"
    );
    assert!(std::fs::metadata(dir.path().join("objects").join(format!("{}.blob", r.id))).is_ok());
    std::fs::write(
        dir.path().join("objects").join(format!("{}.blob", r.id)),
        b"bad",
    )
    .unwrap();
    let mut corrupt = Vec::new();
    assert!(
        content
            .read(ctx(s, principal), r)
            .await
            .unwrap()
            .read_to_end(&mut corrupt)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn retention_preserves_pending_work_and_dedupe_until_its_own_window() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let db = SqliteStore::open(&path).await.unwrap();
    let s = scope();
    let e = event(s);
    db.append(e.clone()).await.unwrap();
    let mut pending = event(s);
    pending.source_event_id = Some("pending".into());
    db.append(pending.clone()).await.unwrap();
    db.enqueue(DispatchPlan {
        workspace_id: s.workspace_id,
        key: DispatchKey::Event {
            scope: s,
            event_id: e.event_id,
        },
        deliveries: vec![],
    })
    .await
    .unwrap();
    old(&path, 10);
    let report = db
        .maintain(s.workspace_id, RetentionPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.payload_rows, 1);
    assert!(matches!(
        db.load(s, e.event_id).await,
        Err(PortError::NotFound)
    ));
    assert!(db.load(s, pending.event_id).await.is_ok());
    assert_eq!(db.append(e.clone()).await.unwrap().event_id, e.event_id);
    assert_eq!(
        db.backlog_status(s.workspace_id)
            .await
            .unwrap()
            .pending_items,
        1
    );
    old(&path, 100);
    db.maintain(s.workspace_id, RetentionPolicy::default())
        .await
        .unwrap();
    let mut again = e;
    again.event_id = Uuid::new_v4();
    assert_eq!(
        db.append(again).await.unwrap().disposition,
        synquay_server::domain::event::IngestDisposition::Stored
    );
    let c = rusqlite::Connection::open(&path).unwrap();
    assert!(
        c.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn versions_pin_event_to_delivery_and_reject_unknown_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let db = SqliteStore::open(&path).await.unwrap();
    let s = scope();
    db.bind_pipeline(serde_json::json!({"workspace":s.workspace_id,"principal":Uuid::new_v4()}))
        .await
        .unwrap();
    let first = db
        .activate_release(serde_json::json!({"capabilities":["json"]}))
        .await
        .unwrap();
    let e = event(s);
    db.append(e.clone()).await.unwrap();
    let next = db
        .activate_release(serde_json::json!({"capabilities":["json","binary"]}))
        .await
        .unwrap();
    assert_ne!(first, next);
    let j = job(s);
    let id = j.delivery_id;
    db.enqueue(DispatchPlan {
        workspace_id: s.workspace_id,
        key: DispatchKey::Event {
            scope: s,
            event_id: e.event_id,
        },
        deliveries: vec![j],
    })
    .await
    .unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT config_revision FROM deliveries WHERE delivery_id=?1",
            [id.to_string()],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        first
    );
    assert_eq!(
        db.activate_release(serde_json::json!({"capabilities":["json"]}))
            .await
            .unwrap(),
        first
    );
    c.execute(
        "UPDATE events SET record=json_set(record,'$.storage_version',999)",
        [],
    )
    .unwrap();
    assert!(matches!(
        db.load(s, e.event_id).await,
        Err(PortError::Unsupported)
    ));
}
#[tokio::test]
async fn http_content_upload_download_and_raw_binary_delivery_round_trip() {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::Request,
        routing::post,
    };
    use tower::ServiceExt;
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let receiver = Router::new().route(
        "/sink",
        post(move |request: Request<Body>| {
            let tx = tx.clone();
            async move {
                let digest = request.headers()["X-Synquay-Content-Sha256"]
                    .to_str()
                    .unwrap()
                    .to_owned();
                let bytes = to_bytes(request.into_body(), 4 * 1024 * 1024)
                    .await
                    .unwrap();
                tx.send((digest, bytes.len())).await.unwrap();
                axum::http::StatusCode::NO_CONTENT
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/sink", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, receiver).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut config = config(&path, &url);
    config.content = Some(ContentConfig {
        directory: dir.path().join("objects"),
        ..Default::default()
    });
    let source = config.source_connection_id;
    let pipeline = Arc::new(
        synquay_server::infrastructure::pipeline::Pipeline::build(config, TOKEN.into(), None)
            .await
            .unwrap(),
    );
    let app = synquay_server::interfaces::http::router(synquay_server::AppState {
        pipeline: Some(pipeline.clone()),
        ..Default::default()
    });
    let payload = vec![42u8; 2 * 1024 * 1024];
    let upload = Request::post(format!("/api/content/upload/{source}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-length", payload.len())
        .body(Body::from(payload.clone()))
        .unwrap();
    let r = app.clone().oneshot(upload).await.unwrap();
    assert_eq!(r.status(), 201);
    let reference: synquay_server::domain::content::ContentRef =
        serde_json::from_slice(&to_bytes(r.into_body(), 8192).await.unwrap()).unwrap();
    let download = Request::get(format!("/api/content/{}", reference.id))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(download).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        to_bytes(response.into_body(), payload.len())
            .await
            .unwrap()
            .as_ref(),
        payload
    );
    let request=Request::post(format!("/api/webhooks/{source}")).header("authorization",format!("Bearer {TOKEN}")).header("content-type","application/json").body(Body::from(serde_json::to_vec(&serde_json::json!({"source_event_id":"blob-1","event_type":"binary.received","payload":null,"content":reference})).unwrap())).unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), 202);
    pipeline.tick(&CancellationToken::new()).await.unwrap();
    let (hash, size) = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(size, payload.len());
    assert_eq!(hash, reference.sha256);
    let c = rusqlite::Connection::open(path).unwrap();
    let stored: i64 = c
        .query_row("SELECT length(record) FROM deliveries", [], |r| r.get(0))
        .unwrap();
    assert!(stored < 4096, "outbox must store reference only");
    server.abort();
}
#[tokio::test]
async fn hot_queries_have_targeted_indexes_and_do_not_sort_full_backlog() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let _db = SqliteStore::open(&path).await.unwrap();
    let c = rusqlite::Connection::open(path).unwrap();
    for (query, index) in [
        (
            "SELECT delivery_id FROM deliveries WHERE workspace='w' AND target_id='t' AND state='pending' AND resolution IS NULL AND due_ms<=100 ORDER BY due_ms,rowid LIMIT 1",
            "delivery_target_pending",
        ),
        (
            "SELECT rowid FROM audit WHERE workspace='w' AND occurred_ms<=100 ORDER BY occurred_ms LIMIT 100",
            "audit_retention",
        ),
        (
            "SELECT rowid FROM retired_work WHERE workspace='w' AND payload_purged=0 AND retired_ms<=100 LIMIT 100",
            "retired_payload_due",
        ),
        (
            "SELECT content_id FROM event_content WHERE content_id='id'",
            "event_content_reverse",
        ),
        (
            "SELECT generation FROM source_checkpoints WHERE workspace='w' AND source='s' AND partition='p'",
            "sqlite_autoindex_source_checkpoints",
        ),
    ] {
        let plan = c
            .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains(index), "{query}: {plan}");
        assert!(!plan.contains("USE TEMP B-TREE"), "{plan}");
    }
}

#[tokio::test]
async fn backup_restore_verifies_blob_checksums_and_retains_pending_references() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let s = scope();
    let principal = Uuid::new_v4();
    let db = Arc::new(SqliteStore::open(&path).await.unwrap());
    let content = FileContentStore::open(
        db.clone(),
        ContentConfig {
            directory: dir.path().join("objects"),
            ..Default::default()
        },
        s,
        principal,
        vec![],
    )
    .await
    .unwrap();
    let r = content
        .upload(
            ctx(s, principal),
            7,
            "text/plain".into(),
            Box::pin(std::io::Cursor::new(b"restore")),
        )
        .await
        .unwrap();
    let mut e = event(s);
    e.payload = EventPayload::Content(r.clone());
    db.append(e.clone()).await.unwrap();
    drop(content);
    drop(db);
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/backup.py");
    let bundle = dir.path().join("backup");
    let restored = dir.path().join("restored");
    for (operation, source, destination) in
        [("backup", &path, &bundle), ("restore", &bundle, &restored)]
    {
        let result = std::process::Command::new("python3")
            .arg(&script)
            .arg(operation)
            .arg(source)
            .arg(destination)
            .arg("--offline")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let compact = std::process::Command::new("python3")
        .arg(&script)
        .arg("compact")
        .arg(restored.join("database.sqlite3"))
        .arg("--offline")
        .output()
        .unwrap();
    assert!(
        compact.status.success(),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
    let db = Arc::new(
        SqliteStore::open(restored.join("database.sqlite3"))
            .await
            .unwrap(),
    );
    assert_eq!(
        db.backlog_status(s.workspace_id)
            .await
            .unwrap()
            .pending_items,
        1
    );
    assert_eq!(db.append(e.clone()).await.unwrap().event_id, e.event_id);
    let content = FileContentStore::open(
        db,
        ContentConfig {
            directory: restored.join("content"),
            ..Default::default()
        },
        s,
        principal,
        vec![],
    )
    .await
    .unwrap();
    let mut output = Vec::new();
    content
        .read(ctx(s, principal), r.clone())
        .await
        .unwrap()
        .read_to_end(&mut output)
        .await
        .unwrap();
    assert_eq!(output, b"restore");
    std::fs::write(
        bundle.join("content").join(format!("{}.blob", r.id)),
        b"corrupt",
    )
    .unwrap();
    assert!(
        !std::process::Command::new("python3")
            .arg(script)
            .arg("verify")
            .arg(bundle)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[tokio::test]
async fn retention_unlinks_blob_only_after_terminal_dispatch_payload_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let s = scope();
    let principal = Uuid::new_v4();
    let db = Arc::new(SqliteStore::open(&path).await.unwrap());
    let content = FileContentStore::open(
        db.clone(),
        ContentConfig {
            directory: dir.path().join("objects"),
            ..Default::default()
        },
        s,
        principal,
        vec![],
    )
    .await
    .unwrap();
    let r = content
        .upload(
            ctx(s, principal),
            3,
            "text/plain".into(),
            Box::pin(std::io::Cursor::new(b"abc")),
        )
        .await
        .unwrap();
    let mut e = event(s);
    e.payload = EventPayload::Content(r.clone());
    db.append(e.clone()).await.unwrap();
    let mut delivery = job(s);
    delivery.message.parts = vec![synquay_server::domain::message::ContentPart::StoredContent(
        r.clone(),
    )];
    db.enqueue(DispatchPlan {
        workspace_id: s.workspace_id,
        key: DispatchKey::Event {
            scope: s,
            event_id: e.event_id,
        },
        deliveries: vec![delivery],
    })
    .await
    .unwrap();
    let lease = db
        .claim(
            s.workspace_id,
            std::time::SystemTime::now(),
            Duration::from_secs(60),
        )
        .await
        .unwrap()
        .unwrap();
    db.mark_sending(lease.token.clone()).await.unwrap();
    db.finish(
        lease.token,
        synquay_server::domain::delivery::DeliveryOutcome::Submitted(
            synquay_server::domain::message::DeliveryReceipt {
                delivery_id: lease.job.delivery_id,
                external_message_id: None,
                status: synquay_server::domain::message::DeliveryStatus::Accepted,
            },
        ),
    )
    .await
    .unwrap();
    old(&path, 10);
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute("UPDATE content_objects SET created_ms=0", [])
        .unwrap();
    assert_eq!(content.collect().await.unwrap(), 0);
    db.maintain(s.workspace_id, RetentionPolicy::default())
        .await
        .unwrap();
    assert_eq!(content.collect().await.unwrap(), 1);
    assert!(matches!(
        content.read(ctx(s, principal), r).await,
        Err(PortError::NotFound)
    ));
    assert_eq!(
        db.append(e).await.unwrap().disposition,
        synquay_server::domain::event::IngestDisposition::Duplicate
    );
}

#[tokio::test]
async fn v3_upgrade_failure_is_atomic_and_reopen_preserves_legacy_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let e = event(scope());
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(include_str!("../src/infrastructure/storage/schema.sql"))
        .unwrap();
    c.execute(
        "INSERT INTO events VALUES(?1,?2,?3,?4,1,?5)",
        rusqlite::params![
            e.scope.workspace_id.to_string(),
            e.scope.connection_id.to_string(),
            e.event_id.to_string(),
            e.source_event_id,
            serde_json::to_string(&e).unwrap()
        ],
    )
    .unwrap();
    c.execute_batch(include_str!(
        "../src/infrastructure/storage/migration_v2.sql"
    ))
    .unwrap();
    c.execute_batch(include_str!(
        "../src/infrastructure/storage/migration_v3.sql"
    ))
    .unwrap();
    c.execute_batch("CREATE TABLE content_objects(collision INTEGER)")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='config_releases'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    c.execute_batch("DROP TABLE content_objects").unwrap();
    drop(c);
    let db = SqliteStore::open(path).await.unwrap();
    assert_eq!(
        db.load(e.scope, e.event_id).await.unwrap().event_id,
        e.event_id
    );
}
