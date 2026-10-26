#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub stage: &'static str,
}
