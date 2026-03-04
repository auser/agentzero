use anyhow::Context;
use futures_util::StreamExt;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PullProgress {
    pub status: String,
    pub digest: Option<String>,
    pub total: Option<u64>,
    pub completed: Option<u64>,
}

impl PullProgress {
    pub fn percent(&self) -> Option<f64> {
        match (self.total, self.completed) {
            (Some(total), Some(completed)) if total > 0 => {
                Some((completed as f64 / total as f64) * 100.0)
            }
            _ => None,
        }
    }
}

pub async fn pull_model(
    base_url: &str,
    model_name: &str,
    timeout_ms: u64,
    mut on_progress: impl FnMut(PullProgress),
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .context("failed to build HTTP client")?;

    let url = format!("{}/api/pull", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "name": model_name,
        "stream": true,
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("failed to connect to Ollama for model pull")?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        anyhow::bail!("Ollama returned HTTP {status} during pull: {body_text}");
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("error reading pull response stream")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim().to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Ok(progress) = serde_json::from_str::<OllamaPullStatus>(&line) {
                on_progress(PullProgress {
                    status: progress.status,
                    digest: progress.digest,
                    total: progress.total,
                    completed: progress.completed,
                });
            }
        }
    }

    if !buffer.trim().is_empty() {
        if let Ok(progress) = serde_json::from_str::<OllamaPullStatus>(buffer.trim()) {
            on_progress(PullProgress {
                status: progress.status,
                digest: progress.digest,
                total: progress.total,
                completed: progress.completed,
            });
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct OllamaPullStatus {
    status: String,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_progress_percent_calculates_correctly() {
        let progress = PullProgress {
            status: "downloading".to_string(),
            digest: None,
            total: Some(1000),
            completed: Some(500),
        };
        assert_eq!(progress.percent(), Some(50.0));
    }

    #[test]
    fn pull_progress_percent_none_when_no_total() {
        let progress = PullProgress {
            status: "downloading".to_string(),
            digest: None,
            total: None,
            completed: Some(500),
        };
        assert_eq!(progress.percent(), None);
    }

    #[test]
    fn pull_progress_percent_none_when_no_completed() {
        let progress = PullProgress {
            status: "downloading".to_string(),
            digest: None,
            total: Some(1000),
            completed: None,
        };
        assert_eq!(progress.percent(), None);
    }

    #[test]
    fn pull_progress_percent_zero_total_returns_none() {
        let progress = PullProgress {
            status: "downloading".to_string(),
            digest: None,
            total: Some(0),
            completed: Some(0),
        };
        assert_eq!(
            progress.percent(),
            None,
            "zero total should return None to avoid division by zero"
        );
    }

    #[test]
    fn pull_progress_percent_100_when_complete() {
        let progress = PullProgress {
            status: "downloading".to_string(),
            digest: None,
            total: Some(1000),
            completed: Some(1000),
        };
        assert_eq!(progress.percent(), Some(100.0));
    }

    #[test]
    fn parse_ollama_pull_status() {
        let json = r#"{"status":"downloading digestname","digest":"sha256:abc123","total":4661224676,"completed":2330612338}"#;
        let parsed: OllamaPullStatus = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.status, "downloading digestname");
        assert_eq!(parsed.total, Some(4661224676));
        assert_eq!(parsed.completed, Some(2330612338));
    }

    #[test]
    fn parse_ollama_pull_status_minimal() {
        let json = r#"{"status":"success"}"#;
        let parsed: OllamaPullStatus = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.status, "success");
        assert!(parsed.digest.is_none());
        assert!(parsed.total.is_none());
    }

    #[test]
    fn parse_ollama_pull_status_with_extra_fields() {
        let json = r#"{"status":"pulling manifest","extra_field":"ignored"}"#;
        let parsed: OllamaPullStatus = serde_json::from_str(json).expect("should parse");
        assert_eq!(parsed.status, "pulling manifest");
    }

    #[tokio::test]
    async fn pull_model_unreachable_returns_error() {
        let result = pull_model("http://127.0.0.1:19992", "test-model", 500, |_| {}).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("connect") || err.contains("Ollama"),
            "error should mention connection failure: {err}"
        );
    }
}
