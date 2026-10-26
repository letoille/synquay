use crate::domain::system::ServiceInfo;

#[derive(Clone, Default)]
pub struct SystemService;

impl SystemService {
    #[tracing::instrument(name = "system.info", skip_all)]
    pub fn info(&self) -> ServiceInfo {
        ServiceInfo {
            name: "Synquay",
            version: env!("CARGO_PKG_VERSION"),
            stage: "scaffold",
        }
    }
}
