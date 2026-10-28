mod support;
use std::time::{Duration, SystemTime};
use support::*;
use synquay_server::{
    application::ports::{
        PortError, delivery::DeliveryOutbox, events::EventStore, routing::EventReader,
    },
    domain::{delivery::*, event::*, message::*},
    infrastructure::storage::{
        SqliteStore,
        capacity::{CapacityLimits, MAX_RECORD_BYTES},
    },
};
use uuid::Uuid;

fn automatic(e: &EventEnvelope, deliveries: Vec<PendingDelivery>) -> DispatchPlan {
    DispatchPlan {
        workspace_id: e.scope.workspace_id,
        key: DispatchKey::Event {
            scope: e.scope,
            event_id: e.event_id,
        },
        deliveries,
    }
}
async fn sent(store: &SqliteStore, workspace: Uuid) {
    let lease = store
        .claim(workspace, SystemTime::now(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    store.mark_sending(lease.token.clone()).await.unwrap();
    store
        .finish(
            lease.token,
            DeliveryOutcome::Submitted(DeliveryReceipt {
                delivery_id: lease.job.delivery_id,
                external_message_id: None,
                status: DeliveryStatus::Accepted,
            }),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn concurrent_capacity_is_atomic_and_duplicates_survive_high_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.db");
    let a = SqliteStore::open(&path).await.unwrap();
    a.configure_capacity(CapacityLimits {
        max_pending_items: 2,
        ..Default::default()
    })
    .await
    .unwrap();
    let b = SqliteStore::open(&path).await.unwrap();
    let scope = scope();
    let mut tasks = Vec::new();
    for i in 0..16 {
        let store = if i % 2 == 0 { a.clone() } else { b.clone() };
        let mut e = event(scope);
        e.source_event_id = Some(format!("{i}"));
        tasks.push(tokio::spawn(
            async move { (e.clone(), store.append(e).await) },
        ));
    }
    let mut accepted = Vec::new();
    for task in tasks {
        let (e, r) = task.await.unwrap();
        match r {
            Ok(_) => accepted.push(e),
            Err(PortError::CapacityExceeded { .. }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert_eq!(accepted.len(), 2);
    let status = a.backlog_status(scope.workspace_id).await.unwrap();
    assert_eq!(status.pending_items, 2);
    assert!(status.paused);
    assert_eq!(
        b.append(accepted[0].clone()).await.unwrap().disposition,
        IngestDisposition::Duplicate
    );
    // Empty dispatch completes one work item and lowers occupancy to <=80%.
    a.enqueue(automatic(&accepted[0], vec![])).await.unwrap();
    let mut next = event(scope);
    next.source_event_id = Some("after-drain".into());
    b.append(next).await.unwrap();
    assert_eq!(
        b.backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_items,
        2
    );
    drop(a);
    drop(b);
    assert_eq!(
        SqliteStore::open(&path)
            .await
            .unwrap()
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_items,
        2
    );
}

#[tokio::test]
async fn routing_preserves_reservation_until_every_target_accepts() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("all.db")).await.unwrap();
    let scope = scope();
    let e = event(scope);
    store.append(e.clone()).await.unwrap();
    let before = store.backlog_status(scope.workspace_id).await.unwrap();
    store
        .enqueue(automatic(&e, vec![job(scope), job(scope)]))
        .await
        .unwrap();
    let after = store.backlog_status(scope.workspace_id).await.unwrap();
    assert_eq!(after.pending_items, 1);
    assert_eq!(before.pending_bytes, after.pending_bytes);
    sent(&store, scope.workspace_id).await;
    assert_eq!(
        store
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_items,
        1
    );
    sent(&store, scope.workspace_id).await;
    assert_eq!(
        store
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_bytes,
        0
    );
    assert!(
        store.load(scope, e.event_id).await.is_ok(),
        "completion must not delete event history"
    );
}

#[tokio::test]
async fn byte_limits_disk_pressure_and_large_records_do_not_accept_new_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bytes.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let scope = scope();
    let e = event(scope);
    let size = serde_json::to_vec(&serde_json::json!({"storage_version":2,"data":&e}))
        .unwrap()
        .len() as u64;
    store
        .configure_capacity(CapacityLimits {
            max_pending_bytes: size + 16,
            ..Default::default()
        })
        .await
        .unwrap();
    store.append(e.clone()).await.unwrap();
    let mut next = e.clone();
    next.event_id = Uuid::new_v4();
    next.source_event_id = Some("another".into());
    assert!(matches!(
        store.append(next.clone()).await,
        Err(PortError::CapacityExceeded { .. })
    ));
    store
        .configure_capacity(CapacityLimits {
            min_free_disk_bytes: u64::MAX,
            resume_free_disk_bytes: u64::MAX,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(matches!(
        store.append(next.clone()).await,
        Err(PortError::CapacityExceeded { reason: "disk_low" })
    ));
    assert_eq!(
        store.append(e.clone()).await.unwrap().disposition,
        IngestDisposition::Duplicate
    );
    assert_eq!(
        store
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_items,
        1
    );
    // A new connection retains the operator's policy instead of resetting to defaults.
    let other = SqliteStore::open(&path).await.unwrap();
    assert!(matches!(
        other.append(next.clone()).await,
        Err(PortError::CapacityExceeded { reason: "disk_low" })
    ));
    store
        .configure_capacity(CapacityLimits::default())
        .await
        .unwrap();
    next.payload = EventPayload::Json(serde_json::json!({"huge":"x".repeat(MAX_RECORD_BYTES)}));
    assert!(matches!(
        store.append(next).await,
        Err(PortError::PayloadTooLarge)
    ));
    assert_eq!(
        store
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_items,
        1
    );
}

#[tokio::test]
async fn failed_or_unknown_deliveries_keep_capacity_and_empty_replies_do_not_charge() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("unknown.db"))
        .await
        .unwrap();
    let scope = scope();
    for outcome in [
        DeliveryOutcome::Reconcile,
        DeliveryOutcome::Failed { code: "rejected" },
    ] {
        store.enqueue(plan(job(scope))).await.unwrap();
        let lease = store
            .claim(
                scope.workspace_id,
                SystemTime::now(),
                Duration::from_secs(60),
            )
            .await
            .unwrap()
            .unwrap();
        store.mark_sending(lease.token.clone()).await.unwrap();
        store.finish(lease.token, outcome).await.unwrap();
    }
    assert_eq!(
        store
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_items,
        2
    );
    let mut empty = plan(job(scope));
    empty.deliveries.clear();
    store.enqueue(empty).await.unwrap();
    assert_eq!(
        store
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .pending_items,
        2
    );
}

#[tokio::test]
async fn migration_backfills_only_unfinished_work_and_preserves_dispatches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let scope = scope();
    let mut events = Vec::new();
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(include_str!("../src/infrastructure/storage/schema.sql"))
            .unwrap();
        for (i, state) in [None, Some("submitted"), Some("reconcile")]
            .into_iter()
            .enumerate()
        {
            let mut e = event(scope);
            e.source_event_id = Some(format!("legacy-{i}"));
            c.execute(
                "INSERT INTO events VALUES(?1,?2,?3,?4,1,?5)",
                rusqlite::params![
                    scope.workspace_id.to_string(),
                    scope.connection_id.to_string(),
                    e.event_id.to_string(),
                    e.source_event_id,
                    serde_json::to_string(&e).unwrap()
                ],
            )
            .unwrap();
            if let Some(state) = state {
                let j = job(scope);
                let key = format!("event:{}:{}", scope.connection_id, e.event_id);
                c.execute(
                    "INSERT INTO dispatches VALUES(?1,?2,?3,NULL)",
                    rusqlite::params![
                        scope.workspace_id.to_string(),
                        key,
                        serde_json::to_string(&vec![j.delivery_id]).unwrap()
                    ],
                )
                .unwrap();
                c.execute("INSERT INTO deliveries(workspace,delivery_id,dispatch_key,record,state,due_ms) VALUES(?1,?2,?3,?4,?5,1)",rusqlite::params![scope.workspace_id.to_string(),j.delivery_id.to_string(),key,serde_json::to_string(&j).unwrap(),state]).unwrap();
            }
            events.push(e);
        }
    }
    for _ in 0..2 {
        let store = SqliteStore::open(&path).await.unwrap();
        assert_eq!(
            store
                .backlog_status(scope.workspace_id)
                .await
                .unwrap()
                .pending_items,
            2
        );
        assert_eq!(
            store.unrouted(scope.workspace_id, 100).await.unwrap(),
            vec![(scope, events[0].event_id)]
        );
        assert!(
            store
                .enqueue(automatic(&events[1], vec![]))
                .await
                .unwrap()
                .duplicate
        );
    }
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        21
    );
}

