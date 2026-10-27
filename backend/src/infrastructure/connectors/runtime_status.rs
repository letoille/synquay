//! Live transport observations, independent of the configured enable flag.
use serde::Serialize;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    Connecting,
    Subscribing,
    Connected,
    Reconnecting,
    Stopped,
    Error,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    pub code: Option<&'static str>,
}
#[derive(Clone, Default)]
pub struct RuntimeStatus(Arc<RwLock<ConnectionStatus>>);
impl RuntimeStatus {
    pub fn snapshot(&self) -> ConnectionStatus {
        self.0
            .read()
            .map(|s| s.clone())
            .unwrap_or(ConnectionStatus {
                state: ConnectionState::Error,
                code: Some("status_unavailable"),
            })
    }
    pub fn update(&self, connection: Uuid, state: ConnectionState, code: Option<&'static str>) {
        if let Ok(mut current) = self.0.write()
            && (current.state != state || current.code != code)
        {
            tracing::info!(event="connection.state_changed",connection_id=%connection,previous=?current.state,state=?state,code,"Connection state changed");
            *current = ConnectionStatus { state, code };
        }
    }
    pub fn running(&self, connection: Uuid) -> RunningStatus {
        RunningStatus {
            status: self.clone(),
            connection,
        }
    }
}
pub struct RunningStatus {
    status: RuntimeStatus,
    connection: Uuid,
}
impl Drop for RunningStatus {
    fn drop(&mut self) {
        self.status
            .update(self.connection, ConnectionState::Stopped, None);
    }
}
