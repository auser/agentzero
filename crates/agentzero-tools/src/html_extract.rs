use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::anyhow;
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde::Deserialize;
use std::collections::HashMap;

/// Extracts structured data from raw HTML using CSS selectors.
///
/// Input: JSON with `html` (raw HTML string) and `selectors` (map of name → CSS selector).
/// Output: JSON object mapping each name to extracted text (single match) or array of text (multiple matches).
pub struct HtmlExtractTool;

#[derive(Deserialize)]
struct HtmlExtractInput {
    html: String,
    selectors: HashMap<String, String>,
}

#[async_trait]
impl Tool for HtmlExtractTool {
    fn name(&self) -> &'static str {
        "html_extract"
    }

    fn description(&self) -> &'static str {
        "Extract structured data from HTML using CSS selectors. Returns a JSON object mapping selector names to extracted text."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "html": {
                    "type": "string",
                    "description": "Raw HTML content to parse"
                },
                "selectors": {
                    "type": "object",
                    "description": "Map of field names to CSS selectors. Each selector extracts text from matching elements.",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["html", "selectors"]
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: HtmlExtractInput =
            serde_json::from_str(input).map_err(|e| anyhow!("invalid input: {e}"))?;

        if parsed.html.is_empty() {
            return Err(anyhow!("html must not be empty"));
        }
        if parsed.selectors.is_empty() {
            return Err(anyhow!("selectors must not be empty"));
        }

        let document = Html::parse_document(&parsed.html);
        let mut results: HashMap<String, serde_json::Value> = HashMap::new();

        for (name, css) in &parsed.selectors {
            let selector = Selector::parse(css)
                .map_err(|_| anyhow!("invalid CSS selector for \"{name}\": {css}"))?;

            let matches: Vec<String> = document
                .select(&selector)
                .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let value = if matches.len() == 1 {
                serde_json::Value::String(matches.into_iter().next().unwrap())
            } else {
                serde_json::Value::Array(
                    matches.into_iter().map(serde_json::Value::String).collect(),
                )
            };

            results.insert(name.clone(), value);
        }

        let output = serde_json::to_string_pretty(&results)
            .map_err(|e| anyhow!("failed to serialize results: {e}"))?;
        Ok(ToolResult { output })
    }
}

#[cfg(test)]
mod tests {
    use super::HtmlExtractTool;
    use agentzero_core::{Tool, ToolContext};

    #[tokio::test]
    async fn extracts_single_element() {
        let tool = HtmlExtractTool;
        let input = serde_json::json!({
            "html": "<html><body><h1>Hello World</h1></body></html>",
            "selectors": { "title": "h1" }
        });
        let result = tool
            .execute(&input.to_string(), &ToolContext::new(".".to_string()))
            .await
            .expect("should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result.output).expect("should be valid JSON");
        assert_eq!(parsed["title"], "Hello World");
    }

    #[tokio::test]
    async fn extracts_multiple_elements_as_array() {
        let tool = HtmlExtractTool;
        let input = serde_json::json!({
            "html": "<ul><li>One</li><li>Two</li><li>Three</li></ul>",
            "selectors": { "items": "li" }
        });
        let result = tool
            .execute(&input.to_string(), &ToolContext::new(".".to_string()))
            .await
            .expect("should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result.output).expect("should be valid JSON");
        let items = parsed["items"].as_array().expect("should be array");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], "One");
        assert_eq!(items[2], "Three");
    }

    #[tokio::test]
    async fn extracts_multiple_selectors() {
        let tool = HtmlExtractTool;
        let input = serde_json::json!({
            "html": "<div><h1>Title</h1><p class=\"desc\">Description</p><span class=\"price\">$99</span></div>",
            "selectors": {
                "title": "h1",
                "description": ".desc",
                "price": ".price"
            }
        });
        let result = tool
            .execute(&input.to_string(), &ToolContext::new(".".to_string()))
            .await
            .expect("should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result.output).expect("should be valid JSON");
        assert_eq!(parsed["title"], "Title");
        assert_eq!(parsed["description"], "Description");
        assert_eq!(parsed["price"], "$99");
    }

    #[tokio::test]
    async fn returns_empty_array_for_no_matches() {
        let tool = HtmlExtractTool;
        let input = serde_json::json!({
            "html": "<p>Hello</p>",
            "selectors": { "missing": ".nonexistent" }
        });
        let result = tool
            .execute(&input.to_string(), &ToolContext::new(".".to_string()))
            .await
            .expect("should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&result.output).expect("should be valid JSON");
        let items = parsed["missing"].as_array().expect("should be array");
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn rejects_empty_html() {
        let tool = HtmlExtractTool;
        let input = serde_json::json!({
            "html": "",
            "selectors": { "title": "h1" }
        });
        let err = tool
            .execute(&input.to_string(), &ToolContext::new(".".to_string()))
            .await
            .expect_err("empty html should fail");
        assert!(err.to_string().contains("html must not be empty"));
    }

    #[tokio::test]
    async fn rejects_empty_selectors() {
        let tool = HtmlExtractTool;
        let input = serde_json::json!({
            "html": "<p>Hello</p>",
            "selectors": {}
        });
        let err = tool
            .execute(&input.to_string(), &ToolContext::new(".".to_string()))
            .await
            .expect_err("empty selectors should fail");
        assert!(err.to_string().contains("selectors must not be empty"));
    }

    #[tokio::test]
    async fn rejects_invalid_css_selector() {
        let tool = HtmlExtractTool;
        let input = serde_json::json!({
            "html": "<p>Hello</p>",
            "selectors": { "bad": "[[[invalid" }
        });
        let err = tool
            .execute(&input.to_string(), &ToolContext::new(".".to_string()))
            .await
            .expect_err("invalid selector should fail");
        assert!(err.to_string().contains("invalid CSS selector"));
    }
}