#[tokio::test]
async fn retryable_routes_backoff_and_permanent_errors_leave_the_ready_window() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("routes.db"))
        .await
        .unwrap();
    let scope = scope();
    let first = event(scope);
    store.append(first.clone()).await.unwrap();
    store
        .record_routing_failure(scope, first.event_id, PortError::Unavailable)
        .await
        .unwrap();
    assert!(
        store
            .unrouted(scope.workspace_id, 100)
            .await
            .unwrap()
            .is_empty()
    );
    for i in 0..101 {
        let mut e = event(scope);
        e.source_event_id = Some(format!("bad-{i}"));
        store.append(e.clone()).await.unwrap();
        store
            .record_routing_failure(scope, e.event_id, PortError::Unsupported)
            .await
            .unwrap();
    }
    let mut good = event(scope);
    good.source_event_id = Some("good".into());
    store.append(good.clone()).await.unwrap();
    assert_eq!(
        store.unrouted(scope.workspace_id, 100).await.unwrap(),
        vec![(scope, good.event_id)]
    );
    store.enqueue(automatic(&good, vec![])).await.unwrap();
    store
        .record_routing_failure(scope, good.event_id, PortError::Unsupported)
        .await
        .unwrap();
    assert_eq!(
        store
            .backlog_status(scope.workspace_id)
            .await
            .unwrap()
            .blocked_routes,
        101
    );
}

#[tokio::test]
async fn failed_migration_rolls_back_schema_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.db");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(include_str!("../src/infrastructure/storage/schema.sql"))
            .unwrap();
        // Force failure after some v2 statements have executed.
        c.execute_batch("CREATE TABLE routing_tasks(conflict INTEGER)")
            .unwrap();
    }
    assert!(SqliteStore::open(&path).await.is_err());
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('capacity_policy','pending_work')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
