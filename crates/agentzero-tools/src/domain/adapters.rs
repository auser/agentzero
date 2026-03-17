use super::types::{SearchResult, SourceConfig};
use anyhow::Context;
use reqwest::Client;
use std::collections::HashMap;
use url::form_urlencoded;

const MAX_RESULTS_CAP: usize = 20;
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Search a single source using the appropriate adapter.
pub async fn search_source(
    client: &Client,
    source: &SourceConfig,
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let max = max_results.clamp(1, MAX_RESULTS_CAP);
    match source.kind.as_str() {
        "arxiv" => search_arxiv(client, query, &source.config, max).await,
        "semantic_scholar" => search_semantic_scholar(client, query, &source.config, max).await,
        "openalex" => search_openalex(client, query, &source.config, max).await,
        "web_search" => search_web(client, query, &source.config, max).await,
        "http_api" => search_generic_http(client, query, &source.config, max).await,
        other => anyhow::bail!("unknown source adapter: {other}"),
    }
}

/// Create a default reqwest client for domain tools.
pub fn default_client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .user_agent("AgentZero/1.0")
        .build()
        .unwrap_or_default()
}

// --- arXiv Adapter ---

async fn search_arxiv(
    client: &Client,
    query: &str,
    config: &serde_json::Value,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let category = config
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let sort_by = config
        .get("sort_by")
        .and_then(|v| v.as_str())
        .unwrap_or("relevance");

    let search_query = if category.is_empty() {
        format!(
            "all:{}",
            form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>()
        )
    } else {
        format!(
            "all:{}+AND+cat:{}",
            form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>(),
            category
        )
    };

    let url = format!(
        "http://export.arxiv.org/api/query?search_query={search_query}&start=0&max_results={max_results}&sortBy={sort_by}&sortOrder=descending"
    );

    let response = client
        .get(&url)
        .send()
        .await
        .context("arXiv API request failed")?;

    let body = response
        .text()
        .await
        .context("failed reading arXiv response")?;

    parse_arxiv_entries(&body, max_results)
}

fn parse_arxiv_entries(xml: &str, max_results: usize) -> anyhow::Result<Vec<SearchResult>> {
    let mut results = Vec::new();

    for entry_chunk in xml.split("<entry>").skip(1).take(max_results) {
        let title = extract_between(entry_chunk, "<title>", "</title>")
            .unwrap_or_default()
            .replace('\n', " ")
            .trim()
            .to_string();
        let summary = extract_between(entry_chunk, "<summary>", "</summary>")
            .unwrap_or_default()
            .replace('\n', " ")
            .trim()
            .to_string();
        let arxiv_id = extract_between(entry_chunk, "<id>", "</id>")
            .unwrap_or_default()
            .trim()
            .to_string();
        let published = extract_between(entry_chunk, "<published>", "</published>")
            .unwrap_or_default()
            .trim()
            .to_string();

        // Extract authors.
        let mut authors = Vec::new();
        for author_chunk in entry_chunk.split("<author>").skip(1) {
            if let Some(name) = extract_between(author_chunk, "<name>", "</name>") {
                authors.push(name.trim().to_string());
            }
        }

        // Extract PDF link.
        let pdf_url = extract_between(entry_chunk, "title=\"pdf\" href=\"", "\"")
            .unwrap_or_default()
            .trim()
            .to_string();

        // Extract categories.
        let mut categories = Vec::new();
        for cat_chunk in entry_chunk.split("category term=\"").skip(1) {
            if let Some(cat) = extract_between(cat_chunk, "", "\"") {
                categories.push(cat.to_string());
            }
        }

        let year = published.get(..4).and_then(|y| y.parse::<u64>().ok());

        let mut metadata = HashMap::new();
        if !arxiv_id.is_empty() {
            metadata.insert("arxiv_id".to_string(), serde_json::json!(arxiv_id));
        }
        if !published.is_empty() {
            metadata.insert("published".to_string(), serde_json::json!(published));
        }
        if let Some(y) = year {
            metadata.insert("year".to_string(), serde_json::json!(y));
        }
        if !pdf_url.is_empty() {
            metadata.insert("pdf_url".to_string(), serde_json::json!(pdf_url));
        }
        if !categories.is_empty() {
            metadata.insert("categories".to_string(), serde_json::json!(categories));
        }

        results.push(SearchResult {
            title,
            authors,
            url: arxiv_id.clone(),
            source_kind: "arxiv".to_string(),
            snippet: truncate_str(&summary, 300),
            metadata,
        });
    }

    Ok(results)
}

