use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn admission_is_bounded_and_cancelling_waiters_reclaims_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("queue.db"))
        .await
        .unwrap();
    let connection = store.connection.clone().lock_owned().await;
    let ran = Arc::new(AtomicBool::new(false));
    let mut tasks = Vec::new();
    for _ in 0..DATABASE_ADMISSION_LIMIT {
        let store = store.clone();
        let ran = ran.clone();
        tasks.push(tokio::spawn(async move {
            store
                .run(move |_| {
                    ran.store(true, Ordering::SeqCst);
                    Ok(())
                })
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.admission.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        store.run(|_| Ok(())).await,
        Err(PortError::RateLimited { .. })
    ));
    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    assert_eq!(
        store.admission.available_permits(),
        DATABASE_ADMISSION_LIMIT
    );
    assert!(!ran.load(Ordering::SeqCst));
    drop(connection);
    assert!(store.run(|_| Ok(())).await.is_ok());
}

#[tokio::test]
async fn cancelling_caller_keeps_running_sql_capacity_until_commit_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("cancel.db"))
        .await
        .unwrap();
    let worker_store = store.clone();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let task = tokio::spawn(async move {
        worker_store
            .run(move |c| {
                started.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                c.execute_batch("CREATE TABLE committed_after_cancel(id INTEGER)")
                    .map_err(db_error)
            })
            .await
    });
    ready.await.unwrap();
    task.abort();
    let _ = task.await;
    assert_eq!(
        store.admission.available_permits(),
        DATABASE_ADMISSION_LIMIT - 1
    );
    assert!(store.connection.try_lock().is_err());
    release.send(()).unwrap();
    let committed = store
        .run(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='committed_after_cancel')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(db_error)
        })
        .await
        .unwrap();
    assert!(committed);
}

#[tokio::test]
async fn normal_admission_saturation_does_not_consume_completion_reserve() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("reserved.db"))
        .await
        .unwrap();
    let _normal = store
        .admission
        .clone()
        .acquire_many_owned(DATABASE_ADMISSION_LIMIT as u32)
        .await
        .unwrap();
    assert!(matches!(
        store.run(|_| Ok(())).await,
        Err(PortError::RateLimited { .. })
    ));
    store
        .run_critical(|c| {
            c.execute_batch("CREATE TABLE completed(id INTEGER)")
                .map_err(db_error)
        })
        .await
        .unwrap();
    assert_eq!(
        store.critical_admission.available_permits(),
        DATABASE_CRITICAL_LIMIT
    );
}
