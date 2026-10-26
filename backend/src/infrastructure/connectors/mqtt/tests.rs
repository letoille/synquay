use super::*;
use crate::domain::event::IngestDisposition;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

async fn packet<S: AsyncRead + Unpin>(s: &mut S) -> (u8, Vec<u8>) {
    let head = s.read_u8().await.unwrap();
    let mut n = 0;
    let mut shift = 0;
    loop {
        let b = s.read_u8().await.unwrap();
        n += ((b & 127) as usize) << shift;
        if b < 128 {
            break;
        }
        shift += 7;
        assert!(shift < 28);
    }
    assert!(n < 70000);
    let mut data = vec![0; n];
    s.read_exact(&mut data).await.unwrap();
    (head, data)
}
async fn broker_send<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S, ack: bool) {
    assert_eq!(packet(s).await.0, 0x10);
    s.write_all(&[0x20, 2, 0, 0]).await.unwrap();
    let (head, p) = packet(s).await;
    assert_eq!(head >> 4, 3);
    let len = u16::from_be_bytes([p[0], p[1]]) as usize;
    assert_eq!(&p[2..2 + len], b"home/heater/set");
    let id = &p[2 + len..4 + len];
    assert_eq!(&p[4 + len..], br#"{"on":true}"#);
    if ack {
        s.write_all(&[0x40, 2, id[0], id[1]]).await.unwrap();
        let mut byte = [0];
        let _ = tokio::time::timeout(Duration::from_secs(3), s.read(&mut byte)).await;
    }
}
fn cfg(port: u16) -> MqttConfig {
    MqttConfig {
        host: "localhost".into(),
        port,
        username: None,
        password_env: None,
        tls: None,
        subscription: None,
        publish: Some(MqttPublish {
            topic: "home/heater/set".into(),
            qos: 1,
            retain: false,
            body: BodyMode::Payload,
        }),
    }
}
fn context() -> OperationContext {
    OperationContext {
        scope: ConnectionScope {
            workspace_id: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
        },
        principal_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        correlation_id: Uuid::new_v4(),
        causation_id: None,
        route_trace: vec![],
        deadline: Some(std::time::Instant::now() + Duration::from_secs(5)),
        cancellation: Default::default(),
    }
}
fn request(id: Uuid) -> DeliveryRequest {
    DeliveryRequest {
        delivery_id: Uuid::new_v4(),
        destination_id: id,
        message: Message {
            destination_binding: None,
            parts: vec![ContentPart::Json(
                serde_json::json!({"payload":{"on":true}}),
            )],
            reply_route_id: None,
        },
    }
}
#[tokio::test]
async fn tcp_publish_waits_for_puback_and_disconnect_is_uncertain() {
    for ack in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            broker_send(&mut socket, ack).await;
        });
        let ctx = context();
        let id = Uuid::new_v4();
        let driver = MqttConnector::new(ctx.scope, id, &cfg(port)).unwrap();
        let result = driver.deliver(ctx, request(id)).await;
        if ack {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(PortError::OutcomeUnknown)));
        }
        task.await.unwrap();
    }
}
#[tokio::test]
async fn tls_trusted_ca_works_and_untrusted_ca_is_rejected() {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let pem = certified.cert.pem();
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![certified.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()).into(),
    )
    .unwrap();
    for (trusted, correct_host) in [(true, true), (false, true), (true, false)] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server.clone()));
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            match acceptor.accept(socket).await {
                Ok(mut tls) => {
                    assert!(trusted && correct_host);
                    broker_send(&mut tls, true).await;
                }
                Err(_) => assert!(!trusted || !correct_host),
            }
        });
        let mut c = cfg(port);
        if !correct_host {
            c.host = "127.0.0.1".into();
        }
        c.tls = Some(MqttTls {
            ca_env: trusted.then(|| "CA".into()),
            ..Default::default()
        });
        let ctx = context();
        let id = Uuid::new_v4();
        let driver =
            MqttConnector::new_with_resolver(ctx.scope, id, &c, &|_| Ok(pem.clone())).unwrap();
        let result = driver.deliver(ctx, request(id)).await;
        assert_eq!(result.is_ok(), trusted && correct_host);
        task.await.unwrap();
    }
}
struct DelayedIngress {
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl EventIngress for DelayedIngress {
    fn accept(
        &self,
        _: OperationContext,
        input: IncomingEvent,
    ) -> PortFuture<'_, crate::domain::event::IngestReceipt> {
        Box::pin(async move {
            assert_eq!(input.event_type, "device.status");
            assert_eq!(
                input.route_context.as_ref().unwrap().config["topic"],
                "home/heater/state"
            );
            assert!(
                matches!(input.payload,EventPayload::Json(ref p) if p==&serde_json::json!({"on":true}))
            );
            self.started.notify_one();
            self.release.notified().await;
            Ok(crate::domain::event::IngestReceipt {
                event_id: Uuid::new_v4(),
                disposition: IngestDisposition::Stored,
            })
        })
    }
}
#[tokio::test]
async fn subscription_puback_follows_durable_ingress_and_cancel_stops_worker() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ingress = Arc::new(DelayedIngress {
        started: Default::default(),
        release: Default::default(),
    });
    let check = ingress.clone();
    let broker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(packet(&mut socket).await.0, 0x10);
        socket.write_all(&[0x20, 2, 0, 0]).await.unwrap();
        let (head, sub) = packet(&mut socket).await;
        assert_eq!(head, 0x82);
        socket
            .write_all(&[0x90, 3, sub[0], sub[1], 1])
            .await
            .unwrap();
        let topic = b"home/heater/state";
        let body = br#"{"on":true}"#;
        let mut publish = vec![
            0x32,
            (2 + topic.len() + 2 + body.len()) as u8,
            0,
            topic.len() as u8,
        ];
        publish.extend_from_slice(topic);
        publish.extend_from_slice(&[0, 42]);
        publish.extend_from_slice(body);
        socket.write_all(&publish).await.unwrap();
        check.started.notified().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(80), socket.read_u8())
                .await
                .is_err(),
            "PUBACK before durable ingest"
        );
        check.release.notify_one();
        assert_eq!(packet(&mut socket).await, (0x40, vec![0, 42]));
    });
    let mut c = cfg(port);
    c.subscription = Some(MqttSubscription {
        include_metadata: false,
        topic: "home/+/state".into(),
        qos: 1,
        event_type: "device.status".into(),
    });
    c.publish = None;
    let mut ctx = context();
    ctx.deadline = None;
    let stop = ctx.cancellation.clone();
    let driver = MqttConnector::new(ctx.scope, Uuid::new_v4(), &c).unwrap();
    let run = tokio::spawn(async move { driver.run(ctx, ingress).await });
    tokio::time::timeout(Duration::from_secs(5), broker)
        .await
        .unwrap()
        .unwrap();
    stop.cancel();
    assert_eq!(run.await.unwrap().unwrap(), SourceExit::Cancelled);
}
#[test]
fn mqtt_validation_rejects_wildcard_publish_loop_and_invalid_tls_pair() {
    let mut c = cfg(1883);
    c.publish.as_mut().unwrap().topic = "home/+".into();
    assert!(c.validate().is_err());
    c.publish.as_mut().unwrap().topic = "home/heater/set".into();
    c.subscription = Some(MqttSubscription {
        include_metadata: false,
        topic: "home/#".into(),
        qos: 1,
        event_type: "event".into(),
    });
    assert!(c.validate().is_err());
    c.subscription = None;
    c.tls = Some(MqttTls {
        client_cert_env: Some("CERT".into()),
        ..Default::default()
    });
    assert!(c.validate().is_err());
}

#[tokio::test]
async fn routing_binding_overrides_default_topic_but_not_from_payload() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        broker_send(&mut socket, true).await;
    });
    let ctx = context();
    let id = Uuid::new_v4();
    let mut config = cfg(port);
    config.publish.as_mut().unwrap().topic = "default/topic".into();
    let driver = MqttConnector::new(ctx.scope, id, &config).unwrap();
    let mut req = request(id);
    req.message.destination_binding = Some(crate::domain::connection_routing::RouteBinding::new(
        "mqtt",
        serde_json::json!({"mode":"topic","topic":"home/heater/set"}),
    ));
    driver.deliver(ctx, req).await.unwrap();
    task.await.unwrap();
    let mut bad = request(id);
    bad.message.destination_binding = Some(crate::domain::connection_routing::RouteBinding::new(
        "mqtt",
        serde_json::json!({"mode":"topic","topic":"home/+/set"}),
    ));
    assert!(driver.validate(&bad.message).is_err());
}
