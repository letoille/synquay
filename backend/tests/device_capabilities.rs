mod support;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use support::*;
use synquay_server::{
    application::{
        ports::{PortError, events::EventStore},
        services::tools::PublishedToolCall,
    },
    domain::{connection_routing::RouteBinding, connector::ConnectionScope, event::EventPayload},
    infrastructure::{
        connectors::message_config::MessageConnection,
        pipeline::{Pipeline, message_connections::ConnectionInput},
        storage::SqliteStore,
        tool_runtime::{ToolRuntime, config::ToolsConfig},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

fn connection(port: u16) -> MessageConnection {
    serde_json::from_value(json!({"id":Uuid::new_v4(),"name":"Switch","enabled":true,"config":{"kind":"mqtt","host":"127.0.0.1","port":port,"subscription":{"topic":"home/switch/pub","qos":1,"event_type":"device.report","include_metadata":true},"publish":{"topic":"home/switch/sub","qos":1,"retain":false,"body":"payload"},"capabilities":{
 "tools":[{"id":"off","description":"Turn off switch","input_schema":{"type":"object","properties":{},"additionalProperties":false},"topic":"home/switch/sub","payload":{"type":{"source":"literal","value":"event"},"key":{"source":"literal","value":0},"messageId":{"source":"invocation_id"}},"acknowledgement":{"topic":"home/switch/pub","correlation_pointer":"/messageId","result_pointer":"/key","expected":0,"timeout_ms":500}}],
 "resources":[{"id":"switch","name":"Switch","description":"Device state","groups":[
 {"id":"basic","name":"Basic","topic":"home/switch/pub","max_age_ms":0,"fields":[{"name":"mac","pointer":"/mac","value_type":"string"}]},
 {"id":"power","name":"Power","topic":"home/switch/pub","max_age_ms":1000,"fields":[{"name":"on","pointer":"/key","value_type":"boolean","equals":1},{"name":"watts","pointer":"/power","value_type":"number"}]}]}]
 }}})).unwrap()
}
async fn setup(
    port: u16,
) -> (
    tempfile::TempDir,
    Arc<Pipeline>,
    Arc<ToolRuntime>,
    Arc<SqliteStore>,
    MessageConnection,
    ToolsConfig,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let p = Arc::new(
        Pipeline::build(
            config(&path, "http://127.0.0.1:9193/messages"),
            TOKEN.into(),
            None,
        )
        .await
        .unwrap(),
    );
    let c = connection(port);
    p.change_connection(
        Uuid::from_u128(10),
        ConnectionInput {
            secret: None,
            operation_id: Uuid::new_v4(),
            expected_revision: 0,
            connection_id: c.id,
            connection: Some(c.clone()),
        },
    )
    .await
    .unwrap();
    let mut config: ToolsConfig =
        serde_json::from_str(include_str!("../../examples/tools.json")).unwrap();
    config.database = path.clone();
    config.tools.clear();
    config.profile_id = Uuid::from_u128(77);
    let t = ToolRuntime::build_with_secrets(config.clone(), TOKEN.into(), vec![])
        .await
        .unwrap();
    let s = Arc::new(SqliteStore::open(path).await.unwrap());
    (dir, p, t, s, c, config)
}
async fn report(s: &SqliteStore, c: &MessageConnection, value: Value, ago: u64, retained: bool) {
    let scope = ConnectionScope {
        workspace_id: Uuid::from_u128(1),
        connection_id: c.id,
    };
    let mut e = event(scope);
    e.source_event_id = None;
    e.received_at = SystemTime::now() - Duration::from_millis(ago);
    e.route_context = Some(RouteBinding::new(
        "mqtt",
        json!({"topic":"home/switch/pub","retain":retained}),
    ));
    e.payload = EventPayload::Json(json!({"payload":value}));
    s.append(e).await.unwrap();
}
async fn expose(t: &Arc<ToolRuntime>, c: &MessageConnection) -> Value {
    let v = t.server_view(t.config.profile_id).await.unwrap();
    t.save_server(t.config.profile_id,serde_json::from_value(json!({"operation_id":Uuid::new_v4(),"expected_revision":v["revision"],"expected_tool_revision":v["tool_revision"],"name":"Device tools","enabled":true,"exposed_tools":[format!("mqtt_{}_off",c.id.simple()),format!("mqtt_{}_state_switch",c.id.simple())]})).unwrap()).await.unwrap()
}
#[tokio::test]
async fn grouped_state_projection_survives_restart_and_publication() {
    let (_dir, p, t, s, c, config) = setup(1883).await;
    report(&s, &c, json!({"mac":"test-device","key":1}), 2000, false).await;
    report(&s, &c, json!({"power":42}), 0, false).await;
    report(&s, &c, json!({"key":0}), 0, true).await;
    let v = t.device_states(c.id).await.unwrap();
    assert_eq!(
        v["switch"]["groups"]["basic"]["fields"]["mac"]["freshness"],
        "fresh"
    );
    assert_eq!(
        v["switch"]["groups"]["power"]["fields"]["on"]["value"],
        true
    );
    assert_eq!(
        v["switch"]["groups"]["power"]["fields"]["on"]["freshness"],
        "stale"
    );
    assert_eq!(
        v["switch"]["groups"]["power"]["fields"]["watts"]["freshness"],
        "fresh"
    );
    assert!(matches!(
        s.resource_states(ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: c.id
        })
        .await,
        Err(PortError::NotFound)
    ));
    let server = expose(&t, &c).await;
    let ctx = t.context(Default::default());
    let listing = t.service.list(&ctx, t.config.profile_id).await.unwrap();
    assert_eq!(listing.tools.len(), 2);
    let result = t
        .service
        .invoke(
            &ctx,
            PublishedToolCall {
                profile_id: t.config.profile_id,
                revision: server["tool_revision"].as_u64().unwrap(),
                public_name: format!("mqtt_{}_state_switch", c.id.simple()),
                invocation_id: Uuid::new_v4(),
                arguments: json!({"group":"power"}),
            },
        )
        .await
        .unwrap()
        .outcome
        .unwrap();
    assert!(
        result.structured_content.unwrap()["groups"]
            .get("basic")
            .is_none()
    );
    let view = p.routes_view().await.unwrap();
    assert_eq!(
        view.targets
            .iter()
            .find(|x| x.owner_connection_id == Some(c.id))
            .unwrap()
            .actions
            .len(),
        2
    );
    assert_eq!(
        t.control_view().await.unwrap()["tools"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    drop(t);
    let t = ToolRuntime::build_with_secrets(config, TOKEN.into(), vec![])
        .await
        .unwrap();
    assert_eq!(
        t.service
            .list(&t.context(Default::default()), t.config.profile_id)
            .await
            .unwrap()
            .tools
            .len(),
        2
    );
    p.tick(&Default::default()).await.unwrap();
    // Editing a binding revokes it from every profile and clears observations, atomically.
    let mut next = c.clone();
    if let synquay_server::infrastructure::connectors::message_config::ConnectionConfig::Mqtt(m) =
        &mut next.config
    {
        m.capabilities.tools[0].payload.0.insert(
            "key".into(),
            synquay_server::domain::capability::binding::ValueBinding::Literal { value: json!(1) },
        );
    }
    p.change_connection(
        Uuid::from_u128(10),
        ConnectionInput {
            operation_id: Uuid::new_v4(),
            expected_revision: 1,
            connection_id: c.id,
            connection: Some(next),
            secret: None,
        },
    )
    .await
    .unwrap();
    t.refresh_device_tools().await.unwrap();
    assert!(
        t.service
            .list(&t.context(Default::default()), t.config.profile_id)
            .await
            .unwrap()
            .tools
            .is_empty()
    );
    assert_eq!(
        t.device_states(c.id).await.unwrap()["switch"]["groups"]["power"]["fields"]["on"]["freshness"],
        "unknown"
    );
    assert!(t.control_view().await.is_ok());
}
async fn packet(s: &mut tokio::net::TcpStream) -> (u8, Vec<u8>) {
    let h = s.read_u8().await.unwrap();
    let (mut len, mut shift) = (0, 0);
    loop {
        let b = s.read_u8().await.unwrap();
        len += ((b & 127) as usize) << shift;
        if b < 128 {
            break;
        }
        shift += 7;
    }
    let mut data = vec![0; len];
    s.read_exact(&mut data).await.unwrap();
    (h, data)
}
async fn publish(s: &mut tokio::net::TcpStream, value: Value, retain: bool) {
    let mut b = vec![0, 15];
    b.extend_from_slice(b"home/switch/pub"); // correct topic length below
    b[1] = b"home/switch/pub".len() as u8;
    b.extend_from_slice(&serde_json::to_vec(&value).unwrap());
    let mut head = vec![if retain { 0x31 } else { 0x30 }];
    let mut n = b.len();
    loop {
        let mut d = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            d |= 128
        }
        head.push(d);
        if n == 0 {
            break;
        }
    }
    head.extend(b);
    s.write_all(&head).await.unwrap();
}
#[tokio::test]
async fn mqtt_tool_subscribes_before_publish_correlates_ack_and_deduplicates() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (_dir, _p, t, _s, c, _) = setup(listener.local_addr().unwrap().port()).await;
    let server = expose(&t, &c).await;
    let broker = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        assert_eq!(packet(&mut s).await.0, 0x10);
        s.write_all(&[0x20, 2, 0, 0]).await.unwrap();
        let (h, b) = packet(&mut s).await;
        assert_eq!(h, 0x82);
        s.write_all(&[0x90, 3, b[0], b[1], 1]).await.unwrap();
        let (h, b) = packet(&mut s).await;
        assert_eq!(h, 0x32);
        let n = u16::from_be_bytes([b[0], b[1]]) as usize;
        let body: Value = serde_json::from_slice(&b[n + 4..]).unwrap();
        assert_eq!(body["key"], 0);
        s.write_all(&[0x40, 2, b[n + 2], b[n + 3]]).await.unwrap();
        publish(&mut s, json!({"messageId":"other","key":1}), false).await;
        publish(&mut s, json!({"messageId":body["messageId"],"key":1}), true).await;
        publish(
            &mut s,
            json!({"messageId":body["messageId"],"key":0}),
            false,
        )
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(2), s.read_u8()).await;
    });
    let id = Uuid::new_v4();
    for _ in 0..2 {
        let result = t
            .service
            .invoke(
                &t.context(Default::default()),
                PublishedToolCall {
                    profile_id: t.config.profile_id,
                    revision: server["tool_revision"].as_u64().unwrap(),
                    public_name: format!("mqtt_{}_off", c.id.simple()),
                    invocation_id: id,
                    arguments: json!({}),
                },
            )
            .await
            .unwrap()
            .outcome
            .unwrap();
        assert!(
            !result.is_error,
            "{}",
            serde_json::to_string(&result.structured_content).unwrap()
        );
        assert_eq!(result.structured_content.unwrap()["status"], "confirmed");
    }
    broker.await.unwrap();
    assert_eq!(
        t.device_states(c.id).await.unwrap()["switch"]["groups"]["power"]["fields"]["on"]["value"],
        false
    );
}

