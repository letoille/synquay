//! MQTT 3.1.1, bounded packets, manual inbound PUBACK and explicit outbound acceptance.
use super::{http_client::body, message_config::*};
use crate::{
    application::ports::{
        OperationContext, PortError, PortFuture,
        events::{EventIngress, EventSource, SourceExit},
        messages::MessageSink,
    },
    domain::{
        connector::ConnectionScope,
        event::{EventPayload, IncomingEvent},
        message::*,
    },
};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, TlsConfiguration, Transport};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

pub struct MqttConnector {
    options: MqttOptions,
    config: MqttConfig,
    scope: ConnectionScope,
    destination: Uuid,
}
impl MqttConnector {
    pub fn new(
        scope: ConnectionScope,
        destination: Uuid,
        c: &MqttConfig,
    ) -> Result<Self, PortError> {
        Self::new_with_resolver(scope, destination, c, &secret)
    }
    fn new_with_resolver(
        scope: ConnectionScope,
        destination: Uuid,
        c: &MqttConfig,
        resolve: &dyn Fn(&str) -> Result<String, PortError>,
    ) -> Result<Self, PortError> {
        c.validate()?;
        let mut options = MqttOptions::new(
            format!("sq-{}", scope.connection_id.simple()),
            &c.host,
            c.port,
        );
        options
            .set_keep_alive(Duration::from_secs(60))
            .set_max_packet_size(65536 + 1024, 65536 + 1024)
            .set_inflight(1)
            .set_manual_acks(true);
        if let Some(username) = &c.username {
            let password = c
                .password_env
                .as_deref()
                .map(resolve)
                .transpose()?
                .unwrap_or_default();
            if password.len() > 4096 {
                return Err(invalid("invalid_mqtt_password"));
            }
            options.set_credentials(username, password);
        }
        if let Some(tls) = &c.tls {
            let mut roots = rustls::RootCertStore::empty();
            if let Some(env) = &tls.ca_env {
                let pem = resolve(env)?;
                let certs = rustls_pemfile::certs(&mut pem.as_bytes())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| invalid("invalid_mqtt_ca"))?;
                if certs.is_empty() {
                    return Err(invalid("invalid_mqtt_ca"));
                }
                for c in certs {
                    roots.add(c).map_err(|_| invalid("invalid_mqtt_ca"))?;
                }
            } else {
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            }
            let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| invalid("mqtt_tls_versions"))?
            .with_root_certificates(roots);
            let client =
                if let (Some(cert), Some(key)) = (&tls.client_cert_env, &tls.client_key_env) {
                    let cert = resolve(cert)?;
                    let key = resolve(key)?;
                    let certs = rustls_pemfile::certs(&mut cert.as_bytes())
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| invalid("invalid_mqtt_certificate"))?;
                    let key = rustls_pemfile::private_key(&mut key.as_bytes())
                        .map_err(|_| invalid("invalid_mqtt_key"))?
                        .ok_or_else(|| invalid("invalid_mqtt_key"))?;
                    builder
                        .with_client_auth_cert(certs, key)
                        .map_err(|_| invalid("invalid_mqtt_certificate"))?
                } else {
                    builder.with_no_client_auth()
                };
            options.set_transport(Transport::tls_with_config(TlsConfiguration::Rustls(
                Arc::new(client),
            )));
        }
        Ok(Self {
            options,
            config: c.clone(),
            scope,
            destination,
        })
    }
}
fn qos(q: u8) -> QoS {
    if q == 0 {
        QoS::AtMostOnce
    } else {
        QoS::AtLeastOnce
    }
}
impl MessageSink for MqttConnector {
    fn validate(&self, m: &Message) -> Result<(), PortError> {
        let p = self.config.publish.as_ref().ok_or(PortError::Unsupported)?;
        self.publish_topic(m)?;
        body(m, p.body).map(|_| ())
    }
    fn deliver(
        &self,
        ctx: OperationContext,
        r: DeliveryRequest,
    ) -> PortFuture<'_, DeliveryReceipt> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != self.scope || r.destination_id != self.destination {
                return Err(PortError::ScopeMismatch);
            }
            let p = self.config.publish.as_ref().ok_or(PortError::Unsupported)?;
            let topic = self.publish_topic(&r.message)?;
            let bytes = body(&r.message, p.body)?;
            // Each delivery has a fresh clean session: unconfirmed publications are never
            // silently replayed by the MQTT library after we return OutcomeUnknown.
            let mut options = self.options.clone();
            options.set_clean_session(true);
            let (client, mut events) = AsyncClient::new(options, 4);
            events.network_options.set_connection_timeout(5);
            let connected = tokio::time::timeout(Duration::from_secs(8), events.poll());
            match tokio::select! {biased;_=ctx.stopped()=>return Err(PortError::Unavailable),r=connected=>r}
            {
                Ok(Ok(Event::Incoming(Packet::ConnAck(a))))
                    if a.code == rumqttc::ConnectReturnCode::Success => {}
                _ => return Err(PortError::Unavailable),
            }
            client
                .try_publish(topic, qos(p.qos), p.retain, bytes)
                .map_err(|_| PortError::Unavailable)?;
            let attempt = async {
                loop {
                    match events.poll().await {
                        Ok(Event::Incoming(Packet::PubAck(_))) if p.qos == 1 => return Ok(()),
                        Ok(Event::Outgoing(rumqttc::Outgoing::Publish(_))) if p.qos == 0 => {
                            return Ok(());
                        }
                        Ok(_) => {}
                        Err(_) => return Err(PortError::OutcomeUnknown),
                    }
                }
            };
            tokio::select! {biased;_=ctx.stopped()=>return Err(PortError::OutcomeUnknown),r=tokio::time::timeout(Duration::from_secs(15),attempt)=>r.map_err(|_|PortError::OutcomeUnknown)??};
            Ok(DeliveryReceipt {
                delivery_id: r.delivery_id,
                external_message_id: None,
                status: DeliveryStatus::Accepted,
            })
        })
    }
}
impl EventSource for MqttConnector {
    fn run(
        &self,
        ctx: OperationContext,
        ingress: Arc<dyn EventIngress>,
    ) -> PortFuture<'_, SourceExit> {
        Box::pin(async move {
            ctx.check_active()?;
            if ctx.scope != self.scope {
                return Err(PortError::ScopeMismatch);
            }
            let s = self
                .config
                .subscription
                .as_ref()
                .ok_or(PortError::Unsupported)?;
            let mut options = self.options.clone();
            options.set_clean_session(false);
            let mut delay = 1;
            loop {
                if ctx.cancellation.is_cancelled() {
                    return Ok(SourceExit::Cancelled);
                }
                let (client, mut events) = AsyncClient::new(options.clone(), 8);
                events.network_options.set_connection_timeout(5);
                loop {
                    let event = tokio::select! {biased;_=ctx.cancellation.cancelled()=>return Ok(SourceExit::Cancelled),e=events.poll()=>e};
                    match event {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            client
                                .try_subscribe(&s.topic, qos(s.qos))
                                .map_err(|_| PortError::Unavailable)?;
                        }
                        Ok(Event::Incoming(Packet::SubAck(ack))) => {
                            if ack
                                .return_codes
                                .iter()
                                .any(|q| matches!(q, rumqttc::SubscribeReasonCode::Failure))
                            {
                                tracing::warn!(event="mqtt.subscription_rejected",connection=%self.scope.connection_id);
                                break;
                            }
                            delay = 1;
                        }
                        Ok(Event::Incoming(Packet::Publish(p))) => {
                            if p.payload.len() > 65536 {
                                break;
                            }
                            // JSON is preserved; binary input is explicitly encoded, never lossy UTF-8.
                            let payload=serde_json::from_slice(&p.payload).unwrap_or_else(|_|{use base64::Engine;serde_json::json!({"encoding":"base64","data":base64::engine::general_purpose::STANDARD.encode(&p.payload)})});
                            let payload = if s.include_metadata {
                                serde_json::json!({"topic":p.topic,"qos":p.qos as u8,"retain":p.retain,"payload":payload})
                            } else {
                                payload
                            };
                            let input = IncomingEvent {
                                route_context: Some(
                                    crate::domain::connection_routing::RouteBinding::new(
                                        "mqtt",
                                        serde_json::json!({"topic":p.topic}),
                                    ),
                                ),
                                event_type: s.event_type.clone(),
                                schema_version: 1,
                                occurred_at: None,
                                source_event_id: None,
                                payload: EventPayload::Json(payload),
                            };
                            let mut operation = ctx.clone();
                            operation.request_id = Uuid::new_v4();
                            operation.correlation_id = operation.request_id;
                            operation.deadline =
                                Some(std::time::Instant::now() + Duration::from_secs(20));
                            let accepted = tokio::select! {biased;_=ctx.cancellation.cancelled()=>return Ok(SourceExit::Cancelled),r=ingress.accept(operation,input)=>r};
                            if accepted.is_err() {
                                tracing::warn!(event="mqtt.ingress_unconfirmed",connection=%self.scope.connection_id);
                                break;
                            }
                            // try_ack avoids deadlocking the event loop on its own request queue.
                            if client.try_ack(&p).is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => {
                            tracing::warn!(event="mqtt.disconnected",connection=%self.scope.connection_id);
                            break;
                        }
                    }
                }
                tokio::select! {biased;_=ctx.cancellation.cancelled()=>return Ok(SourceExit::Cancelled),_=tokio::time::sleep(Duration::from_secs(delay))=>{}}
                delay = (delay * 2).min(30);
            }
        })
    }
}

#[cfg(test)]
mod tests;

impl MqttConnector {
    fn publish_topic<'a>(&'a self, m: &'a Message) -> Result<&'a str, PortError> {
        let config = ConnectionConfig::Mqtt(self.config.clone());
        let adapter = super::routing::adapter(&config);
        // Validate the durable routing binding again before any network side effect.
        adapter.validate_target(m.destination_binding.as_ref(), false)?;
        if let Some(b) = &m.destination_binding
            && b.config["mode"] == "topic"
        {
            return b.config["topic"]
                .as_str()
                .ok_or(invalid("invalid_mqtt_publish"));
        }
        Ok(&self
            .config
            .publish
            .as_ref()
            .ok_or(PortError::Unsupported)?
            .topic)
    }
}
