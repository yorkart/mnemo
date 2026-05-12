use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Debug, Clone)]
pub struct MnemoConfig {
    pub bind: SocketAddr,
    pub sqlite_path: PathBuf,
    pub artifact_dir: PathBuf,
}

impl MnemoConfig {
    pub fn from_env() -> Self {
        let bind = env::var("MNEMO_BIND")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| "127.0.0.1:8787".parse().expect("valid default bind"));
        let sqlite_path = env::var("MNEMO_SQLITE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("mnemo.db"));
        let artifact_dir = env::var("MNEMO_ARTIFACT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("mnemo-artifacts"));
        Self {
            bind,
            sqlite_path,
            artifact_dir,
        }
    }
}
