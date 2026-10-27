mod support;
use std::time::{Duration, SystemTime};
use support::*;
use synquay_server::{
    application::ports::{
        PortError, delivery::DeliveryOutbox, events::EventStore, routing::EventReader,
    },
    domain::{
        delivery::*,
        event::IngestDisposition,
        message::{DeliveryReceipt, DeliveryStatus},
    },
    infrastructure::storage::SqliteStore,
};
use uuid::Uuid;

#[tokio::test]
async fn concurrent_ingestion_deduplicates_across_connections_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let a = SqliteStore::open(&path).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let b = SqliteStore::open(&path).await.unwrap();
    let scope = scope();
    let first = event(scope);
    let mut second = first.clone();
    second.event_id = Uuid::new_v4();
    let (x, y) = tokio::join!(a.append(first.clone()), b.append(second));
    let x = x.unwrap();
    let y = y.unwrap();
    assert_eq!(x.event_id, y.event_id);
    assert_ne!(x.disposition, y.disposition);
    drop(a);
    drop(b);
    let store = SqliteStore::open(&path).await.unwrap();
    let loaded = store.load(scope, x.event_id).await.unwrap();
    assert_eq!(loaded.source_event_id, first.source_event_id);
    assert_eq!(
        store.unrouted(scope.workspace_id, 100).await.unwrap().len(),
        1
    );
    let mut other = first;
    other.event_id = Uuid::new_v4();
    other.scope.connection_id = Uuid::new_v4();
    assert_eq!(
        store.append(other).await.unwrap().disposition,
        IngestDisposition::Stored
    );
    assert!(matches!(
        store.load(support::scope(), x.event_id).await,
        Err(PortError::NotFound)
    ));
}

#[tokio::test]
async fn empty_dispatch_is_durable_and_duplicate_returns_original_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let scope = scope();
    let e = event(scope);
    store.append(e.clone()).await.unwrap();
    let make = || DispatchPlan {
        workspace_id: scope.workspace_id,
        key: DispatchKey::Event {
            scope,
            event_id: e.event_id,
        },
        deliveries: vec![],
    };
    assert!(!store.enqueue(make()).await.unwrap().duplicate);
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(store.enqueue(make()).await.unwrap().duplicate);
    assert!(
        store
            .unrouted(scope.workspace_id, 100)
            .await
            .unwrap()
            .is_empty()
    );
    let mut changed = make();
    changed.deliveries.push(job(scope));
    assert!(
        store
            .enqueue(changed)
            .await
            .unwrap()
            .delivery_ids
            .is_empty()
    );
    assert!(
        store
            .claim(
                scope.workspace_id,
                SystemTime::now(),
                Duration::from_secs(60)
            )
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn transaction_rolls_back_marker_and_all_jobs_when_any_insert_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("test.db")).await.unwrap();
    let scope = scope();
    let existing = job(scope);
    store.enqueue(plan(existing.clone())).await.unwrap();
    let new = job(scope);
    let mut bad = plan(new.clone());
    let key = bad.key.clone();
    bad.deliveries.push(existing);
    assert!(store.enqueue(bad).await.is_err());
    assert!(matches!(
        store
            .delivery_state(scope.workspace_id, new.delivery_id)
            .await,
        Err(PortError::NotFound)
    ));
    let good = DispatchPlan {
        workspace_id: scope.workspace_id,
        key: key.clone(),
        deliveries: vec![new.clone()],
    };
    let receipt = store.enqueue(good).await.unwrap();
    assert!(!receipt.duplicate);
    assert_eq!(receipt.delivery_ids, vec![new.delivery_id]);
    let mut changed = new;
    changed.message.parts.clear();
    assert!(matches!(
        store
            .enqueue(DispatchPlan {
                workspace_id: scope.workspace_id,
                key,
                deliveries: vec![changed]
            })
            .await,
        Err(PortError::Conflict)
    ));
}

