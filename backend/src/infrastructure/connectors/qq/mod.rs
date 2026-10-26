//! QQ official group adapter. Credentials and destination scope are operator-owned.
mod inbound;
mod sink;
use super::message_config::invalid;
use crate::application::ports::PortError;
pub use inbound::{Callback, callback};
use serde::{Deserialize, Serialize};
pub use sink::QqBotSink;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QqBotConfig {
    pub app_id: String,
    /// Runtime only: never accepted from or serialized to configuration JSON.
    #[serde(skip)]
    pub resolved_secret: Option<std::sync::Arc<str>>,
    /// Legacy fixed-group restriction. Empty means bot-level; routes must select scope explicitly.
    #[serde(default)]
    pub group_openid: String,
    #[serde(default)]
    pub allowed_senders: Vec<String>,
    #[serde(default)]
    pub allow_proactive: bool,
}
impl QqBotConfig {
    pub fn secret(&self) -> Result<String, PortError> {
        self.resolved_secret
            .as_ref()
            .map(|s| s.to_string())
            .ok_or(invalid("qq_secret_required"))
    }
    pub fn validate(&self) -> Result<(), PortError> {
        if self.app_id.is_empty()
            || self.app_id.len() > 32
            || !self.app_id.bytes().all(|c| c.is_ascii_digit())
            || (!self.group_openid.is_empty() && !openid(&self.group_openid))
            || self.allowed_senders.len() > 100
            || self.allowed_senders.iter().any(|s| !openid(s))
        {
            return Err(invalid("invalid_qq_config"));
        }
        let s = self.secret()?;
        if s.is_empty() || s.len() > 4096 || !s.bytes().all(|c| c.is_ascii_graphic()) {
            return Err(invalid("invalid_qq_secret"));
        }
        Ok(())
    }
}
pub(crate) fn openid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}

pub mod actions;
