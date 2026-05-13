#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthStatus {
    pub service: &'static str,
    pub status: &'static str,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            service: "mnemo",
            status: "ok",
        }
    }
}
