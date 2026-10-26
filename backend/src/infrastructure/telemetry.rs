use crate::config::LogFormat;
use std::{error::Error, io::IsTerminal};
use tracing_subscriber::EnvFilter;

pub fn init(format: LogFormat, filter: EnvFilter) -> Result<(), Box<dyn Error + Send + Sync>> {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr);
    match format {
        LogFormat::Text => subscriber
            .with_ansi(std::io::stderr().is_terminal())
            .try_init()?,
        LogFormat::Json => subscriber.json().with_ansi(false).try_init()?,
    }
    Ok(())
}
