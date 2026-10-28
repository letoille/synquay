mod support;
use std::time::{Duration, SystemTime};
use support::*;
use synquay_server::{
    application::ports::{
        PortError, delivery::DeliveryOutbox, events::EventStore, recovery::RecoveryStore,
        routing::EventReader,
    },
    domain::{
        delivery::*,
        message::{DeliveryReceipt, DeliveryStatus},
        recovery::*,
    },
    infrastructure::storage::SqliteStore,
};
use uuid::Uuid;
fn command(id: Uuid, state: &str, action: RecoveryAction) -> RecoveryCommand {
    RecoveryCommand {
        command_id: Uuid::new_v4(),
        target: RecoveryTarget::Delivery { delivery_id: id },
        expected_state: state.into(),
        expected_revision: 0,
        action,
        reason: "operator recovery test".into(),
        evidence: None,
        confirmed_not_submitted: false,
    }
}
async fn failed(store: &SqliteStore, w: Uuid, outcome: DeliveryOutcome) -> Uuid {
    let s = synquay_server::domain::connector::ConnectionScope {
        workspace_id: w,
        connection_id: Uuid::new_v4(),
    };
    let j = job(s);
    let id = j.delivery_id;
    store.enqueue(plan(j)).await.unwrap();
    let l = store
        .claim(w, SystemTime::now(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    store.mark_sending(l.token.clone()).await.unwrap();
    store.finish(l.token, outcome).await.unwrap();
    id
}
#[tokio::test]
async fn unknown_replay_requires_evidence_and_is_idempotent_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let w = Uuid::new_v4();
    let p = Uuid::new_v4();
    let id = failed(&store, w, DeliveryOutcome::Reconcile).await;
    let mut cmd = command(id, "reconcile", RecoveryAction::Replay);
    assert!(matches!(
        store.recover(w, p, cmd.clone()).await,
        Err(PortError::InvalidInput { .. })
    ));
    cmd.confirmed_not_submitted = true;
    cmd.evidence = Some("provider lookup: absent".into());
    let a = store.recover(w, p, cmd.clone()).await.unwrap();
    let new_id = a.new_delivery_id.unwrap();
    assert_ne!(id, new_id);
    assert_eq!(store.backlog_status(w).await.unwrap().pending_items, 1);
    drop(store);
    let store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(
        store
            .recover(w, p, cmd.clone())
            .await
            .unwrap()
            .new_delivery_id,
        Some(new_id)
    );
    cmd.reason = "changed intent".into();
    assert!(matches!(
        store.recover(w, p, cmd).await,
        Err(PortError::Conflict)
    ));
    let l = store
        .claim(w, SystemTime::now(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(l.job.delivery_id, new_id);
    store.mark_sending(l.token.clone()).await.unwrap();
    store
        .finish(
            l.token,
            DeliveryOutcome::Submitted(DeliveryReceipt {
                delivery_id: new_id,
                external_message_id: None,
                status: DeliveryStatus::Accepted,
            }),
        )
        .await
        .unwrap();
    assert_eq!(store.backlog_status(w).await.unwrap().pending_items, 0);
    let c = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM recovery_actions", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM delivery_attempts WHERE finished_ms IS NOT NULL",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        c.query_row(
            "SELECT parent_delivery_id FROM deliveries WHERE delivery_id=?1",
            [new_id.to_string()],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        id.to_string()
    );
}
#[tokio::test]
async fn abandon_and_confirm_release_capacity_without_resending() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("a.db")).await.unwrap();
    let w = Uuid::new_v4();
    let p = Uuid::new_v4();
    let a = failed(&store, w, DeliveryOutcome::Failed { code: "rejected" }).await;
    let b = failed(&store, w, DeliveryOutcome::Reconcile).await;
    let cmd = command(a, "failed", RecoveryAction::Abandon);
    assert!(matches!(
        store.recover(Uuid::new_v4(), p, cmd.clone()).await,
        Err(PortError::NotFound)
    ));
    store.recover(w, p, cmd).await.unwrap();
    let mut cmd = command(b, "reconcile", RecoveryAction::ConfirmSubmitted);
    assert!(store.recover(w, p, cmd.clone()).await.is_err());
    cmd.evidence = Some("provider message 123 accepted".into());
    store.recover(w, p, cmd).await.unwrap();
    assert_eq!(store.backlog_status(w).await.unwrap().pending_items, 0);
    assert!(
        store
            .claim(w, SystemTime::now(), Duration::from_secs(60))
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn route_recovery_revision_rejects_stale_intent_after_another_failure() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("a.db")).await.unwrap();
    let s = scope();
    let e = event(s);
    store.append(e.clone()).await.unwrap();
    store
        .record_routing_failure(s, e.event_id, PortError::Unsupported)
        .await
        .unwrap();
    let mut cmd = command(e.event_id, "blocked", RecoveryAction::Replay);
    cmd.target = RecoveryTarget::Route {
        source_id: s.connection_id,
        event_id: e.event_id,
    };
    let p = Uuid::new_v4();
    store.recover(s.workspace_id, p, cmd.clone()).await.unwrap();
    assert_eq!(store.unrouted(s.workspace_id, 100).await.unwrap().len(), 1);
    store
        .record_routing_failure(s, e.event_id, PortError::Unsupported)
        .await
        .unwrap();
    cmd.command_id = Uuid::new_v4();
    cmd.action = RecoveryAction::Abandon;
    assert!(matches!(
        store.recover(s.workspace_id, p, cmd.clone()).await,
        Err(PortError::Conflict)
    ));
    cmd.expected_revision = 1;
    store.recover(s.workspace_id, p, cmd).await.unwrap();
    assert_eq!(
        store
            .backlog_status(s.workspace_id)
            .await
            .unwrap()
            .pending_items,
        0
    );
}
#[tokio::test]
async fn recovery_audit_failure_rolls_back_state_and_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let w = Uuid::new_v4();
    let id = failed(&store, w, DeliveryOutcome::Failed { code: "rejected" }).await;
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TRIGGER reject_recovery BEFORE INSERT ON recovery_actions BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        store
            .recover(
                w,
                Uuid::new_v4(),
                command(id, "failed", RecoveryAction::Abandon)
            )
            .await
            .is_err()
    );
    assert_eq!(store.backlog_status(w).await.unwrap().pending_items, 1);
    assert_eq!(
        c.query_row("SELECT resolution FROM deliveries", [], |r| r
            .get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
}
#[tokio::test]
async fn strict_target_order_waits_for_resolution_and_other_targets_continue() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("a.db")).await.unwrap();
    let s = scope();
    let a = job(s);
    let mut b = job(s);
    b.destination = a.destination.clone();
    let c = job(s);
    let aid = a.delivery_id;
    let bid = b.delivery_id;
    let at = a.destination.id;
    let ct = c.destination.id;
    for j in [a, b, c] {
        store.enqueue(plan(j)).await.unwrap();
    }
    let l = store
        .claim_target(
            s.workspace_id,
            at,
            true,
            SystemTime::now(),
            Duration::from_secs(60),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(l.job.delivery_id, aid);
    store
        .finish(l.token, DeliveryOutcome::Failed { code: "failed" })
        .await
        .unwrap();
    assert!(
        store
            .claim_target(
                s.workspace_id,
                at,
                true,
                SystemTime::now(),
                Duration::from_secs(60)
            )
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .claim_target(
                s.workspace_id,
                ct,
                true,
                SystemTime::now(),
                Duration::from_secs(60)
            )
            .await
            .unwrap()
            .is_some()
    );
    store
        .recover(
            s.workspace_id,
            Uuid::new_v4(),
            command(aid, "failed", RecoveryAction::Abandon),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .claim_target(
                s.workspace_id,
                at,
                true,
                SystemTime::now(),
                Duration::from_secs(60)
            )
            .await
            .unwrap()
            .unwrap()
            .job
            .delivery_id,
        bid
    );
}
#[tokio::test]
async fn v2_migration_preserves_target_and_does_not_invent_attempt_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v2.db");
    let s = scope();
    let j = job(s);
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(include_str!("../src/infrastructure/storage/schema.sql"))
            .unwrap();
        c.execute_batch(include_str!(
            "../src/infrastructure/storage/migration_v2.sql"
        ))
        .unwrap();
        let key = "reply:legacy";
        c.execute(
            "INSERT INTO dispatches(workspace,dispatch_key,receipt) VALUES(?1,?2,'[]')",
            rusqlite::params![s.workspace_id.to_string(), key],
        )
        .unwrap();
        c.execute("INSERT INTO deliveries(workspace,delivery_id,dispatch_key,record,state,due_ms) VALUES(?1,?2,?3,?4,'pending',0)",rusqlite::params![s.workspace_id.to_string(),j.delivery_id.to_string(),key,serde_json::to_string(&j).unwrap()]).unwrap();
    }
    let store = SqliteStore::open(&path).await.unwrap();
    let l = store
        .claim_target(
            s.workspace_id,
            j.destination.id,
            false,
            SystemTime::now(),
            Duration::from_secs(60),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(l.job.delivery_id, j.delivery_id);
    let c = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        21
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn failed_v3_migration_rolls_back_added_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v2.db");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(include_str!("../src/infrastructure/storage/schema.sql"))
        .unwrap();
    c.execute_batch(include_str!(
        "../src/infrastructure/storage/migration_v2.sql"
    ))
    .unwrap();
    c.execute_batch("CREATE TABLE delivery_attempts(conflict INTEGER)")
        .unwrap();
    assert!(SqliteStore::open(&path).await.is_err());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('deliveries') WHERE name='resolution'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}
