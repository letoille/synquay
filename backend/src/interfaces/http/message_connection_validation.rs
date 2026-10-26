//! Safe field metadata only. Never return rejected values or deserializer diagnostics.
use crate::infrastructure::pipeline::webhook::{WebhookFormat, valid_event_id};
use crate::{application::ports::PortError, infrastructure::connectors::message_config::*};
use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
pub(super) fn response(code: &'static str, connection: Option<&MessageConnection>) -> Response {
    let mut fields = Vec::new();
    if let Some((path, reason)) = field(code, connection) {
        fields.push(serde_json::json!({"path":path,"code":reason}));
    }
    let mut response = (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"code":code,"fields":fields})),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}
fn field(
    code: &'static str,
    connection: Option<&MessageConnection>,
) -> Option<(&'static str, &'static str)> {
    let c = connection?;
    let path = match code {
        "qq_secret_required" | "invalid_qq_secret" => "secret",
        "duplicate_qq_app" => "config.app_id",
        "invalid_connection" => {
            if c.name.trim().is_empty()
                || c.name.len() > 120
                || c.name.chars().any(char::is_control)
            {
                return Some(("name", "invalid_connection_name"));
            }
            if c.description.len() > 4000 {
                return Some(("description", "invalid_connection_description"));
            }
            return None;
        }
        "invalid_qq_config" => {
            let ConnectionConfig::QqBot(q) = &c.config else {
                return None;
            };
            if q.app_id.is_empty()
                || q.app_id.len() > 32
                || !q.app_id.bytes().all(|b| b.is_ascii_digit())
            {
                return Some(("config.app_id", "invalid_qq_app_id"));
            }
            if !q.group_openid.is_empty()
                && !crate::infrastructure::connectors::qq::openid(&q.group_openid)
            {
                return Some(("config.group_openid", "invalid_qq_group"));
            }
            return Some(("config.allowed_senders", "invalid_qq_members"));
        }
        "invalid_http_url" => "config.url",
        "invalid_http_method" => "config.method",
        "invalid_http_username" => "config.auth.username",
        "invalid_http_auth" => match &c.config {
            ConnectionConfig::HttpClient(h) => match h.auth {
                HttpAuth::Bearer { .. } => "config.auth.token_env",
                HttpAuth::Basic { .. } => "config.auth.password_env",
                _ => return None,
            },
            _ => return None,
        },
        "invalid_webhook_secret" => "config.secret_env",
        "invalid_webhook_format" => {
            let ConnectionConfig::Webhook { policy, .. } = &c.config else {
                return None;
            };
            let WebhookFormat::Json { event_type, .. } = &policy.format else {
                return None;
            };
            return Some(if !valid_event_id(event_type, 128) {
                ("config.policy.format.event_type", "invalid_event_type")
            } else {
                ("config.policy.format.id_header", "invalid_event_header")
            });
        }
        "invalid_mqtt_config" => {
            let ConnectionConfig::Mqtt(m) = &c.config else {
                return None;
            };
            if m.host.is_empty()
                || m.host.len() > 253
                || m.host
                    .chars()
                    .any(|c| c.is_whitespace() || "/@?#".contains(c))
                || (m.host.contains(':') && m.host.parse::<std::net::IpAddr>().is_err())
            {
                return Some(("config.host", "invalid_mqtt_host"));
            }
            if m.port == 0 {
                return Some(("config.port", "invalid_mqtt_port"));
            }
            if m.subscription.is_none() && m.publish.is_none() {
                return Some(("config.subscription", "mqtt_receive_or_send_required"));
            }
            if m.password_env.is_some() && m.username.is_none() {
                return Some(("config.username", "mqtt_username_required"));
            }
            return Some(("config.username", "invalid_mqtt_username"));
        }
        "invalid_mqtt_subscription" => {
            let ConnectionConfig::Mqtt(m) = &c.config else {
                return None;
            };
            let s = m.subscription.as_ref()?;
            return Some(if !valid_topic(&s.topic, true) {
                ("config.subscription.topic", "invalid_subscription_topic")
            } else if s.qos > 1 {
                ("config.subscription.qos", "invalid_qos")
            } else {
                ("config.subscription.event_type", "invalid_event_type")
            });
        }
        "invalid_mqtt_publish" => {
            let ConnectionConfig::Mqtt(m) = &c.config else {
                return None;
            };
            let p = m.publish.as_ref()?;
            return Some(if !valid_topic(&p.topic, false) {
                ("config.publish.topic", "invalid_publish_topic")
            } else {
                ("config.publish.qos", "invalid_qos")
            });
        }
        "mqtt_topic_loop" => "config.publish.topic",
        "invalid_mqtt_password" => "config.password_env",
        "invalid_mqtt_ca" => "config.tls.ca_env",
        "invalid_mqtt_certificate" => "config.tls.client_cert_env",
        "invalid_mqtt_key" => "config.tls.client_key_env",
        "mqtt_client_certificate_pair" => {
            let ConnectionConfig::Mqtt(m) = &c.config else {
                return None;
            };
            if m.tls.as_ref()?.client_key_env.is_none() {
                "config.tls.client_key_env"
            } else {
                "config.tls.client_cert_env"
            }
        }
        "invalid_secret_reference" | "missing_connection_secret" | "invalid_connection_secret" => {
            let mut references = Vec::new();
            match &c.config {
                ConnectionConfig::Webhook { secret_env, .. } => {
                    references.push(("config.secret_env", secret_env.as_str()))
                }
                ConnectionConfig::HttpClient(h) => match &h.auth {
                    HttpAuth::Bearer { token_env } => {
                        references.push(("config.auth.token_env", token_env.as_str()))
                    }
                    HttpAuth::Basic { password_env, .. } => {
                        references.push(("config.auth.password_env", password_env.as_str()))
                    }
                    _ => {}
                },
                ConnectionConfig::Mqtt(m) => {
                    if let Some(name) = &m.password_env {
                        references.push(("config.password_env", name.as_str()));
                    }
                    if let Some(t) = &m.tls {
                        for (field, value) in [
                            ("config.tls.ca_env", &t.ca_env),
                            ("config.tls.client_cert_env", &t.client_cert_env),
                            ("config.tls.client_key_env", &t.client_key_env),
                        ] {
                            if let Some(value) = value {
                                references.push((field, value.as_str()));
                            }
                        }
                    }
                }
                _ => {}
            }
            return references
                .into_iter()
                .find_map(|(path, name)| match secret(name) {
                    Err(PortError::InvalidInput { code: reason }) if reason == code => {
                        Some((path, code))
                    }
                    _ => None,
                });
        }
        _ => return None,
    };
    Some((path, code))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qq_error_points_to_one_field_without_echoing_values() {
        let mut c:MessageConnection=serde_json::from_value(serde_json::json!({"id":uuid::Uuid::new_v4(),"name":"Bot","enabled":false,"config":{"kind":"qq_bot","app_id":"abc","group_openid":"bad space","allowed_senders":["bad space"]}})).unwrap();
        assert_eq!(
            field("invalid_qq_config", Some(&c)),
            Some(("config.app_id", "invalid_qq_app_id"))
        );
        if let ConnectionConfig::QqBot(q) = &mut c.config {
            q.app_id = "123456".into();
        }
        assert_eq!(
            field("invalid_qq_config", Some(&c)),
            Some(("config.group_openid", "invalid_qq_group"))
        );
        if let ConnectionConfig::QqBot(q) = &mut c.config {
            q.group_openid.clear();
        }
        assert_eq!(
            field("invalid_qq_config", Some(&c)),
            Some(("config.allowed_senders", "invalid_qq_members"))
        );
        assert_eq!(field("origin_denied", Some(&c)), None);
        assert_eq!(
            field("qq_secret_required", Some(&c)),
            Some(("secret", "qq_secret_required"))
        );
    }
}
