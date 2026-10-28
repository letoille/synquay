mod support;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::SystemTime};
use synquay_server::{
    AppState,
    domain::{
        builtin_tools::{BuiltinScope, BuiltinTool, ConversationGrant},
        connection_routing::RouteBinding,
        event::{EventPayload, IncomingEvent},
        mcp_server::{ServerAuth, ServerChange},
    },
    infrastructure::{
        connectors::{
            message_config::{ConnectionConfig, MessageConnection},
            qq::QqBotConfig,
        },
        pipeline::{Pipeline, message_connections::ConnectionInput},
        tool_runtime::{ToolRuntime, config::ToolsConfig},
    },
};
use tower::ServiceExt;
use uuid::Uuid;

async fn rpc(app: &axum::Router, path: &str, method: &str, params: Value) -> Value {
    let request = Request::post(path)
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(Body::from(
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
async fn builtin_tools_are_isolated_and_messages_are_not_persisted_as_invocation_results() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    let mut cfg: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    cfg.database = db.clone();
    let runtime = ToolRuntime::build_with_secrets(
        cfg.clone(),
        support::TOKEN.into(),
        vec![None; cfg.tools.len()],
    )
    .await
    .unwrap();
    let mut pcfg = support::config(&db, "http://127.0.0.1:1");
    pcfg.workspace_id = cfg.workspace_id;
    let pipeline = Arc::new(
        Pipeline::build(pcfg, support::TOKEN.into(), None)
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
        name: "QQ".into(),
        description: String::new(),
        enabled: true,
        config: ConnectionConfig::QqBot(QqBotConfig {
            app_id: "123456".into(),
            resolved_secret: None,
            group_openid: String::new(),
            allowed_senders: vec![],
            allow_proactive: true,
        }),
    };
    pipeline
        .change_connection(
            Uuid::from_u128(10),
            ConnectionInput {
                secret: Some(support::TOKEN.into()),
                operation_id: Uuid::new_v4(),
                expected_revision: 0,
                connection_id: conn.id,
                connection: Some(conn.clone()),
            },
        )
        .await
        .unwrap();
    pipeline.accept_from(conn.id,Uuid::new_v4(),IncomingEvent{route_context:Some(RouteBinding::new("qq",json!({"app_id":"123456","kind":"group","id":"group_A","sender_openid":"user_A"}))),event_type:"qq.group.message".into(),schema_version:1,occurred_at:Some(SystemTime::now()),source_event_id:Some("message-one".into()),payload:EventPayload::Json(json!({"content":"private-message-fixture"}))}).await.unwrap();
    let grant = ConversationGrant {
        connection_id: conn.id,
        account: "123456".into(),
        kind: "group".into(),
        conversation_id: "group_A".into(),
    };
    let scope = BuiltinScope {
        tools: vec![
            BuiltinTool::ConnectionsList,
            BuiltinTool::RecentMessages,
            BuiltinTool::QqBotInfo,
            BuiltinTool::QqSendText,
        ],
        connections: vec![conn.id],
        conversations: vec![grant.clone()],
        qq_send_conversations: vec![grant],
    };
    let id = Uuid::new_v4();
    runtime
        .save_server(
            id,
            ServerChange {
                builtin_scope: Some(scope),
                operation_id: Uuid::new_v4(),
                expected_revision: 0,
                expected_tool_revision: 0,
                name: "builtin test".into(),
                description: None,
                public_url: None,
                content_scope: None,
                expected_content_revision: None,
                enabled: true,
                exposed_tools: vec![],
                auth: Some(ServerAuth::None),
                transport: Default::default(),
            },
        )
        .await
        .unwrap();
    let app = synquay_server::interfaces::http::router(AppState {
        tools: Some(runtime.clone()),
        pipeline: Some(pipeline.clone()),
        ..Default::default()
    });
    let path = format!("/mcp/{id}");
    let isolated = Uuid::new_v4();
    runtime
        .save_server(
            isolated,
            ServerChange {
                builtin_scope: None,
                operation_id: Uuid::new_v4(),
                expected_revision: 0,
                expected_tool_revision: 0,
                name: "isolated".into(),
                description: None,
                public_url: None,
                content_scope: None,
                expected_content_revision: None,
                enabled: true,
                exposed_tools: vec![],
                auth: Some(ServerAuth::None),
                transport: Default::default(),
            },
        )
        .await
        .unwrap();
    let isolated_path = format!("/mcp/{isolated}");
    assert!(
        rpc(&app, &isolated_path, "tools/list", json!({})).await["result"]["tools"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let list = rpc(&app, &path, "tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    let name = tools
        .iter()
        .find_map(|tool| {
            tool["name"]
                .as_str()
                .filter(|name| name.starts_with("synquay_builtin_recent_messages__s"))
        })
        .unwrap()
        .to_string();
    let send_tool = tools
        .iter()
        .find(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("synquay_builtin_qq_send_text__s"))
        })
        .unwrap();
    assert_eq!(send_tool["annotations"]["readOnlyHint"], false);
    let send_name = send_tool["name"].as_str().unwrap();
    let invocation_id = Uuid::new_v4();
    let send_params = json!({"name":send_name,"_meta":{"synquay/invocation-id":invocation_id},"arguments":{"connection_id":conn.id,"kind":"group","conversation_id":"group_B","content":"not delivered"}});
    let denied_send = rpc(&app, &path, "tools/call", send_params.clone()).await;
    assert_eq!(denied_send["result"]["isError"], true);
    assert_eq!(
        denied_send["result"]["_meta"]["synquay/invocation-id"],
        invocation_id.to_string()
    );
    let replay = rpc(&app, &path, "tools/call", send_params.clone()).await;
    assert_eq!(replay["result"]["isError"], true);
    let mut changed = send_params;
    changed["arguments"]["content"] = json!("different intent");
    let conflict = rpc(&app, &path, "tools/call", changed).await;
    assert_eq!(conflict["result"]["isError"], true);
    let cross_send = rpc(&app, &isolated_path, "tools/call", json!({"name":send_name,"arguments":{"connection_id":conn.id,"kind":"group","conversation_id":"group_A","content":"not delivered"}})).await;
    assert_eq!(cross_send["result"]["isError"], true);
    let bot_info = tools
        .iter()
        .find_map(|tool| {
            tool["name"]
                .as_str()
                .filter(|name| name.starts_with("synquay_builtin_qq_bot_info__s"))
        })
        .unwrap();
    let denied_info = rpc(
        &app,
        &path,
        "tools/call",
        json!({"name":bot_info,"arguments":{"connection_id":Uuid::new_v4()}}),
    )
    .await;
    assert_eq!(denied_info["result"]["isError"], true);
    let isolated_info = rpc(
        &app,
        &isolated_path,
        "tools/call",
        json!({"name":bot_info,"arguments":{"connection_id":conn.id}}),
    )
    .await;
    assert_eq!(isolated_info["result"]["isError"], true);
    let response=rpc(&app,&path,"tools/call",json!({"name":name,"arguments":{"connection_id":conn.id,"kind":"group","conversation_id":"group_A"}})).await;
    assert_eq!(
        response["result"]["structuredContent"]["messages"][0]["text"],
        "private-message-fixture"
    );
    let cross=rpc(&app,&isolated_path,"tools/call",json!({"name":name,"arguments":{"connection_id":conn.id,"kind":"group","conversation_id":"group_A"}})).await;
    assert_eq!(cross["result"]["isError"], true);
    let denied=rpc(&app,&path,"tools/call",json!({"name":name,"arguments":{"connection_id":conn.id,"kind":"group","conversation_id":"group_B"}})).await;
    assert_eq!(denied["result"]["isError"], true);
    let view = runtime.server_view(id).await.unwrap();
    runtime
        .save_server(
            id,
            ServerChange {
                builtin_scope: Some(BuiltinScope::default()),
                operation_id: Uuid::new_v4(),
                expected_revision: view["revision"].as_u64().unwrap(),
                expected_tool_revision: view["tool_revision"].as_u64().unwrap(),
                name: "builtin test".into(),
                description: None,
                public_url: None,
                content_scope: None,
                expected_content_revision: None,
                enabled: true,
                exposed_tools: vec![],
                auth: Some(ServerAuth::None),
                transport: Default::default(),
            },
        )
        .await
        .unwrap();
    let stale=rpc(&app,&path,"tools/call",json!({"name":name,"arguments":{"connection_id":conn.id,"kind":"group","conversation_id":"group_A"}})).await;
    assert_eq!(stale["result"]["isError"], true);
    let conn_db = rusqlite::Connection::open(db).unwrap();
    let count: u64 = conn_db
        .query_row("SELECT count(*) FROM tool_invocations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let leaked: u64 = conn_db
        .query_row(
            "SELECT count(*) FROM audit WHERE record LIKE '%private-message-fixture%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0);
}