#[tokio::test]
async fn broker_acceptance_without_device_ack_does_not_change_actual_state() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (_dir, _p, t, s, c, _) = setup(listener.local_addr().unwrap().port()).await;
    report(&s, &c, json!({"key":1}), 0, false).await;
    let server = expose(&t, &c).await;
    let broker = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        packet(&mut s).await;
        s.write_all(&[0x20, 2, 0, 0]).await.unwrap();
        let (_, b) = packet(&mut s).await;
        s.write_all(&[0x90, 3, b[0], b[1], 1]).await.unwrap();
        let (_, b) = packet(&mut s).await;
        let n = u16::from_be_bytes([b[0], b[1]]) as usize;
        s.write_all(&[0x40, 2, b[n + 2], b[n + 3]]).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), s.read_u8()).await;
    });
    let result = t
        .service
        .invoke(
            &t.context(Default::default()),
            PublishedToolCall {
                profile_id: t.config.profile_id,
                revision: server["tool_revision"].as_u64().unwrap(),
                public_name: format!("mqtt_{}_off", c.id.simple()),
                invocation_id: Uuid::new_v4(),
                arguments: json!({}),
            },
        )
        .await
        .unwrap()
        .outcome
        .unwrap();
    assert!(result.is_error);
    assert_eq!(result.structured_content.unwrap()["status"], "unconfirmed");
    assert_eq!(
        t.device_states(c.id).await.unwrap()["switch"]["groups"]["power"]["fields"]["on"]["value"],
        true
    );
    broker.await.unwrap();
}

