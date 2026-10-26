mod support;
use serde_json::json;
use synquay_server::{
    domain::{connection_routing::RouteBinding, event::EventPayload},
    infrastructure::connectors::{message_config::*, qq::QqBotConfig, routing::adapter},
};
fn binding(adapter: &str, v: serde_json::Value) -> RouteBinding {
    RouteBinding::new(adapter, v)
}
fn mqtt() -> ConnectionConfig {
    ConnectionConfig::Mqtt(MqttConfig {
        host: "localhost".into(),
        port: 1883,
        username: None,
        password_env: None,
        tls: None,
        subscription: Some(MqttSubscription {
            include_metadata: false,
            topic: "home/+/state".into(),
            qos: 1,
            event_type: "mqtt.state".into(),
        }),
        publish: Some(MqttPublish {
            topic: "home/heater/set".into(),
            qos: 1,
            retain: false,
            body: BodyMode::Payload,
        }),
    })
}
fn qq() -> ConnectionConfig {
    ConnectionConfig::QqBot(QqBotConfig {
        resolved_secret: None,
        app_id: "123456".into(),
        group_openid: String::new(),
        allowed_senders: vec![],
        allow_proactive: false,
    })
}
#[test]
fn mqtt_matching_uses_transport_context_and_bounds_topic_filters() {
    let c = mqtt();
    let a = adapter(&c);
    let b = binding("mqtt", json!({"mode":"topic","filter":"home/heater/state"}));
    assert!(a.validate_source(Some(&b)).is_ok());
    assert!(!a.matches(Some(&b), None));
    assert!(a.matches(
        Some(&b),
        Some(&binding("mqtt", json!({"topic":"home/heater/state"})))
    ));
    assert!(!a.matches(
        Some(&b),
        Some(&binding("mqtt", json!({"topic":"home/lamp/state"})))
    ));
    for filter in ["#", "home/#", "home/+/#", "other/+/state"] {
        assert!(
            a.validate_source(Some(&binding(
                "mqtt",
                json!({"mode":"topic","filter":filter})
            )))
            .is_err()
        );
    }
    assert!(
        a.validate_target(
            Some(&binding(
                "mqtt",
                json!({"mode":"topic","topic":"home/lamp/set"})
            )),
            false
        )
        .is_ok()
    );
    for topic in ["home/+/set", "home/lamp/state", ""] {
        assert!(
            a.validate_target(
                Some(&binding("mqtt", json!({"mode":"topic","topic":topic}))),
                false
            )
            .is_err()
        );
    }
    let mut unknown = b.clone();
    unknown.version = 2;
    assert!(a.validate_source(Some(&unknown)).is_err());
    assert!(
        a.validate_source(Some(&binding("qq", json!({"mode":"all"}))))
            .is_err()
    );
}
#[test]
fn qq_scope_and_reply_are_bound_to_bot_and_authenticated_conversation() {
    let c = qq();
    let a = adapter(&c);
    let groups = binding("qq", json!({"mode":"groups","ids":["group_A","group_B"]}));
    assert!(a.validate_source(None).is_err());
    for kind in ["groups", "direct"] {
        assert!(
            a.validate_source(Some(&binding("qq", json!({"mode":kind,"ids":[]}))))
                .is_err()
        );
    }
    let ctx = binding(
        "qq",
        json!({"app_id":"123456","kind":"group","id":"group_B","msg_id":"signed-message"}),
    );
    assert!(a.matches(Some(&groups), Some(&ctx)));
    let mut event = support::event(support::scope());
    event.route_context = Some(ctx);
    // Content attempts to select another group or replace the passive reply identity.
    event.payload = EventPayload::Json(json!({"group_openid":"attack","msg_id":"forged"}));
    let reply = binding("qq", json!({"mode":"reply"}));
    let target = a
        .resolve_target(Some(&reply), &event, true)
        .unwrap()
        .unwrap();
    assert_eq!(target.config["id"], "group_B");
    assert_eq!(target.config["msg_id"], "signed-message");
    assert!(a.resolve_target(Some(&reply), &event, false).is_err());
    event.route_context.as_mut().unwrap().config["app_id"] = json!("other-bot");
    assert!(!a.matches(Some(&groups), event.route_context.as_ref()));
    assert!(a.resolve_target(Some(&reply), &event, true).is_err());
    event.route_context = None;
    assert!(a.resolve_target(Some(&reply), &event, true).is_err());
    // Cross-group forwarding cannot reuse the inbound group's passive reply grant.
    event.route_context = Some(binding(
        "qq",
        json!({"app_id":"123456","kind":"group","id":"group_B","msg_id":"signed-message"}),
    ));
    assert!(
        a.resolve_target(
            Some(&binding("qq", json!({"mode":"group","id":"group_C"}))),
            &event,
            true
        )
        .is_err()
    );
}