#[tokio::test]
async fn lease_is_exclusive_and_stale_owner_cannot_send_or_complete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let a = SqliteStore::open(&path).await.unwrap();
    let b = SqliteStore::open(&path).await.unwrap();
    let scope = scope();
    let job = job(scope);
    a.enqueue(plan(job.clone())).await.unwrap();
    let now = SystemTime::now();
    let (x, y) = tokio::join!(
        a.claim(scope.workspace_id, now, Duration::from_secs(60)),
        b.claim(scope.workspace_id, now, Duration::from_secs(60))
    );
    let (x, y) = (x.unwrap(), y.unwrap());
    assert_ne!(x.is_some(), y.is_some());
    let lease = x.or(y).unwrap();
    let next = b
        .claim(
            scope.workspace_id,
            now + Duration::from_secs(61),
            Duration::from_secs(60),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.attempt, 2);
    assert_ne!(next.token.lease_id, lease.token.lease_id);
    assert!(matches!(
        a.mark_sending(lease.token.clone()).await,
        Err(PortError::Conflict)
    ));
    assert!(matches!(
        a.finish(lease.token, DeliveryOutcome::Failed { code: "old" })
            .await,
        Err(PortError::Conflict)
    ));
    assert!(
        a.claim(Uuid::new_v4(), now, Duration::from_secs(60))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn crashed_sender_is_reconciled_after_reopen_without_resend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let scope = scope();
    let job = job(scope);
    store.enqueue(plan(job.clone())).await.unwrap();
    let now = SystemTime::now();
    let lease = store
        .claim(scope.workspace_id, now, Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    store.mark_sending(lease.token.clone()).await.unwrap();
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    assert!(
        store
            .claim(
                scope.workspace_id,
                now + Duration::from_secs(61),
                Duration::from_secs(60)
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .delivery_state(scope.workspace_id, job.delivery_id)
            .await
            .unwrap(),
        "reconcile"
    );
    assert!(matches!(
        store
            .finish(
                lease.token,
                DeliveryOutcome::Submitted(DeliveryReceipt {
                    delivery_id: job.delivery_id,
                    external_message_id: None,
                    status: DeliveryStatus::Accepted
                })
            )
            .await,
        Err(PortError::Conflict)
    ));
}

#[tokio::test]
async fn retry_waits_until_due_and_completion_cannot_skip_sending() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("test.db")).await.unwrap();
    let scope = scope();
    let mut job = job(scope);
    job.destination.replay_safety = ReplaySafety::Idempotent;
    store.enqueue(plan(job.clone())).await.unwrap();
    let now = SystemTime::now();
    let lease = store
        .claim(scope.workspace_id, now, Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    let receipt = DeliveryReceipt {
        delivery_id: job.delivery_id,
        external_message_id: None,
        status: DeliveryStatus::Accepted,
    };
    assert!(matches!(
        store
            .finish(
                lease.token.clone(),
                DeliveryOutcome::Submitted(receipt.clone())
            )
            .await,
        Err(PortError::Conflict)
    ));
    store.mark_sending(lease.token.clone()).await.unwrap();
    store
        .finish(
            lease.token,
            DeliveryOutcome::RetryAfter(Duration::from_secs(60)),
        )
        .await
        .unwrap();
    assert!(
        store
            .claim(
                scope.workspace_id,
                SystemTime::now(),
                Duration::from_secs(60)
            )
            .await
            .unwrap()
            .is_none()
    );
    let next = store
        .claim(
            scope.workspace_id,
            SystemTime::now() + Duration::from_secs(61),
            Duration::from_secs(60),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.attempt, 2);
    assert_eq!(next.job.delivery_id, job.delivery_id);
    store.mark_sending(next.token.clone()).await.unwrap();
    store
        .finish(next.token, DeliveryOutcome::Submitted(receipt))
        .await
        .unwrap();
    assert_eq!(
        store
            .delivery_state(scope.workspace_id, job.delivery_id)
            .await
            .unwrap(),
        "submitted"
    );
}

#[tokio::test]
async fn foreign_batches_and_rebinding_database_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("test.db")).await.unwrap();
    let mut plan = plan(job(scope()));
    plan.workspace_id = Uuid::new_v4();
    assert!(matches!(
        store.enqueue(plan).await,
        Err(PortError::ScopeMismatch)
    ));
    store
        .bind_pipeline(serde_json::json!({"target":"a"}))
        .await
        .unwrap();
    store
        .bind_pipeline(serde_json::json!({"target":"a"}))
        .await
        .unwrap();
    assert!(
        store
            .bind_pipeline(serde_json::json!({"target":"b"}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn unknown_schema_version_is_not_downgraded() {
    assert!(matches!(
        SqliteStore::open(":memory:").await,
        Err(PortError::InvalidInput { .. })
    ));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("PRAGMA user_version=99").unwrap();
    }
    assert!(matches!(
        SqliteStore::open(&path).await,
        Err(PortError::Unsupported)
    ));
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        99
    );
}

#[tokio::test]
async fn cumulative_message_counts_deduplicate_finish_survive_cleanup_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("counts.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let scope = scope();
    let e = event(scope);
    store.append(e.clone()).await.unwrap();
    store.append(e.clone()).await.unwrap();
    assert_eq!(
        store
            .message_totals(scope.workspace_id, "received", vec![scope.connection_id])
            .await
            .unwrap()[&scope.connection_id],
        1
    );
    let job = job(scope);
    let target = job.destination.id;
    store.enqueue(plan(job)).await.unwrap();
    let leased = store
        .claim(
            scope.workspace_id,
            SystemTime::now(),
            Duration::from_secs(60),
        )
        .await
        .unwrap()
        .unwrap();
    store.mark_sending(leased.token.clone()).await.unwrap();
    let result = DeliveryOutcome::Submitted(DeliveryReceipt {
        delivery_id: leased.job.delivery_id,
        external_message_id: None,
        status: DeliveryStatus::Accepted,
    });
    store
        .finish(leased.token.clone(), result.clone())
        .await
        .unwrap();
    assert!(store.finish(leased.token, result).await.is_err());
    assert_eq!(
        store
            .message_totals(scope.workspace_id, "sent", vec![target])
            .await
            .unwrap()[&target],
        1
    );
    assert!(
        store
            .message_totals(Uuid::new_v4(), "sent", vec![target])
            .await
            .unwrap()
            .is_empty()
    );
    store
        .enqueue(DispatchPlan {
            workspace_id: scope.workspace_id,
            key: DispatchKey::Event {
                scope,
                event_id: e.event_id,
            },
            deliveries: vec![],
        })
        .await
        .unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute("UPDATE retired_work SET retired_ms=0", [])
        .unwrap();
    store
        .maintain(scope.workspace_id, Default::default())
        .await
        .unwrap();
    for table in ["events", "deliveries"] {
        let remaining: u64 = c
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0, "retention deleted {table}");
    }
    drop(c);
    drop(store);
    let reopened = SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .message_totals(scope.workspace_id, "received", vec![scope.connection_id])
            .await
            .unwrap()[&scope.connection_id],
        1
    );
    assert_eq!(
        reopened
            .message_totals(scope.workspace_id, "sent", vec![target])
            .await
            .unwrap()[&target],
        1
    );
}
