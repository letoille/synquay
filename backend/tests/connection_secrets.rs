mod support;
use std::sync::Arc;
use support::*;
use synquay_server::{
    application::ports::PortError,
    infrastructure::{
        connectors::{
            message_config::{ConnectionConfig, MessageConnection},
            qq::QqBotConfig,
        },
        pipeline::{Pipeline, message_connections::ConnectionInput},
    },
};
use uuid::Uuid;

const SECRET: &str = "test-only-inline-qq-secret-123456";
fn connection() -> MessageConnection {
    MessageConnection {
        id: Uuid::new_v4(),
        name: "Bot".into(),
        description: String::new(),
        enabled: true,
        config: ConnectionConfig::QqBot(QqBotConfig {
            app_id: "123456".into(),
            resolved_secret: None,
            group_openid: String::new(),
            allowed_senders: vec![],
            allow_proactive: false,
        }),
    }
}
fn change(c: &MessageConnection, revision: u64, secret: Option<&str>) -> ConnectionInput {
    ConnectionInput {
        operation_id: Uuid::new_v4(),
        expected_revision: revision,
        connection_id: c.id,
        connection: Some(c.clone()),
        secret: secret.map(str::to_owned),
    }
}
fn secret(p: &Pipeline) -> String {
    let snapshot = p.connection_snapshot().unwrap();
    assert!(!serde_json::to_string(&snapshot).unwrap().contains(SECRET));
    let ConnectionConfig::QqBot(qq) = &snapshot.connections[0].config else {
        panic!()
    };
    qq.secret().unwrap()
}
#[tokio::test]
async fn inline_secret_is_encrypted_replayed_retained_rotated_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    let cfg = || config(&db, "http://127.0.0.1:1/messages");
    let p = Arc::new(Pipeline::build(cfg(), TOKEN.into(), None).await.unwrap());
    let actor = Uuid::new_v4();
    let c = connection();
    assert!(matches!(
        p.change_connection(actor, change(&c, 0, None)).await,
        Err(PortError::InvalidInput {
            code: "qq_secret_required"
        })
    ));
    let input = change(&c, 0, Some(SECRET));
    assert_eq!(p.change_connection(actor, input.clone()).await.unwrap(), 1);
    assert_eq!(p.change_connection(actor, input.clone()).await.unwrap(), 1);
    let mut altered = input.clone();
    altered.secret = Some("different".into());
    assert!(matches!(
        p.change_connection(actor, altered).await,
        Err(PortError::Conflict)
    ));
    assert_eq!(secret(&p), SECRET);
    p.change_connection(actor, change(&c, 1, Some("")))
        .await
        .unwrap();
    assert_eq!(secret(&p), SECRET);
    let mut new_account = c.clone();
    if let ConnectionConfig::QqBot(qq) = &mut new_account.config {
        qq.app_id = "654321".into();
    }
    assert!(matches!(
        p.change_connection(actor, change(&new_account, 2, None))
            .await,
        Err(PortError::InvalidInput {
            code: "qq_secret_required"
        })
    ));
    drop(p);
    let p = Arc::new(Pipeline::build(cfg(), TOKEN.into(), None).await.unwrap());
    assert_eq!(secret(&p), SECRET);
    assert_eq!(p.change_connection(actor, input).await.unwrap(), 1);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let record: String = conn
        .query_row("SELECT record FROM message_connections", [], |r| r.get(0))
        .unwrap();
    assert!(!record.contains(SECRET));
    let mut stmt = conn
        .prepare("SELECT request FROM message_connection_operations")
        .unwrap();
    for row in stmt.query_map([], |r| r.get::<_, String>(0)).unwrap() {
        assert!(!row.unwrap().contains(SECRET));
    }
    let encrypted: Vec<u8> = conn
        .query_row("SELECT encrypted FROM connection_secrets", [], |r| r.get(0))
        .unwrap();
    assert!(
        !encrypted
            .windows(SECRET.len())
            .any(|w| w == SECRET.as_bytes())
    );
    p.change_connection(actor, change(&c, 2, Some("rotated-test-secret")))
        .await
        .unwrap();
    assert_eq!(secret(&p), "rotated-test-secret");
    let mut deletion = change(&c, 3, None);
    deletion.connection = None;
    p.change_connection(actor, deletion).await.unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM connection_secrets", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(p);
    assert!(
        Pipeline::build(cfg(), TOKEN.into(), None)
            .await
            .unwrap()
            .connection_snapshot()
            .unwrap()
            .connections
            .is_empty()
    );
}
#[tokio::test]
async fn missing_encryption_key_fails_closed_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    let cfg = || config(&db, "http://127.0.0.1:1/messages");
    let p = Arc::new(Pipeline::build(cfg(), TOKEN.into(), None).await.unwrap());
    let c = connection();
    p.change_connection(Uuid::new_v4(), change(&c, 0, Some(SECRET)))
        .await
        .unwrap();
    drop(p);
    std::fs::remove_file(dir.path().join("test.db.connection-secrets.key")).unwrap();
    assert!(Pipeline::build(cfg(), TOKEN.into(), None).await.is_err());
    assert!(!dir.path().join("test.db.connection-secrets.key").exists());
}

#[tokio::test]
async fn backup_restores_connection_credentials_and_rejects_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    let p = Arc::new(
        Pipeline::build(
            config(&db, "http://127.0.0.1:1/messages"),
            TOKEN.into(),
            None,
        )
        .await
        .unwrap(),
    );
    let c = connection();
    p.change_connection(Uuid::new_v4(), change(&c, 0, Some(SECRET)))
        .await
        .unwrap();
    drop(p);
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/backup.py");
    let bundle = dir.path().join("backup");
    let restored = dir.path().join("restored");
    for (operation, source, target) in [("backup", &db, &bundle), ("restore", &bundle, &restored)] {
        let output = std::process::Command::new("python3")
            .arg(&script)
            .arg(operation)
            .arg(source)
            .arg(target)
            .arg("--offline")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let p = Pipeline::build(
        config(
            &restored.join("database.sqlite3"),
            "http://127.0.0.1:1/messages",
        ),
        TOKEN.into(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(secret(&p), SECRET);
    std::fs::remove_file(bundle.join("connection-secrets.key")).unwrap();
    let output = std::process::Command::new("python3")
        .arg(&script)
        .arg("verify")
        .arg(&bundle)
        .output()
        .unwrap();
    assert!(!output.status.success());
}
