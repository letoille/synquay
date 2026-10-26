//! Operator-owned connection definitions. Secrets are environment references, never event data.
use crate::application::ports::PortError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageConnection {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub enabled: bool,
    pub config: ConnectionConfig,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionConfig {
    Webhook {
        secret_env: String,
        #[serde(default)]
        policy: crate::infrastructure::pipeline::webhook::WebhookConfig,
    },
    HttpClient(HttpClientConfig),
    Mqtt(MqttConfig),
    QqBot(super::qq::QqBotConfig),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpClientConfig {
    pub url: String,
    #[serde(default = "post")]
    pub method: String,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default)]
    pub auth: HttpAuth,
    #[serde(default)]
    pub body: BodyMode,
}
fn post() -> String {
    "POST".into()
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpAuth {
    #[default]
    None,
    Bearer {
        token_env: String,
    },
    Basic {
        username: String,
        password_env: String,
    },
}
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyMode {
    #[default]
    Payload,
    Event,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password_env: Option<String>,
    #[serde(default)]
    pub tls: Option<MqttTls>,
    #[serde(default)]
    pub subscription: Option<MqttSubscription>,
    #[serde(default)]
    pub publish: Option<MqttPublish>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttTls {
    #[serde(default)]
    pub ca_env: Option<String>,
    #[serde(default)]
    pub client_cert_env: Option<String>,
    #[serde(default)]
    pub client_key_env: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttSubscription {
    #[serde(default)]
    pub include_metadata: bool,
    pub topic: String,
    pub qos: u8,
    pub event_type: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttPublish {
    pub topic: String,
    pub qos: u8,
    #[serde(default)]
    pub retain: bool,
    #[serde(default)]
    pub body: BodyMode,
}

pub fn invalid(code: &'static str) -> PortError {
    PortError::InvalidInput { code }
}
pub fn secret(name: &str) -> Result<String, PortError> {
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || name.as_bytes()[0].is_ascii_digit()
    {
        return Err(invalid("invalid_secret_reference"));
    }
    let value = std::env::var(name).map_err(|_| invalid("missing_connection_secret"))?;
    if value.is_empty() || value.len() > 128 * 1024 {
        return Err(invalid("invalid_connection_secret"));
    }
    Ok(value)
}
impl MessageConnection {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.id.is_nil()
            || self.send_id().is_nil()
            || self.name.trim().is_empty()
            || self.name.len() > 120
            || self.name.chars().any(char::is_control)
            || self.description.len() > 4000
        {
            return Err(invalid("invalid_connection"));
        }
        match &self.config {
            ConnectionConfig::Webhook { secret_env, policy } => {
                policy.validate()?;
                let s = secret(secret_env)?;
                if s.len() < 32 || s.len() > 4096 || !s.bytes().all(|b| b.is_ascii_graphic()) {
                    return Err(invalid("invalid_webhook_secret"));
                }
            }
            ConnectionConfig::Mqtt(c) => c.validate()?,
            ConnectionConfig::HttpClient(_) => {}
            ConnectionConfig::QqBot(c) => c.validate()?,
        }
        Ok(())
    }
    pub fn receives(&self) -> bool {
        matches!(&self.config, ConnectionConfig::Webhook { .. })
            || matches!(&self.config, ConnectionConfig::QqBot(_))
            || matches!(&self.config,ConnectionConfig::Mqtt(c) if c.subscription.is_some())
    }
    pub fn sends(&self) -> bool {
        matches!(&self.config, ConnectionConfig::HttpClient(_))
            || matches!(&self.config, ConnectionConfig::QqBot(_))
            || matches!(&self.config,ConnectionConfig::Mqtt(c) if c.publish.is_some())
    }
    // Distinct endpoint scopes allow one MQTT connection to receive and send without
    // confusing endpoint loop detection. UUID v5 is not required: use a fixed bijection.
    pub fn send_id(&self) -> Uuid {
        Uuid::from_u128(self.id.as_u128() ^ (1u128 << 127))
    }
    pub fn protocol(&self) -> &'static str {
        match self.config {
            ConnectionConfig::Webhook { .. } => "Webhook",
            ConnectionConfig::HttpClient(_) => "HTTP",
            ConnectionConfig::Mqtt(_) => "MQTT",
            ConnectionConfig::QqBot(_) => "QQ",
        }
    }
    pub fn address(&self) -> String {
        match &self.config {
            ConnectionConfig::Webhook { .. } => format!("/api/webhooks/{}", self.id),
            ConnectionConfig::HttpClient(c) => c.url.clone(),
            ConnectionConfig::QqBot(_) => format!("/api/qq/{}/webhook", self.id),
            ConnectionConfig::Mqtt(c) => format!(
                "{}://{}:{}",
                if c.tls.is_some() { "mqtts" } else { "mqtt" },
                if c.host.contains(':') {
                    format!("[{}]", c.host)
                } else {
                    c.host.clone()
                },
                c.port
            ),
        }
    }
}
impl MqttConfig {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.host.is_empty()
            || self.host.len() > 253
            || self
                .host
                .chars()
                .any(|c| c.is_whitespace() || "/@?#".contains(c))
            || (self.host.contains(':') && self.host.parse::<std::net::IpAddr>().is_err())
            || self.port == 0
            || (self.subscription.is_none() && self.publish.is_none())
            || self.password_env.is_some() && self.username.is_none()
            || self
                .username
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 256 || s.contains('\0'))
        {
            return Err(invalid("invalid_mqtt_config"));
        }
        if let Some(s) = &self.subscription
            && (!valid_topic(&s.topic, true)
                || s.qos > 1
                || !crate::infrastructure::pipeline::webhook::valid_event_id(&s.event_type, 128))
        {
            return Err(invalid("invalid_mqtt_subscription"));
        }
        if let Some(p) = &self.publish
            && (!valid_topic(&p.topic, false) || p.qos > 1)
        {
            return Err(invalid("invalid_mqtt_publish"));
        }
        // Reject a local self-feeding route at configuration time as a conservative guard.
        if let (Some(s), Some(p)) = (&self.subscription, &self.publish)
            && rumqttc::matches(&p.topic, &s.topic)
        {
            return Err(invalid("mqtt_topic_loop"));
        }
        if let Some(t) = &self.tls
            && t.client_cert_env.is_some() != t.client_key_env.is_some()
        {
            return Err(invalid("mqtt_client_certificate_pair"));
        }
        Ok(())
    }
}
pub(crate) fn valid_topic(s: &str, filter: bool) -> bool {
    !s.is_empty()
        && s.len() <= 512
        && !s.contains('\0')
        && if filter {
            rumqttc::valid_filter(s)
        } else {
            !s.contains(['#', '+'])
        }
}
