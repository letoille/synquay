//! Protocol-independent webhook authentication and normalization policy.
use crate::application::ports::PortError;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookAuth {
    #[default]
    Bearer,
    HmacSha256,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebhookFormat {
    #[default]
    Envelope,
    Json {
        event_type: String,
        id_header: String,
    },
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookConfig {
    #[serde(default)]
    pub authentication: WebhookAuth,
    #[serde(default)]
    pub format: WebhookFormat,
}
impl WebhookConfig {
    pub fn validate(&self) -> Result<(), PortError> {
        if let WebhookFormat::Json {
            event_type,
            id_header,
        } = &self.format
            && (!valid_event_id(event_type, 128)
                || id_header.len() > 64
                || !id_header.starts_with("x-")
                || !id_header
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                || ["x-synquay-timestamp", "x-synquay-signature"].contains(&id_header.as_str()))
        {
            return Err(PortError::InvalidInput {
                code: "invalid_webhook_format",
            });
        }
        Ok(())
    }
}
pub fn valid_event_id(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
pub fn fresh_timestamp(value: &str, now: SystemTime) -> bool {
    if value.is_empty() || value.len() > 12 || !value.bytes().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(seconds) = value.parse::<u64>() else {
        return false;
    };
    value == seconds.to_string()
        && now
            .duration_since(UNIX_EPOCH)
            .is_ok_and(|t| t.as_secs().abs_diff(seconds) <= 300)
}
/// Raw wire bytes are authenticated, including the configured source and raw-JSON delivery ID.
/// Never decode/re-encode JSON before verification. The caller limits body and header sizes.
pub fn verify_signature(
    secret: &[u8],
    source: Uuid,
    timestamp: &str,
    delivery_id: &str,
    signature: &str,
    body: &[u8],
    now: SystemTime,
) -> bool {
    if !fresh_timestamp(timestamp, now) {
        return false;
    }
    let Some(hex) = signature.strip_prefix("v1=").filter(|v| v.len() == 64) else {
        return false;
    };
    let mut expected = [0u8; 32];
    for (i, pair) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let nibble = |b: u8| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        let (Some(a), Some(b)) = (nibble(pair[0]), nibble(pair[1])) else {
            return false;
        };
        expected[i] = (a << 4) | b;
    }
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let mut ctx = hmac::Context::with_key(&key);
    ctx.update(format!("v1\n{source}\n{timestamp}\n{delivery_id}\n").as_bytes());
    ctx.update(body);
    use subtle::ConstantTimeEq;
    bool::from(ctx.sign().as_ref().ct_eq(&expected))
}