#[tokio::test]
async fn qq_command_routes_to_mqtt_action_with_original_conversation_reply() {
    let (_dir, p, _t, _s, c, _) = setup(1883).await;
    let qq:MessageConnection=serde_json::from_value(json!({"id":Uuid::new_v4(),"name":"QQ","config":{"kind":"qq_bot","app_id":"123456","group_openid":"","allowed_senders":[],"allow_proactive":false}})).unwrap();
    p.change_connection(
        Uuid::from_u128(10),
        ConnectionInput {
            operation_id: Uuid::new_v4(),
            expected_revision: 1,
            connection_id: qq.id,
            connection: Some(qq.clone()),
            secret: Some(TOKEN.into()),
        },
    )
    .await
    .unwrap();
    let view = p.routes_view().await.unwrap();
    let target = view
        .targets
        .iter()
        .find(|t| t.owner_connection_id == Some(c.id))
        .unwrap();
    let action = target
        .actions
        .iter()
        .find(|a| a.definition.id == "off")
        .unwrap();
    let id = Uuid::new_v4();
    let input=serde_json::from_value(json!({"operation_id":Uuid::new_v4(),"expected_revision":view.snapshot.revision,"route_id":id,"change":{"kind":"save","name":"QQ switch off","enabled":false,"source_id":qq.id,"event_type":null,"destination_ids":[target.destination_id],"response":{"kind":"source"},"processor":{"kind":"invoke_capability","tool_id":"off","revision":action.revision,"allowed_senders":["member_A"]},"command":{"text_pointer":"/content","trigger":"/off","mode":"exact","output":{"kind":"json","event_type":"command.invoke","template":{}}},"routing":{"source":{"adapter":"qq","version":1,"config":{"mode":"all_groups"}},"targets":{}}}})).unwrap();
    p.change_route(Uuid::from_u128(10), input).await.unwrap();
    assert!(
        p.routes_view()
            .await
            .unwrap()
            .snapshot
            .routes
            .iter()
            .any(|r| r.id == id)
    );
    let mut next = c.clone();
    if let synquay_server::infrastructure::connectors::message_config::ConnectionConfig::Mqtt(m) =
        &mut next.config
    {
        m.capabilities.tools.clear();
    }
    p.change_connection(
        Uuid::from_u128(10),
        ConnectionInput {
            operation_id: Uuid::new_v4(),
            expected_revision: 2,
            connection_id: c.id,
            connection: Some(next),
            secret: None,
        },
    )
    .await
    .unwrap();
    assert!(
        !p.routes_view()
            .await
            .unwrap()
            .snapshot
            .routes
            .iter()
            .any(|r| r.id == id)
    );
}
