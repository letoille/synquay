mod support;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use std::time::SystemTime;
use support::{TOKEN, config};
use synquay_server::{
    application::ports::connection_data::ConnectionMessageReader,
    domain::{
        builtin_tools::{BuiltinScope, BuiltinTool, ConversationGrant},
        connection_routing::RouteBinding,
        connector::ConnectionScope,
        event::{EventPayload, IncomingEvent},
        recent_messages::ConversationKey,
    },
    infrastructure::{
        connectors::{
            message_config::{ConnectionConfig, MessageConnection},
            qq::QqBotConfig,
        },
        pipeline::{Pipeline, message_connections::ConnectionInput},
    },
};
use uuid::Uuid;

#[tokio::test]
async fn queries_enforce_exact_connection_account_and_conversation_scope() {
    let dir = tempfile::tempdir().unwrap();
    let pipeline = std::sync::Arc::new(
        Pipeline::build(
            config(&dir.path().join("db"), "http://127.0.0.1:1"),
            TOKEN.into(),
            None,
        )
        .await
        .unwrap(),
    );
    let conn = MessageConnection {
        recent_messages:
            synquay_server::infrastructure::connectors::message_config::RecentMessagesConfig {
                enabled: true,
                ..Default::default()
            },
        id: Uuid::new_v4(),
        name: "Test".into(),
        description: String::new(),
        enabled: true,
        config: ConnectionConfig::QqBot(QqBotConfig {
            app_id: "123456".into(),
            resolved_secret: None,
            group_openid: String::new(),
            allowed_senders: vec![],
            allow_proactive: false,
        }),
    };
    pipeline
        .change_connection(
            Uuid::from_u128(10),
            ConnectionInput {
                secret: Some(TOKEN.into()),
                operation_id: Uuid::new_v4(),
                expected_revision: 0,
                connection_id: conn.id,
                connection: Some(conn.clone()),
            },
        )
        .await
        .unwrap();
    let grant = ConversationGrant {
        connection_id: conn.id,
        account: "123456".into(),
        kind: "group".into(),
        conversation_id: "group_A".into(),
    };
    let scope = BuiltinScope {
        tools: vec![
            BuiltinTool::ConnectionsList,
            BuiltinTool::ConnectionStatus,
            BuiltinTool::ConnectionStatistics,
            BuiltinTool::RecentMessages,
        ],
        connections: vec![conn.id],
        conversations: vec![grant.clone()],
        qq_send_conversations: vec![],
    };
    let event = IncomingEvent {
        route_context: Some(RouteBinding::new(
            "qq",
            json!({"app_id":"123456","kind":"group","id":"group_A","sender_openid":"user_A"}),
        )),
        event_type: "qq.group.message".into(),
        schema_version: 1,
        occurred_at: Some(SystemTime::now()),
        source_event_id: Some("message-one".into()),
        payload: EventPayload::Json(json!({"content":"test message"})),
    };
    pipeline
        .accept_from(conn.id, Uuid::new_v4(), event)
        .await
        .unwrap();
    let second = IncomingEvent {
        route_context: Some(RouteBinding::new(
            "qq",
            json!({"app_id":"123456","kind":"group","id":"group_A","sender_openid":"user_A"}),
        )),
        event_type: "qq.group.message".into(),
        schema_version: 1,
        occurred_at: Some(SystemTime::now()),
        source_event_id: Some("message-two".into()),
        payload: EventPayload::Json(json!({"content":"second message"})),
    };
    pipeline
        .accept_from(conn.id, Uuid::new_v4(), second)
        .await
        .unwrap();
    let first = pipeline
        .builtin_recent_messages(&scope, grant.clone(), None, 1)
        .await
        .unwrap();
    let cursor = first["next_cursor"].as_str().unwrap();
    let next = pipeline
        .builtin_recent_messages(&scope, grant.clone(), Some(cursor), 1)
        .await
        .unwrap();
    assert_eq!(next["messages"][0]["text"], "test message");
    let mut decoded: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(cursor).unwrap()).unwrap();
    decoded["generation"] = json!(Uuid::new_v4());
    let expired = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&decoded).unwrap());
    assert!(
        pipeline
            .builtin_recent_messages(&scope, grant.clone(), Some(&expired), 1)
            .await
            .is_err()
    );
    decoded["conversation"] = json!("group_B");
    let wrong = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&decoded).unwrap());
    assert!(
        pipeline
            .builtin_recent_messages(&scope, grant.clone(), Some(&wrong), 1)
            .await
            .is_err()
    );
    let key = ConversationKey {
        scope: ConnectionScope {
            workspace_id: pipeline.source.workspace_id,
            connection_id: conn.id,
        },
        account: "123456".into(),
        kind: "group".into(),
        id: "group_A".into(),
    };
    let page = pipeline
        .read_connection_messages(key.clone(), None, 10)
        .await
        .unwrap();
    assert_eq!(page.messages.len(), 2);
    let mut wrong = key.clone();
    wrong.scope.workspace_id = Uuid::new_v4();
    assert!(
        pipeline
            .read_connection_messages(wrong, None, 10)
            .await
            .is_err()
    );
    let mut wrong = key.clone();
    wrong.account = "other".into();
    assert!(
        pipeline
            .read_connection_messages(wrong, None, 10)
            .await
            .is_err()
    );
    let mut wrong = key;
    wrong.kind = "unknown".into();
    assert!(
        pipeline
            .read_connection_messages(wrong, None, 10)
            .await
            .is_err()
    );
    let result = pipeline.builtin_connections(&scope).await.unwrap();
    assert_eq!(result["connections"].as_array().unwrap().len(), 1);
    assert_eq!(
        pipeline
            .builtin_observation(&scope, BuiltinTool::ConnectionStatistics, conn.id)
            .await
            .unwrap()["received"],
        2
    );
    let result = pipeline
        .builtin_recent_messages(&scope, grant.clone(), None, 10)
        .await
        .unwrap();
    assert_eq!(result["messages"][0]["text"], "second message");
    assert_eq!(result["history_complete"], false);
    let mut other = grant;
    other.conversation_id = "group_B".into();
    assert!(
        pipeline
            .builtin_recent_messages(&scope, other, None, 10)
            .await
            .is_err()
    );
    assert!(
        pipeline
            .builtin_observation(&scope, BuiltinTool::ConnectionStatus, Uuid::new_v4())
            .await
            .is_err()
    );
    assert!(
        pipeline
            .builtin_connections(&BuiltinScope::default())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn mqtt_stream_reads_require_an_explicit_connection_grant() {
    use synquay_server::infrastructure::connectors::message_config::{
        MqttConfig, MqttSubscription, RecentMessagesConfig,
    };
    let dir = tempfile::tempdir().unwrap();
    let pipeline = std::sync::Arc::new(
        Pipeline::build(
            config(&dir.path().join("db"), "http://127.0.0.1:1"),
            TOKEN.into(),
            None,
        )
        .await
        .unwrap(),
    );
    let conn = MessageConnection {
        id: Uuid::new_v4(),
        name: "MQTT".into(),
        description: String::new(),
        enabled: true,
        recent_messages: RecentMessagesConfig {
            enabled: true,
            ..Default::default()
        },
        config: ConnectionConfig::Mqtt(MqttConfig {
            capabilities: Default::default(),
            host: "127.0.0.1".into(),
            port: 1883,
            username: None,
            password_env: None,
            tls: None,
            subscription: Some(MqttSubscription {
                include_metadata: false,
                topic: "home/+/event".into(),
                qos: 1,
                event_type: "mqtt.message".into(),
            }),
            publish: None,
        }),
    };
    pipeline
        .change_connection(
            Uuid::from_u128(10),
            ConnectionInput {
                secret: None,
                operation_id: Uuid::new_v4(),
                expected_revision: 0,
                connection_id: conn.id,
                connection: Some(conn.clone()),
            },
        )
        .await
        .unwrap();
    pipeline
        .accept_from(
            conn.id,
            Uuid::new_v4(),
            IncomingEvent {
                route_context: Some(RouteBinding::new("mqtt", json!({"topic":"home/a/event"}))),
                event_type: "mqtt.message".into(),
                schema_version: 1,
                occurred_at: None,
                source_event_id: None,
                payload: EventPayload::Json(json!({"value":12})),
            },
        )
        .await
        .unwrap();
    let grant = ConversationGrant {
        connection_id: conn.id,
        account: "connection".into(),
        kind: "stream".into(),
        conversation_id: "default".into(),
    };
    let mut scope = BuiltinScope {
        tools: vec![BuiltinTool::RecentMessages],
        connections: vec![conn.id],
        conversations: vec![],
        qq_send_conversations: vec![],
    };
    assert!(
        pipeline
            .builtin_recent_messages(&scope, grant.clone(), None, 10)
            .await
            .is_err()
    );
    scope.conversations.push(grant.clone());
    let result = pipeline
        .builtin_recent_messages(&scope, grant.clone(), None, 10)
        .await
        .unwrap();
    let message: serde_json::Value =
        serde_json::from_str(result["messages"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(message["topic"], "home/a/event");
    assert_eq!(message["payload"]["value"], 12);
    let mut wrong = grant;
    wrong.conversation_id = "other".into();
    assert!(
        pipeline
            .builtin_recent_messages(&scope, wrong, None, 10)
            .await
            .is_err()
    );
}