// --- Semantic Scholar Adapter ---

async fn search_semantic_scholar(
    client: &Client,
    query: &str,
    config: &serde_json::Value,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let api_key = config
        .get("api_key")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("SEMANTIC_SCHOLAR_API_KEY").ok());

    let encoded_query = form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/search?query={encoded_query}&limit={max_results}&fields=title,authors,abstract,year,citationCount,url,externalIds"
    );

    let mut req = client.get(&url);
    if let Some(ref key) = api_key {
        req = req.header("x-api-key", key);
    }

    let response = req
        .send()
        .await
        .context("Semantic Scholar request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Semantic Scholar API returned HTTP {status}: {body}");
    }

    let body: serde_json::Value = response
        .json()
        .await
        .context("failed parsing Semantic Scholar response")?;

    let mut results = Vec::new();
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for item in data.iter().take(max_results) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let abstract_text = item
                .get("abstract")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let paper_url = item
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let year = item.get("year").and_then(|v| v.as_u64());
            let citation_count = item.get("citationCount").and_then(|v| v.as_u64());

            let authors: Vec<String> = item
                .get("authors")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();

            let mut metadata = HashMap::new();
            if let Some(y) = year {
                metadata.insert("year".to_string(), serde_json::json!(y));
            }
            if let Some(c) = citation_count {
                metadata.insert("citation_count".to_string(), serde_json::json!(c));
            }

            // Extract external IDs (DOI, ArXiv).
            if let Some(ext_ids) = item.get("externalIds") {
                if let Some(doi) = ext_ids.get("DOI").and_then(|v| v.as_str()) {
                    metadata.insert("doi".to_string(), serde_json::json!(doi));
                }
                if let Some(arxiv) = ext_ids.get("ArXiv").and_then(|v| v.as_str()) {
                    metadata.insert("arxiv_id".to_string(), serde_json::json!(arxiv));
                }
            }

            results.push(SearchResult {
                title,
                authors,
                url: paper_url,
                source_kind: "semantic_scholar".to_string(),
                snippet: truncate_str(&abstract_text, 300),
                metadata,
            });
        }
    }

    Ok(results)
}

// --- OpenAlex Adapter ---

async fn search_openalex(
    client: &Client,
    query: &str,
    config: &serde_json::Value,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let email = config
        .get("email")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("OPENALEX_EMAIL").ok());

    let encoded_query = form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    let mut url = format!(
        "https://api.openalex.org/works?filter=default.search:{encoded_query}&per_page={max_results}"
    );
    if let Some(ref em) = email {
        url.push_str(&format!(
            "&mailto={}",
            form_urlencoded::byte_serialize(em.as_bytes()).collect::<String>()
        ));
    }

    let response = client
        .get(&url)
        .send()
        .await
        .context("OpenAlex request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("OpenAlex API returned HTTP {status}: {body}");
    }

    let body: serde_json::Value = response
        .json()
        .await
        .context("failed parsing OpenAlex response")?;

    let mut results = Vec::new();
    if let Some(works) = body.get("results").and_then(|r| r.as_array()) {
        for item in works.iter().take(max_results) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let doi = item
                .get("doi")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let year = item.get("publication_year").and_then(|v| v.as_u64());
            let citation_count = item.get("cited_by_count").and_then(|v| v.as_u64());

            // Extract abstract from inverted index (OpenAlex stores abstracts as inverted index).
            let abstract_text = item
                .get("abstract_inverted_index")
                .and_then(reconstruct_abstract)
                .unwrap_or_default();

            let authors: Vec<String> = item
                .get("authorships")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|a| {
                            a.get("author")
                                .and_then(|au| au.get("display_name"))
                                .and_then(|n| n.as_str())
                        })
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();

            // Open access URL.
            let oa_url = item
                .get("open_access")
                .and_then(|oa| oa.get("oa_url"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let display_url = if !oa_url.is_empty() {
                oa_url.clone()
            } else if !doi.is_empty() {
                doi.clone()
            } else {
                String::new()
            };

            let mut metadata = HashMap::new();
            if let Some(y) = year {
                metadata.insert("year".to_string(), serde_json::json!(y));
            }
            if let Some(c) = citation_count {
                metadata.insert("citation_count".to_string(), serde_json::json!(c));
            }
            if !doi.is_empty() {
                metadata.insert("doi".to_string(), serde_json::json!(doi));
            }
            if !oa_url.is_empty() {
                metadata.insert("open_access_url".to_string(), serde_json::json!(oa_url));
            }

            results.push(SearchResult {
                title,
                authors,
                url: display_url,
                source_kind: "openalex".to_string(),
                snippet: truncate_str(&abstract_text, 300),
                metadata,
            });
        }
    }

    Ok(results)
}

