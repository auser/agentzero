use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use url::form_urlencoded;

const MAX_RESULTS_CAP: usize = 10;
const DEFAULT_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, Deserialize)]
struct WebSearchInput {
    query: String,
    #[serde(default = "default_max_results")]
    max_results: usize,
    #[serde(default)]
    provider: Option<String>,
}

fn default_max_results() -> usize {
    5
}

#[derive(Debug, Clone)]
pub struct WebSearchConfig {
    pub provider: String,
    pub brave_api_key: Option<String>,
    pub jina_api_key: Option<String>,
    pub timeout_secs: u64,
    pub user_agent: String,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: "duckduckgo".to_string(),
            brave_api_key: None,
            jina_api_key: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            user_agent: "AgentZero/1.0".to_string(),
        }
    }
}

pub struct WebSearchTool {
    client: reqwest::Client,
    config: WebSearchConfig,
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new(WebSearchConfig::default())
    }
}

impl WebSearchTool {
    pub fn new(config: WebSearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .user_agent(&config.user_agent)
            .build()
            .unwrap_or_default();
        Self { client, config }
    }

    async fn search_duckduckgo(&self, query: &str, max_results: usize) -> anyhow::Result<String> {
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>()
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("DuckDuckGo request failed")?;
        let body = response
            .text()
            .await
            .context("failed reading DuckDuckGo response")?;

        let mut results = Vec::new();
        for (i, chunk) in body.split("class=\"result__a\"").skip(1).enumerate() {
            if i >= max_results {
                break;
            }
            let title = extract_between(chunk, ">", "</a>").unwrap_or_default();
            let href = extract_between(chunk, "href=\"", "\"").unwrap_or_default();
            let snippet = if let Some(snip_chunk) = chunk.split("class=\"result__snippet\"").nth(1)
            {
                extract_between(snip_chunk, ">", "</")
                    .unwrap_or_default()
                    .replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("<b>", "")
                    .replace("</b>", "")
            } else {
                String::new()
            };
            results.push(format!(
                "{}. {}\n   {}\n   {}",
                i + 1,
                clean_html(title),
                href,
                clean_html(&snippet)
            ));
        }

        if results.is_empty() {
            Ok("no results found".to_string())
        } else {
            Ok(results.join("\n\n"))
        }
    }

    async fn search_brave(
        &self,
        query: &str,
        max_results: usize,
        api_key: &str,
    ) -> anyhow::Result<String> {
        let url = format!(
            "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
            form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>(),
            max_results.min(MAX_RESULTS_CAP)
        );
        let response = self
            .client
            .get(&url)
            .header("X-Subscription-Token", api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .context("Brave search request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Brave API returned HTTP {status}: {body}");
        }

        let body: serde_json::Value = response
            .json()
            .await
            .context("failed parsing Brave response")?;
        let mut results = Vec::new();
        if let Some(web) = body
            .get("web")
            .and_then(|w| w.get("results"))
            .and_then(|r| r.as_array())
        {
            for (i, item) in web.iter().enumerate().take(max_results) {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                results.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, desc));
            }
        }

        if results.is_empty() {
            Ok("no results found".to_string())
        } else {
            Ok(results.join("\n\n"))
        }
    }

    async fn search_jina(
        &self,
        query: &str,
        max_results: usize,
        api_key: &str,
    ) -> anyhow::Result<String> {
        let url = format!(
            "https://s.jina.ai/{}",
            form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>()
        );
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Accept", "application/json")
            .send()
            .await
            .context("Jina search request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Jina API returned HTTP {status}: {body}");
        }

        let body: serde_json::Value = response
            .json()
            .await
            .context("failed parsing Jina response")?;
        let mut results = Vec::new();
        if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
            for (i, item) in data.iter().enumerate().take(max_results) {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                results.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, desc));
            }
        }

        if results.is_empty() {
            Ok("no results found".to_string())
        } else {
            Ok(results.join("\n\n"))
        }
    }
}

fn extract_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let s = text.find(start)? + start.len();
    let e = text[s..].find(end)? + s;
    Some(&text[s..e])
}

fn clean_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(ch);
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn description(&self) -> &'static str {
        "Search the web using DuckDuckGo, Brave, or Jina and return a summary of results."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            },
            "required": ["query"]
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: WebSearchInput =
            serde_json::from_str(input).context("web_search expects JSON: {\"query\": \"...\"}")?;

        if req.query.trim().is_empty() {
            return Err(anyhow!("query must not be empty"));
        }

        let max = req.max_results.clamp(1, MAX_RESULTS_CAP);
        let provider = req.provider.as_deref().unwrap_or(&self.config.provider);

        let brave_env_key = std::env::var("BRAVE_API_KEY").ok();
        let jina_env_key = std::env::var("JINA_API_KEY").ok();

        let output = match provider {
            "brave" => {
                let key = self
                    .config
                    .brave_api_key
                    .as_deref()
                    .or(brave_env_key.as_deref())
                    .ok_or_else(|| {
                        anyhow!("brave_api_key is required for Brave search provider")
                    })?;
                self.search_brave(&req.query, max, key).await?
            }
            "jina" => {
                let key = self
                    .config
                    .jina_api_key
                    .as_deref()
                    .or(jina_env_key.as_deref())
                    .ok_or_else(|| anyhow!("jina_api_key is required for Jina search provider"))?;
                self.search_jina(&req.query, max, key).await?
            }
            _ => self.search_duckduckgo(&req.query, max).await?,
        };

        Ok(ToolResult { output })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn web_search_rejects_empty_query() {
        let tool = WebSearchTool::default();
        let err = tool
            .execute(r#"{"query": ""}"#, &ToolContext::new(".".to_string()))
            .await
            .expect_err("empty query should fail");
        assert!(err.to_string().contains("query must not be empty"));
    }

    #[tokio::test]
    async fn web_search_rejects_invalid_json() {
        let tool = WebSearchTool::default();
        let err = tool
            .execute("not json", &ToolContext::new(".".to_string()))
            .await
            .expect_err("invalid JSON should fail");
        assert!(err.to_string().contains("web_search expects JSON"));
    }

    #[tokio::test]
    async fn web_search_brave_requires_api_key() {
        let tool = WebSearchTool::new(WebSearchConfig {
            provider: "brave".to_string(),
            brave_api_key: None,
            ..Default::default()
        });
        let err = tool
            .execute(r#"{"query": "test"}"#, &ToolContext::new(".".to_string()))
            .await
            .expect_err("missing API key should fail");
        assert!(err.to_string().contains("brave_api_key"));
    }

    #[test]
    fn clean_html_strips_tags() {
        assert_eq!(clean_html("<b>hello</b> world"), "hello world");
        assert_eq!(clean_html("no tags"), "no tags");
    }

    #[test]
    fn extract_between_works() {
        assert_eq!(extract_between("foo=bar;baz", "=", ";"), Some("bar"));
        assert_eq!(extract_between("nothing", "=", ";"), None);
    }
}
