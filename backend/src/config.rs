use std::{env, net::SocketAddr};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub log_format: LogFormat,
    pub log_filter: EnvFilter,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{0} must contain valid Unicode")]
    InvalidEncoding(&'static str),
    #[error("SYNQUAY_BIND must be an IP address and port, e.g. 127.0.0.1:8080")]
    InvalidBind,
    #[error("SYNQUAY_LOG_FORMAT must be text or json")]
    InvalidLogFormat,
    #[error("RUST_LOG contains an invalid tracing filter")]
    InvalidLogFilter,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::parse(
            read_env("SYNQUAY_BIND")?.as_deref(),
            read_env("SYNQUAY_LOG_FORMAT")?.as_deref(),
            read_env("RUST_LOG")?.as_deref(),
        )
    }

    fn parse(
        bind: Option<&str>,
        format: Option<&str>,
        filter: Option<&str>,
    ) -> Result<Self, ConfigError> {
        let bind = bind
            .unwrap_or("127.0.0.1:8080")
            .parse()
            .map_err(|_| ConfigError::InvalidBind)?;
        let log_format = match format.unwrap_or("text") {
            "text" => LogFormat::Text,
            "json" => LogFormat::Json,
            _ => return Err(ConfigError::InvalidLogFormat),
        };
        let log_filter = EnvFilter::try_new(filter.unwrap_or("info"))
            .map_err(|_| ConfigError::InvalidLogFilter)?;
        Ok(Self {
            bind,
            log_format,
            log_filter,
        })
    }
}

fn read_env(key: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError::InvalidEncoding(key)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_local_and_text_logs() {
        let config = Config::parse(None, None, None).unwrap();
        assert_eq!(config.bind.to_string(), "127.0.0.1:8080");
        assert_eq!(config.log_format, LogFormat::Text);
    }

    #[test]
    fn invalid_configuration_fails_before_startup() {
        assert!(matches!(
            Config::parse(Some("localhost"), None, None),
            Err(ConfigError::InvalidBind)
        ));
        assert!(matches!(
            Config::parse(None, Some("jsoon"), None),
            Err(ConfigError::InvalidLogFormat)
        ));
        assert!(matches!(
            Config::parse(None, None, Some("synquay_server=not-a-level")),
            Err(ConfigError::InvalidLogFilter)
        ));
    }

    #[test]
    fn accepts_explicit_address_and_json_logs() {
        let config = Config::parse(
            Some("127.0.0.1:18080"),
            Some("json"),
            Some("warn,synquay_server=debug"),
        )
        .unwrap();
        assert_eq!(config.bind.port(), 18080);
        assert_eq!(config.log_format, LogFormat::Json);
    }
}