/// Reconstruct an abstract from OpenAlex's inverted index format.
fn reconstruct_abstract(inverted_index: &serde_json::Value) -> Option<String> {
    let obj = inverted_index.as_object()?;
    let mut positions: Vec<(usize, &str)> = Vec::new();
    for (word, indices) in obj {
        if let Some(arr) = indices.as_array() {
            for idx in arr {
                if let Some(pos) = idx.as_u64() {
                    positions.push((pos as usize, word.as_str()));
                }
            }
        }
    }
    if positions.is_empty() {
        return None;
    }
    positions.sort_by_key(|(pos, _)| *pos);
    let words: Vec<&str> = positions.iter().map(|(_, w)| *w).collect();
    Some(words.join(" "))
}

// --- Web Search Adapter ---

async fn search_web(
    client: &Client,
    query: &str,
    config: &serde_json::Value,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let query_prefix = config
        .get("query_prefix")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let full_query = if query_prefix.is_empty() {
        query.to_string()
    } else {
        format!("{query_prefix}{query}")
    };

    let encoded = form_urlencoded::byte_serialize(full_query.as_bytes()).collect::<String>();
    let url = format!("https://html.duckduckgo.com/html/?q={encoded}");

    let response = client
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
        let title = extract_between(chunk, ">", "</a>")
            .unwrap_or_default()
            .to_string();
        let href = extract_between(chunk, "href=\"", "\"")
            .unwrap_or_default()
            .to_string();
        let snippet = if let Some(snip_chunk) = chunk.split("class=\"result__snippet\"").nth(1) {
            extract_between(snip_chunk, ">", "</")
                .unwrap_or_default()
                .to_string()
        } else {
            String::new()
        };

        results.push(SearchResult {
            title: clean_html(&title),
            authors: vec![],
            url: href,
            source_kind: "web_search".to_string(),
            snippet: clean_html(&snippet),
            metadata: HashMap::new(),
        });
    }

    Ok(results)
}

// --- Generic HTTP API Adapter ---

async fn search_generic_http(
    client: &Client,
    query: &str,
    config: &serde_json::Value,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let url_template = config
        .get("url_template")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("http_api source requires url_template in config"))?;
    let method = config
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET");
    let results_path = config
        .get("results_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let field_map = config
        .get("field_map")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let headers = config
        .get("headers")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    // Expand template placeholders.
    let encoded_query = form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    let url = expand_template(url_template, &encoded_query, max_results)?;

    let mut req = match method.to_uppercase().as_str() {
        "POST" => client.post(&url),
        _ => client.get(&url),
    };

    // Add headers with env var expansion.
    for (key, value) in &headers {
        if let Some(v) = value.as_str() {
            let expanded = expand_env_vars(v)?;
            req = req.header(key.as_str(), expanded);
        }
    }

    let response = req.send().await.context("HTTP API request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("HTTP API returned {status}: {body}");
    }

    let body: serde_json::Value = response
        .json()
        .await
        .context("failed parsing HTTP API response as JSON")?;

    // Navigate to results array using dot-separated path.
    let results_value = if results_path.is_empty() {
        &body
    } else {
        navigate_json(&body, results_path).unwrap_or(&body)
    };

    let items = results_value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("results_path did not resolve to a JSON array"))?;

    let mut results = Vec::new();
    for item in items.iter().take(max_results) {
        let title = get_mapped_field(item, &field_map, "title");
        let url_val = get_mapped_field(item, &field_map, "url");
        let snippet = get_mapped_field(item, &field_map, "snippet");
        let authors_field = field_map
            .get("authors")
            .and_then(|v| v.as_str())
            .unwrap_or("authors");
        let authors = get_string_array(item, authors_field);

        results.push(SearchResult {
            title,
            authors,
            url: url_val,
            source_kind: "http_api".to_string(),
            snippet,
            metadata: HashMap::new(),
        });
    }

    Ok(results)
}

