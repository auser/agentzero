use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use tokio::fs;

const PROXY_FILE: &str = ".agentzero/proxy.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProxySettings {
    #[serde(default)]
    http_proxy: Option<String>,
    #[serde(default)]
    https_proxy: Option<String>,
    #[serde(default)]
    socks_proxy: Option<String>,
    #[serde(default)]
    no_proxy: Vec<String>,
}

impl ProxySettings {
    async fn load(workspace_root: &str) -> anyhow::Result<Self> {
        let path = Path::new(workspace_root).join(PROXY_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path)
            .await
            .context("failed to read proxy config")?;
        serde_json::from_str(&data).context("failed to parse proxy config")
    }

    async fn save(&self, workspace_root: &str) -> anyhow::Result<()> {
        let path = Path::new(workspace_root).join(PROXY_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("failed to create .agentzero directory")?;
        }
        let data =
            serde_json::to_string_pretty(self).context("failed to serialize proxy config")?;
        fs::write(&path, data)
            .await
            .context("failed to write proxy config")
    }
}

#[derive(Debug, Deserialize)]
struct Input {
    op: String,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    host: Option<String>,
}

/// Runtime proxy configuration tool for HTTP/SOCKS proxy settings.
///
/// Operations:
/// - `get`: Get current proxy settings
/// - `set`: Set a proxy URL for a protocol (http, https, socks)
/// - `clear`: Clear a proxy setting for a protocol
/// - `add_bypass`: Add a host to the no_proxy bypass list
/// - `remove_bypass`: Remove a host from the no_proxy bypass list
#[derive(Debug, Default, Clone, Copy)]
pub struct ProxyConfigTool;

