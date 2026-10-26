use crate::domain::system::ServiceInfo;
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

#[derive(Serialize)]
pub struct ServiceInfoResponse {
    name: &'static str,
    version: &'static str,
    stage: &'static str,
}

impl From<ServiceInfo> for ServiceInfoResponse {
    fn from(info: ServiceInfo) -> Self {
        Self {
            name: info.name,
            version: info.version,
            stage: info.stage,
        }
    }
}
