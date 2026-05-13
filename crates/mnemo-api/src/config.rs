#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind_address: String,
    pub bearer_token: Option<String>,
    pub data_path: Option<String>,
    pub sqlite_path: Option<String>,
    pub wrapup_command: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".to_string(),
            bearer_token: None,
            data_path: None,
            sqlite_path: None,
            wrapup_command: None,
        }
    }
}