#[async_trait]
impl Tool for ProxyConfigTool {
    fn name(&self) -> &'static str {
        "proxy_config"
    }

    fn description(&self) -> &'static str {
        "Manage HTTP/HTTPS proxy settings: get, set, clear, add/remove bypass hosts."
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input =
            serde_json::from_str(input).context("proxy_config expects JSON: {\"op\", ...}")?;

        match parsed.op.as_str() {
            "get" => {
                let settings = ProxySettings::load(&ctx.workspace_root).await?;
                let output = json!({
                    "http_proxy": settings.http_proxy,
                    "https_proxy": settings.https_proxy,
                    "socks_proxy": settings.socks_proxy,
                    "no_proxy": settings.no_proxy,
                })
                .to_string();
                Ok(ToolResult { output })
            }
            "set" => {
                let protocol = parsed
                    .protocol
                    .as_deref()
                    .ok_or_else(|| anyhow!("set requires a `protocol` field"))?;
                let url = parsed
                    .url
                    .as_deref()
                    .ok_or_else(|| anyhow!("set requires a `url` field"))?;

                if url.trim().is_empty() {
                    return Err(anyhow!("url must not be empty"));
                }

                let mut settings = ProxySettings::load(&ctx.workspace_root).await?;
                match protocol {
                    "http" => settings.http_proxy = Some(url.to_string()),
                    "https" => settings.https_proxy = Some(url.to_string()),
                    "socks" => settings.socks_proxy = Some(url.to_string()),
                    other => {
                        return Err(anyhow!(
                            "unknown protocol: {other} (use http, https, or socks)"
                        ))
                    }
                }
                settings.save(&ctx.workspace_root).await?;

                Ok(ToolResult {
                    output: format!("set {protocol}_proxy={url}"),
                })
            }
            "clear" => {
                let protocol = parsed
                    .protocol
                    .as_deref()
                    .ok_or_else(|| anyhow!("clear requires a `protocol` field"))?;

                let mut settings = ProxySettings::load(&ctx.workspace_root).await?;
                match protocol {
                    "http" => settings.http_proxy = None,
                    "https" => settings.https_proxy = None,
                    "socks" => settings.socks_proxy = None,
                    other => {
                        return Err(anyhow!(
                            "unknown protocol: {other} (use http, https, or socks)"
                        ))
                    }
                }
                settings.save(&ctx.workspace_root).await?;

                Ok(ToolResult {
                    output: format!("cleared {protocol}_proxy"),
                })
            }
            "add_bypass" => {
                let host = parsed
                    .host
                    .as_deref()
                    .ok_or_else(|| anyhow!("add_bypass requires a `host` field"))?;

                if host.trim().is_empty() {
                    return Err(anyhow!("host must not be empty"));
                }

                let mut settings = ProxySettings::load(&ctx.workspace_root).await?;
                if !settings.no_proxy.contains(&host.to_string()) {
                    settings.no_proxy.push(host.to_string());
                    settings.save(&ctx.workspace_root).await?;
                }

                Ok(ToolResult {
                    output: format!("added bypass for {host}"),
                })
            }
            "remove_bypass" => {
                let host = parsed
                    .host
                    .as_deref()
                    .ok_or_else(|| anyhow!("remove_bypass requires a `host` field"))?;

                let mut settings = ProxySettings::load(&ctx.workspace_root).await?;
                let before = settings.no_proxy.len();
                settings.no_proxy.retain(|h| h != host);
                let removed = before != settings.no_proxy.len();
                if removed {
                    settings.save(&ctx.workspace_root).await?;
                }

                Ok(ToolResult {
                    output: if removed {
                        format!("removed bypass for {host}")
                    } else {
                        format!("host not in bypass list: {host}")
                    },
                })
            }
            other => Ok(ToolResult {
                output: json!({ "error": format!("unknown op: {other}") }).to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-proxy-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn proxy_get_empty_defaults() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = ProxyConfigTool
            .execute(r#"{"op": "get"}"#, &ctx)
            .await
            .expect("get should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(v["http_proxy"].is_null());
        assert!(v["https_proxy"].is_null());
        assert!(v["no_proxy"].as_array().unwrap().is_empty());

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn proxy_set_and_get_roundtrip() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        ProxyConfigTool
            .execute(
                r#"{"op": "set", "protocol": "http", "url": "http://proxy:8080"}"#,
                &ctx,
            )
            .await
            .expect("set should succeed");

        let result = ProxyConfigTool
            .execute(r#"{"op": "get"}"#, &ctx)
            .await
            .expect("get should succeed");
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(v["http_proxy"], "http://proxy:8080");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn proxy_clear_removes_setting() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        ProxyConfigTool
            .execute(
                r#"{"op": "set", "protocol": "socks", "url": "socks5://127.0.0.1:1080"}"#,
                &ctx,
            )
            .await
            .unwrap();

        ProxyConfigTool
            .execute(r#"{"op": "clear", "protocol": "socks"}"#, &ctx)
            .await
            .expect("clear should succeed");

        let result = ProxyConfigTool
            .execute(r#"{"op": "get"}"#, &ctx)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(v["socks_proxy"].is_null());

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn proxy_bypass_add_and_remove() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        ProxyConfigTool
            .execute(r#"{"op": "add_bypass", "host": "localhost"}"#, &ctx)
            .await
            .expect("add_bypass should succeed");

        ProxyConfigTool
            .execute(r#"{"op": "add_bypass", "host": "127.0.0.1"}"#, &ctx)
            .await
            .unwrap();

        let result = ProxyConfigTool
            .execute(r#"{"op": "get"}"#, &ctx)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        let no_proxy = v["no_proxy"].as_array().unwrap();
        assert_eq!(no_proxy.len(), 2);

        let result = ProxyConfigTool
            .execute(r#"{"op": "remove_bypass", "host": "localhost"}"#, &ctx)
            .await
            .expect("remove_bypass should succeed");
        assert!(result.output.contains("removed bypass"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn proxy_set_unknown_protocol_fails() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = ProxyConfigTool
            .execute(
                r#"{"op": "set", "protocol": "ftp", "url": "ftp://proxy:21"}"#,
                &ctx,
            )
            .await
            .expect_err("unknown protocol should fail");
        assert!(err.to_string().contains("unknown protocol"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn proxy_set_empty_url_fails() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = ProxyConfigTool
            .execute(r#"{"op": "set", "protocol": "http", "url": ""}"#, &ctx)
            .await
            .expect_err("empty url should fail");
        assert!(err.to_string().contains("url must not be empty"));

        fs::remove_dir_all(dir).ok();
    }
}
