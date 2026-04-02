use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
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
    pub fallback_providers: Vec<String>,
    pub retries_per_provider: u32,
    pub retry_backoff_ms: u64,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: "duckduckgo".to_string(),
            brave_api_key: None,
            jina_api_key: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
            fallback_providers: Vec::new(),
            retries_per_provider: 2,
            retry_backoff_ms: 500,
        }
    }
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct WebSearchSchema {
    /// The search query
    query: String,
}

#[tool(
    name = "web_search",
    description = "Search the web using DuckDuckGo, Brave, or Jina and return a summary of results."
)]
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
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(WebSearchSchema::schema())
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Be forgiving: try JSON first, then treat the whole input as a query string.
        let req: WebSearchInput = match serde_json::from_str(input) {
            Ok(r) => r,
            Err(_) => {
                // The model may send a bare string, or {"search": "..."}, etc.
                // Extract anything that looks like a query.
                let query = if let Ok(val) = serde_json::from_str::<serde_json::Value>(input) {
                    // Try common field names the model might use.
                    val.get("query")
                        .or_else(|| val.get("search"))
                        .or_else(|| val.get("q"))
                        .or_else(|| val.get("search_query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                } else {
                    // Bare string input — use it directly.
                    input.trim().trim_matches('"').to_string()
                };
                WebSearchInput {
                    query,
                    max_results: default_max_results(),
                    provider: None,
                }
            }
        };

        if req.query.trim().is_empty() {
            return Err(anyhow!("query must not be empty"));
        }

        let max = req.max_results.clamp(1, MAX_RESULTS_CAP);
        let primary = req.provider.as_deref().unwrap_or(&self.config.provider);

        let brave_env_key = std::env::var("BRAVE_API_KEY").ok();
        let jina_env_key = std::env::var("JINA_API_KEY").ok();

        // Build provider chain: primary + fallbacks.
        let mut providers = vec![primary.to_string()];
        for fb in &self.config.fallback_providers {
            if fb != primary {
                providers.push(fb.clone());
            }
        }

        let retries = self.config.retries_per_provider.max(1) as usize;
        let backoff_ms = self.config.retry_backoff_ms;
        let mut last_err = None;

        for provider in &providers {
            for attempt in 0..retries {
                let result = match provider.as_str() {
                    "brave" => {
                        let key = self
                            .config
                            .brave_api_key
                            .as_deref()
                            .or(brave_env_key.as_deref())
                            .ok_or_else(|| {
                                anyhow!("brave_api_key is required for Brave search provider")
                            });
                        match key {
                            Ok(k) => self.search_brave(&req.query, max, k).await,
                            Err(e) => Err(e),
                        }
                    }
                    "jina" => {
                        let key = self
                            .config
                            .jina_api_key
                            .as_deref()
                            .or(jina_env_key.as_deref())
                            .ok_or_else(|| {
                                anyhow!("jina_api_key is required for Jina search provider")
                            });
                        match key {
                            Ok(k) => self.search_jina(&req.query, max, k).await,
                            Err(e) => Err(e),
                        }
                    }
                    _ => self.search_duckduckgo(&req.query, max).await,
                };

                match result {
                    Ok(output) => return Ok(ToolResult { output }),
                    Err(e) => {
                        tracing::warn!(
                            provider = %provider,
                            attempt = attempt + 1,
                            error = %e,
                            "web search failed, retrying"
                        );
                        last_err = Some(e);
                        if attempt + 1 < retries {
                            tokio::time::sleep(Duration::from_millis(
                                backoff_ms * (attempt as u64 + 1),
                            ))
                            .await;
                        }
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow!("all web search providers failed")))
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
    async fn web_search_accepts_bare_string_as_query() {
        // Verify bare strings are accepted as queries (forgiving parsing)
        // without making a real network call. We use a Brave provider with
        // a dummy API key — it will fail at the API level (not parsing).
        let tool = WebSearchTool::new(WebSearchConfig {
            provider: "brave".to_string(),
            brave_api_key: Some("test-key-not-real".to_string()),
            timeout_secs: 1,
            ..Default::default()
        });
        let result = tool
            .execute("not json", &ToolContext::new(".".to_string()))
            .await;
        // Either succeeds or fails with a network error, NOT a parse error.
        if let Err(e) = &result {
            assert!(
                !e.to_string().contains("web_search expects JSON"),
                "bare string should not be rejected as invalid JSON"
            );
        }
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