// --- Template & Utility Functions ---

fn expand_template(
    template: &str,
    encoded_query: &str,
    max_results: usize,
) -> anyhow::Result<String> {
    let mut result = template.replace("{{query}}", encoded_query);
    result = result.replace("{{max_results}}", &max_results.to_string());

    // Expand {{env:VAR_NAME}} placeholders.
    while let Some(start) = result.find("{{env:") {
        let after = &result[start + 6..];
        let end = after
            .find("}}")
            .ok_or_else(|| anyhow::anyhow!("unclosed {{{{env:...}}}} placeholder"))?;
        let var_name = &after[..end];
        validate_env_var_name(var_name)?;
        let value = std::env::var(var_name).unwrap_or_default();
        result = format!("{}{}{}", &result[..start], value, &after[end + 2..]);
    }

    Ok(result)
}

fn expand_env_vars(value: &str) -> anyhow::Result<String> {
    expand_template(value, "", 0)
}

fn validate_env_var_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("environment variable name must not be empty");
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        anyhow::bail!(
            "environment variable name must contain only alphanumeric characters and underscores: {name}"
        );
    }
    Ok(())
}

fn navigate_json<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current)
}

fn get_mapped_field(
    item: &serde_json::Value,
    field_map: &serde_json::Map<String, serde_json::Value>,
    field_name: &str,
) -> String {
    let source_field = field_map
        .get(field_name)
        .and_then(|v| v.as_str())
        .unwrap_or(field_name);

    // Support dot-separated paths.
    navigate_json(item, source_field)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn get_string_array(item: &serde_json::Value, field: &str) -> Vec<String> {
    navigate_json(item, field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
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

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a valid char boundary at or before max_len.
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Merge search results from multiple sources, deduplicating by DOI or fuzzy title match.
pub fn merge_results(all_results: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
    let mut merged: Vec<SearchResult> = Vec::new();
    let mut seen_dois: std::collections::HashSet<String> = std::collections::HashSet::new();

    for batch in all_results {
        for result in batch {
            // Check DOI-based dedup.
            if let Some(doi) = result.metadata.get("doi").and_then(|v| v.as_str()) {
                let normalized = doi.to_lowercase();
                if seen_dois.contains(&normalized) {
                    continue;
                }
                seen_dois.insert(normalized);
            } else {
                // Fuzzy title dedup: check if any existing result has a similar title.
                let normalized_title = normalize_title(&result.title);
                if merged
                    .iter()
                    .any(|r| normalize_title(&r.title) == normalized_title)
                {
                    continue;
                }
            }
            merged.push(result);
        }
    }

    merged
}

fn normalize_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_between_basic() {
        assert_eq!(extract_between("foo=bar;baz", "=", ";"), Some("bar"));
        assert_eq!(extract_between("nothing", "=", ";"), None);
    }

    #[test]
    fn clean_html_strips_tags() {
        assert_eq!(clean_html("<b>hello</b> world"), "hello world");
        assert_eq!(clean_html("no tags"), "no tags");
    }

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("short", 100), "short");
    }

    #[test]
    fn truncate_str_long() {
        let long = "a".repeat(400);
        let result = truncate_str(&long, 300);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 304);
    }

    #[test]
    fn navigate_json_dot_path() {
        let json: serde_json::Value = serde_json::json!({
            "data": {
                "items": [1, 2, 3]
            }
        });
        let result = navigate_json(&json, "data.items").expect("should navigate");
        assert!(result.is_array());
    }

    #[test]
    fn navigate_json_missing_path() {
        let json: serde_json::Value = serde_json::json!({"a": 1});
        assert!(navigate_json(&json, "b.c").is_none());
    }

    #[test]
    fn expand_template_replaces_placeholders() {
        let result = expand_template(
            "https://api.example.com?q={{query}}&n={{max_results}}",
            "test",
            5,
        )
        .expect("should expand");
        assert_eq!(result, "https://api.example.com?q=test&n=5");
    }

    #[test]
    fn validate_env_var_name_valid() {
        validate_env_var_name("MY_API_KEY").expect("should accept");
        validate_env_var_name("key123").expect("should accept");
    }

    #[test]
    fn validate_env_var_name_rejects_special() {
        let err = validate_env_var_name("bad-var").expect_err("should reject hyphens");
        assert!(err.to_string().contains("alphanumeric"));
    }

    #[test]
    fn merge_results_dedup_by_doi() {
        let batch1 = vec![SearchResult {
            title: "Paper A".to_string(),
            authors: vec![],
            url: String::new(),
            source_kind: "arxiv".to_string(),
            snippet: String::new(),
            metadata: [("doi".to_string(), serde_json::json!("10.1234/a"))]
                .into_iter()
                .collect(),
        }];
        let batch2 = vec![SearchResult {
            title: "Paper A (different title)".to_string(),
            authors: vec![],
            url: String::new(),
            source_kind: "semantic_scholar".to_string(),
            snippet: String::new(),
            metadata: [("doi".to_string(), serde_json::json!("10.1234/A"))]
                .into_iter()
                .collect(),
        }];

        let merged = merge_results(vec![batch1, batch2]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source_kind, "arxiv");
    }

    #[test]
    fn merge_results_dedup_by_title() {
        let batch1 = vec![SearchResult {
            title: "Attention Is All You Need".to_string(),
            authors: vec![],
            url: String::new(),
            source_kind: "arxiv".to_string(),
            snippet: String::new(),
            metadata: HashMap::new(),
        }];
        let batch2 = vec![SearchResult {
            title: "attention is all you need".to_string(),
            authors: vec![],
            url: String::new(),
            source_kind: "openalex".to_string(),
            snippet: String::new(),
            metadata: HashMap::new(),
        }];

        let merged = merge_results(vec![batch1, batch2]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn parse_arxiv_entries_basic() {
        let xml = r#"
        <feed>
        <entry>
            <id>http://arxiv.org/abs/1706.03762v7</id>
            <title>Attention Is All You Need</title>
            <summary>The dominant sequence transduction models.</summary>
            <published>2017-06-12T17:57:34Z</published>
            <author><name>Ashish Vaswani</name></author>
            <author><name>Noam Shazeer</name></author>
            <link title="pdf" href="http://arxiv.org/pdf/1706.03762v7"/>
            <category term="cs.CL"/>
        </entry>
        </feed>"#;

        let results = parse_arxiv_entries(xml, 10).expect("should parse");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Attention Is All You Need");
        assert_eq!(results[0].authors.len(), 2);
        assert_eq!(results[0].authors[0], "Ashish Vaswani");
        assert!(results[0].snippet.contains("dominant sequence"));
    }

    #[test]
    fn reconstruct_abstract_from_inverted_index() {
        let idx = serde_json::json!({
            "The": [0],
            "quick": [1],
            "brown": [2],
            "fox": [3]
        });
        let result = reconstruct_abstract(&idx).expect("should reconstruct");
        assert_eq!(result, "The quick brown fox");
    }

    #[test]
    fn get_mapped_field_with_dot_path() {
        let item = serde_json::json!({
            "properties": {
                "name": "Test Corp"
            }
        });
        let field_map: serde_json::Map<String, serde_json::Value> =
            [("title".to_string(), serde_json::json!("properties.name"))]
                .into_iter()
                .collect();

        let result = get_mapped_field(&item, &field_map, "title");
        assert_eq!(result, "Test Corp");
    }
}
