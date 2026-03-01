use agentzero_storage::EncryptedJsonStore;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

const TUNNELS_FILE: &str = "tunnels-state.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TunnelProtocol {
    Http,
    Https,
    Ssh,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TunnelError {
    #[error("unsupported protocol: {0}")]
    UnsupportedProtocol(String),
    #[error("remote target must be formatted as <host>:<port>")]
    InvalidRemote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelSession {
    pub name: String,
    pub protocol: TunnelProtocol,
    pub remote: String,
    pub local_port: u16,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct TunnelStore {
    store: EncryptedJsonStore,
}

impl TunnelStore {
    pub fn new(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            store: EncryptedJsonStore::in_config_dir(data_dir.as_ref(), TUNNELS_FILE)?,
        })
    }

    pub fn list(&self) -> anyhow::Result<Vec<TunnelSession>> {
        self.store.load_or_default()
    }

    pub fn start(
        &self,
        name: &str,
        protocol: TunnelProtocol,
        remote: &str,
        local_port: u16,
    ) -> anyhow::Result<TunnelSession> {
        if name.trim().is_empty() {
            bail!("tunnel name cannot be empty");
        }
        validate_remote_target(remote).map_err(|err| anyhow::Error::msg(err.to_string()))?;
        if local_port == 0 {
            bail!("local port must be greater than 0");
        }

        let mut sessions = self.list()?;
        if sessions.iter().any(|s| s.name == name && s.active) {
            bail!("tunnel `{name}` is already active");
        }

        // Replace existing record with same name, preserving a single logical tunnel slot.
        sessions.retain(|s| s.name != name);
        let session = TunnelSession {
            name: name.to_string(),
            protocol,
            remote: remote.to_string(),
            local_port,
            active: true,
        };
        sessions.push(session.clone());
        self.store.save(&sessions)?;
        Ok(session)
    }

    pub fn stop(&self, name: &str) -> anyhow::Result<TunnelSession> {
        let mut sessions = self.list()?;
        let session = sessions
            .iter_mut()
            .find(|s| s.name == name)
            .with_context(|| format!("tunnel `{name}` not found"))?;

        if !session.active {
            bail!("tunnel `{name}` is already stopped");
        }

        session.active = false;
        let updated = session.clone();
        self.store.save(&sessions)?;
        Ok(updated)
    }

    pub fn status(&self, name: &str) -> anyhow::Result<TunnelSession> {
        let sessions = self.list()?;
        sessions
            .into_iter()
            .find(|s| s.name == name)
            .with_context(|| format!("tunnel `{name}` not found"))
    }
}

pub fn parse_tunnel_protocol(input: &str) -> Result<TunnelProtocol, TunnelError> {
    match input.to_ascii_lowercase().as_str() {
        "http" => Ok(TunnelProtocol::Http),
        "https" => Ok(TunnelProtocol::Https),
        "ssh" => Ok(TunnelProtocol::Ssh),
        _ => Err(TunnelError::UnsupportedProtocol(input.to_string())),
    }
}

pub fn validate_remote_target(remote: &str) -> Result<(), TunnelError> {
    let (host, port) = remote.split_once(':').ok_or(TunnelError::InvalidRemote)?;
    if host.trim().is_empty() {
        return Err(TunnelError::InvalidRemote);
    }

    let port: u16 = port.parse().map_err(|_| TunnelError::InvalidRemote)?;
    if port == 0 {
        return Err(TunnelError::InvalidRemote);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("agentzero-tunnel-test-{nanos}-{seq}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn validate_remote_target_accepts_host_and_port() {
        validate_remote_target("localhost:42617").expect("valid remote target");
    }

    #[test]
    fn validate_remote_target_rejects_missing_port() {
        let err = validate_remote_target("localhost").expect_err("missing port should fail");
        assert_eq!(err, TunnelError::InvalidRemote);
    }

    #[test]
    fn start_stop_status_success_path() {
        let dir = temp_dir();
        let store = TunnelStore::new(&dir).expect("store should create");

        let started = store
            .start("default", TunnelProtocol::Https, "example.com:443", 9422)
            .expect("start should succeed");
        assert!(started.active);

        let status = store.status("default").expect("status should succeed");
        assert_eq!(status.remote, "example.com:443");
        assert!(status.active);

        let stopped = store.stop("default").expect("stop should succeed");
        assert!(!stopped.active);

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[test]
    fn start_fails_for_invalid_remote_negative_path() {
        let dir = temp_dir();
        let store = TunnelStore::new(&dir).expect("store should create");

        let err = store
            .start("default", TunnelProtocol::Http, "invalid", 8080)
            .expect_err("invalid remote should fail");
        assert!(err.to_string().contains("remote target"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}
