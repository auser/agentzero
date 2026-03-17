use super::adapters::{default_client, merge_results, search_source};
use super::learning::LessonStore;
use super::store::DomainStore;
use super::types::{
    validate_domain_name, Domain, DomainConstraints, FindingToVerify, SourceConfig,
    VerificationConfig, VerificationResult, VerificationStatus, VerificationStrategy,
    WorkflowTemplate,
};
use crate::sop_tools::SopExecuteTool;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

// ============================================================================
// domain_create
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainCreateInput {
    name: String,
    description: String,
    #[serde(default)]
    sources: Vec<SourceConfig>,
    #[serde(default)]
    verification: Option<VerificationConfig>,
    #[serde(default)]
    workflows: Vec<WorkflowTemplate>,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    constraints: Option<DomainConstraints>,
    #[serde(default)]
    template: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DomainCreateTool;

#[async_trait]
impl Tool for DomainCreateTool {
    fn name(&self) -> &'static str {
        "domain_create"
    }

    fn description(&self) -> &'static str {
        "Create a new research domain with configured sources, verification strategies, workflows, and AI persona. Domains can also be created from built-in templates."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Unique domain identifier (alphanumeric, hyphens, underscores)" },
                "description": { "type": "string", "description": "What this domain covers" },
                "sources": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "description": "Adapter: arxiv, semantic_scholar, openalex, web_search, http_api" },
                            "label": { "type": "string" },
                            "config": { "type": "object" },
                            "priority": { "type": "integer" },
                            "enabled": { "type": "boolean" }
                        },
                        "required": ["kind", "label"]
                    }
                },
                "verification": {
                    "type": "object",
                    "properties": {
                        "strategies": { "type": "array" },
                        "min_confidence": { "type": "number" }
                    }
                },
                "workflows": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" },
                            "steps": { "type": "array", "items": { "type": "string" } },
                            "approval_required": { "type": "array", "items": { "type": "integer" } }
                        },
                        "required": ["name", "steps"]
                    }
                },
                "system_prompt": { "type": "string" },
                "constraints": { "type": "object" },
                "template": { "type": "string", "description": "Create from built-in template: academic_research, competitive_intelligence" }
            },
            "required": ["name", "description"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainCreateInput =
            serde_json::from_str(input).context("domain_create expects valid JSON")?;

        validate_domain_name(&req.name)?;

        if DomainStore::exists(&ctx.workspace_root, &req.name).await {
            return Err(anyhow!("domain already exists: {}", req.name));
        }

        // If a template is specified, start from the built-in and override fields.
        let domain = if let Some(ref template_name) = req.template {
            let mut base = super::builtins::get_builtin(template_name)?;
            base.name = req.name.clone();
            base.description = req.description.clone();
            if !req.sources.is_empty() {
                base.sources = req.sources;
            }
            if let Some(v) = req.verification {
                base.verification = v;
            }
            if !req.workflows.is_empty() {
                base.workflows = req.workflows;
            }
            if !req.system_prompt.is_empty() {
                base.system_prompt = req.system_prompt;
            }
            if let Some(c) = req.constraints {
                base.constraints = c;
            }
            base.created_at = now_iso8601();
            base.updated_at = now_iso8601();
            base
        } else {
            Domain {
                name: req.name.clone(),
                description: req.description,
                sources: req.sources,
                verification: req.verification.unwrap_or_default(),
                workflows: req.workflows,
                system_prompt: req.system_prompt,
                constraints: req.constraints.unwrap_or_default(),
                created_at: now_iso8601(),
                updated_at: now_iso8601(),
                enabled: true,
            }
        };

        DomainStore::save(&ctx.workspace_root, &domain).await?;

        Ok(ToolResult {
            output: format!(
                "created domain={} sources={} workflows={} verification_strategies={}",
                domain.name,
                domain.sources.len(),
                domain.workflows.len(),
                domain.verification.strategies.len(),
            ),
        })
    }
}

// ============================================================================
// domain_update
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainUpdateInput {
    name: String,
    #[serde(default)]
    set_description: Option<String>,
    #[serde(default)]
    add_sources: Vec<SourceConfig>,
    #[serde(default)]
    remove_sources: Vec<String>,
    #[serde(default)]
    set_verification: Option<VerificationConfig>,
    #[serde(default)]
    add_workflows: Vec<WorkflowTemplate>,
    #[serde(default)]
    remove_workflows: Vec<String>,
    #[serde(default)]
    set_system_prompt: Option<String>,
    #[serde(default)]
    set_constraints: Option<DomainConstraints>,
    #[serde(default)]
    set_enabled: Option<bool>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DomainUpdateTool;

#[async_trait]
impl Tool for DomainUpdateTool {
    fn name(&self) -> &'static str {
        "domain_update"
    }

    fn description(&self) -> &'static str {
        "Update an existing domain: add/remove sources, workflows, change system prompt, etc."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Domain to update" },
                "set_description": { "type": "string" },
                "add_sources": { "type": "array", "items": { "type": "object" } },
                "remove_sources": { "type": "array", "items": { "type": "string" }, "description": "Source kinds to remove" },
                "set_verification": { "type": "object" },
                "add_workflows": { "type": "array", "items": { "type": "object" } },
                "remove_workflows": { "type": "array", "items": { "type": "string" }, "description": "Workflow names to remove" },
                "set_system_prompt": { "type": "string" },
                "set_constraints": { "type": "object" },
                "set_enabled": { "type": "boolean" }
            },
            "required": ["name"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainUpdateInput =
            serde_json::from_str(input).context("domain_update expects valid JSON")?;

        let mut domain = DomainStore::load(&ctx.workspace_root, &req.name).await?;

        if let Some(desc) = req.set_description {
            domain.description = desc;
        }

        // Remove sources by kind.
        if !req.remove_sources.is_empty() {
            domain
                .sources
                .retain(|s| !req.remove_sources.contains(&s.kind));
        }

        // Add new sources.
        domain.sources.extend(req.add_sources);

        if let Some(v) = req.set_verification {
            domain.verification = v;
        }

        // Remove workflows by name.
        if !req.remove_workflows.is_empty() {
            domain
                .workflows
                .retain(|w| !req.remove_workflows.contains(&w.name));
        }

        // Add new workflows.
        domain.workflows.extend(req.add_workflows);

        if let Some(prompt) = req.set_system_prompt {
            domain.system_prompt = prompt;
        }

        if let Some(c) = req.set_constraints {
            domain.constraints = c;
        }

        if let Some(enabled) = req.set_enabled {
            domain.enabled = enabled;
        }

        domain.updated_at = now_iso8601();
        DomainStore::save(&ctx.workspace_root, &domain).await?;

        Ok(ToolResult {
            output: format!(
                "updated domain={} sources={} workflows={}",
                domain.name,
                domain.sources.len(),
                domain.workflows.len(),
            ),
        })
    }
}

// ============================================================================
// domain_list
// ============================================================================

#[derive(Debug, Default, Clone, Copy)]
pub struct DomainListTool;

#[async_trait]
impl Tool for DomainListTool {
    fn name(&self) -> &'static str {
        "domain_list"
    }

    fn description(&self) -> &'static str {
        "List all available research domains (project + global)."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }))
    }

    async fn execute(&self, _input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let domains = DomainStore::list(&ctx.workspace_root).await?;

        if domains.is_empty() {
            return Ok(ToolResult {
                output: "no domains found".to_string(),
            });
        }

        let mut lines = Vec::new();
        for d in &domains {
            let status = if d.enabled { "enabled" } else { "disabled" };
            lines.push(format!(
                "name={} status={} sources={} workflows={} — {}",
                d.name,
                status,
                d.sources.len(),
                d.workflows.len(),
                d.description,
            ));
        }

        Ok(ToolResult {
            output: lines.join("\n"),
        })
    }
}

// ============================================================================
// domain_info
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainInfoInput {
    name: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DomainInfoTool;

#[async_trait]
impl Tool for DomainInfoTool {
    fn name(&self) -> &'static str {
        "domain_info"
    }

    fn description(&self) -> &'static str {
        "Show the full configuration of a research domain."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Domain name to inspect" }
            },
            "required": ["name"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainInfoInput =
            serde_json::from_str(input).context("domain_info expects JSON: {\"name\": \"...\"}")?;

        let domain = DomainStore::load(&ctx.workspace_root, &req.name).await?;

        let json = serde_json::to_string_pretty(&domain).context("failed to serialize domain")?;

        Ok(ToolResult { output: json })
    }
}

// ============================================================================
// domain_search
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainSearchInput {
    domain: String,
    query: String,
    #[serde(default = "default_max_results")]
    max_results: usize,
    #[serde(default)]
    sources: Vec<String>,
}

fn default_max_results() -> usize {
    5
}

pub struct DomainSearchTool {
    client: Client,
}

impl DomainSearchTool {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl Default for DomainSearchTool {
    fn default() -> Self {
        Self::new(default_client())
    }
}

#[async_trait]
impl Tool for DomainSearchTool {
    fn name(&self) -> &'static str {
        "domain_search"
    }

    fn description(&self) -> &'static str {
        "Search across a domain's configured sources, merge and deduplicate results."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "string", "description": "Domain to search within" },
                "query": { "type": "string", "description": "Search query" },
                "max_results": { "type": "integer", "description": "Max results per source (default 5)" },
                "sources": { "type": "array", "items": { "type": "string" }, "description": "Optional filter: only search these source kinds" }
            },
            "required": ["domain", "query"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainSearchInput =
            serde_json::from_str(input).context("domain_search expects valid JSON")?;

        if req.query.trim().is_empty() {
            return Err(anyhow!("query must not be empty"));
        }

        let domain = DomainStore::load(&ctx.workspace_root, &req.domain).await?;

        // Filter and sort sources.
        let mut sources: Vec<&SourceConfig> = domain
            .sources
            .iter()
            .filter(|s| s.enabled)
            .filter(|s| req.sources.is_empty() || req.sources.contains(&s.kind))
            .collect();
        sources.sort_by_key(|s| s.priority);

        if sources.is_empty() {
            return Ok(ToolResult {
                output: "no enabled sources match the filter".to_string(),
            });
        }

        // Search each source.
        let mut all_results = Vec::new();
        let mut errors = Vec::new();
        for source in &sources {
            match search_source(&self.client, source, &req.query, req.max_results).await {
                Ok(results) => all_results.push(results),
                Err(e) => errors.push(format!("{}: {e}", source.label)),
            }
        }

        let merged = merge_results(all_results);

        // Append any relevant lessons as context hints.
        let lessons_hint = match LessonStore::load(&ctx.workspace_root).await {
            Ok(store) => {
                let lessons = store.recall(&req.domain, None, 3);
                if lessons.is_empty() {
                    String::new()
                } else {
                    let hints: Vec<String> = lessons
                        .iter()
                        .map(|l| format!("  - [{}] {}", l.lesson_type, l.content))
                        .collect();
                    format!("\n\nRelevant lessons:\n{}", hints.join("\n"))
                }
            }
            Err(_) => String::new(),
        };

        // Format output.
        let mut output = String::new();
        if merged.is_empty() && errors.is_empty() {
            output.push_str("no results found");
        } else {
            for (i, r) in merged.iter().enumerate() {
                output.push_str(&format!(
                    "{}. {} [{}]\n   {}\n   {}\n",
                    i + 1,
                    r.title,
                    r.source_kind,
                    r.url,
                    r.snippet
                ));
                if !r.authors.is_empty() {
                    output.push_str(&format!("   Authors: {}\n", r.authors.join(", ")));
                }
                for (key, val) in &r.metadata {
                    output.push_str(&format!("   {key}: {val}\n"));
                }
                output.push('\n');
            }
        }

        if !errors.is_empty() {
            output.push_str(&format!(
                "\nSource errors:\n{}",
                errors
                    .iter()
                    .map(|e| format!("  - {e}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        output.push_str(&lessons_hint);

        Ok(ToolResult { output })
    }
}

// ============================================================================
// domain_verify
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainVerifyInput {
    domain: String,
    findings: Vec<FindingToVerify>,
}

pub struct DomainVerifyTool {
    client: Client,
}

impl DomainVerifyTool {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl Default for DomainVerifyTool {
    fn default() -> Self {
        Self::new(default_client())
    }
}

#[async_trait]
impl Tool for DomainVerifyTool {
    fn name(&self) -> &'static str {
        "domain_verify"
    }

    fn description(&self) -> &'static str {
        "Verify research findings using a domain's configured verification strategies (existence check, metadata match, DOI resolution, cross-reference)."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "string", "description": "Domain whose verification config to use" },
                "findings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "authors": { "type": "array", "items": { "type": "string" } },
                            "year": { "type": "integer" },
                            "doi": { "type": "string" },
                            "arxiv_id": { "type": "string" },
                            "url": { "type": "string" },
                            "claimed_content": { "type": "string" }
                        },
                        "required": ["title"]
                    }
                }
            },
            "required": ["domain", "findings"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainVerifyInput =
            serde_json::from_str(input).context("domain_verify expects valid JSON")?;

        if req.findings.is_empty() {
            return Err(anyhow!("findings must not be empty"));
        }

        let domain = DomainStore::load(&ctx.workspace_root, &req.domain).await?;

        let mut results = Vec::new();
        for finding in &req.findings {
            let result = verify_finding(&self.client, &domain, finding).await;
            results.push(result);
        }

        // Format output.
        let mut output = String::new();
        for r in &results {
            output.push_str(&format!(
                "- {} — status={} confidence={:.2}\n",
                r.title, r.status, r.confidence
            ));
            for detail in &r.details {
                output.push_str(&format!("    {detail}\n"));
            }
        }

        Ok(ToolResult { output })
    }
}

async fn verify_finding(
    client: &Client,
    domain: &Domain,
    finding: &FindingToVerify,
) -> VerificationResult {
    let mut details = Vec::new();
    let mut checks_passed = 0u32;
    let mut checks_total = 0u32;

    for strategy in &domain.verification.strategies {
        checks_total += 1;
        match strategy {
            VerificationStrategy::ExistenceCheck => {
                if let Some(ref url) = finding.url {
                    match client.head(url).send().await {
                        Ok(resp)
                            if resp.status().is_success() || resp.status().is_redirection() =>
                        {
                            checks_passed += 1;
                            details
                                .push(format!("existence_check: URL resolves ({})", resp.status()));
                        }
                        Ok(resp) => {
                            details
                                .push(format!("existence_check: URL returned {}", resp.status()));
                        }
                        Err(e) => {
                            details.push(format!("existence_check: failed — {e}"));
                        }
                    }
                } else {
                    details.push("existence_check: no URL provided".to_string());
                }
            }
            VerificationStrategy::DoiResolve => {
                if let Some(ref doi) = finding.doi {
                    let doi_url = if doi.starts_with("http") {
                        doi.clone()
                    } else {
                        format!("https://doi.org/{doi}")
                    };
                    match client.head(&doi_url).send().await {
                        Ok(resp)
                            if resp.status().is_success() || resp.status().is_redirection() =>
                        {
                            checks_passed += 1;
                            details.push("doi_resolve: DOI resolves successfully".to_string());
                        }
                        Ok(resp) => {
                            details.push(format!("doi_resolve: DOI returned {}", resp.status()));
                        }
                        Err(e) => {
                            details.push(format!("doi_resolve: failed — {e}"));
                        }
                    }
                } else {
                    details.push("doi_resolve: no DOI provided".to_string());
                }
            }
            VerificationStrategy::MetadataMatch { fields } => {
                // Search Semantic Scholar by title to verify metadata.
                let encoded = url::form_urlencoded::byte_serialize(finding.title.as_bytes())
                    .collect::<String>();
                let api_url = format!(
                    "https://api.semanticscholar.org/graph/v1/paper/search?query={encoded}&limit=1&fields=title,authors,year"
                );
                match client.get(&api_url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(body) = resp.json::<serde_json::Value>().await {
                            if let Some(paper) = body
                                .get("data")
                                .and_then(|d| d.as_array())
                                .and_then(|a| a.first())
                            {
                                let mut field_matches = 0;
                                for field in fields {
                                    match field.as_str() {
                                        "title" => {
                                            if let Some(api_title) =
                                                paper.get("title").and_then(|v| v.as_str())
                                            {
                                                if titles_match(&finding.title, api_title) {
                                                    field_matches += 1;
                                                } else {
                                                    details.push(format!(
                                                        "metadata_match: title mismatch — API has \"{api_title}\""
                                                    ));
                                                }
                                            }
                                        }
                                        "year" => {
                                            if let (Some(claimed), Some(api_year)) = (
                                                finding.year,
                                                paper.get("year").and_then(|v| v.as_u64()),
                                            ) {
                                                if claimed as u64 == api_year {
                                                    field_matches += 1;
                                                } else {
                                                    details.push(format!(
                                                        "metadata_match: year mismatch — claimed {claimed}, API has {api_year}"
                                                    ));
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                if field_matches > 0 {
                                    checks_passed += 1;
                                    details.push(format!(
                                        "metadata_match: {field_matches}/{} fields matched",
                                        fields.len()
                                    ));
                                }
                            } else {
                                details.push("metadata_match: no matching paper found".to_string());
                            }
                        }
                    }
                    Ok(resp) => {
                        details.push(format!("metadata_match: API returned {}", resp.status()));
                    }
                    Err(e) => {
                        details.push(format!("metadata_match: request failed — {e}"));
                    }
                }
            }
            VerificationStrategy::CrossReference { min_sources } => {
                // Search multiple sources and check if the paper appears.
                let mut found_in = 0u32;

                // Check Semantic Scholar.
                let encoded = url::form_urlencoded::byte_serialize(finding.title.as_bytes())
                    .collect::<String>();
                let ss_url = format!(
                    "https://api.semanticscholar.org/graph/v1/paper/search?query={encoded}&limit=1&fields=title"
                );
                if let Ok(resp) = client.get(&ss_url).send().await {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if body
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|a| !a.is_empty())
                            .unwrap_or(false)
                        {
                            found_in += 1;
                        }
                    }
                }

                // Check OpenAlex.
                let oa_url = format!(
                    "https://api.openalex.org/works?filter=default.search:{encoded}&per_page=1"
                );
                if let Ok(resp) = client.get(&oa_url).send().await {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if body
                            .get("results")
                            .and_then(|r| r.as_array())
                            .map(|a| !a.is_empty())
                            .unwrap_or(false)
                        {
                            found_in += 1;
                        }
                    }
                }

                if found_in >= (*min_sources as u32) {
                    checks_passed += 1;
                    details.push(format!(
                        "cross_reference: found in {found_in} sources (required {min_sources})"
                    ));
                } else {
                    details.push(format!(
                        "cross_reference: found in only {found_in} sources (required {min_sources})"
                    ));
                }
            }
            VerificationStrategy::ContentSpotCheck { .. } => {
                details.push("content_spot_check: skipped (requires abstract fetch)".to_string());
            }
            VerificationStrategy::CustomHttp {
                url_template,
                expected_status,
            } => {
                let url = url_template
                    .replace("{{title}}", &finding.title)
                    .replace("{{doi}}", finding.doi.as_deref().unwrap_or(""));
                match client.get(&url).send().await {
                    Ok(resp) => {
                        if resp.status().as_u16() == *expected_status {
                            checks_passed += 1;
                            details.push(format!(
                                "custom_http: got expected status {expected_status}"
                            ));
                        } else {
                            details.push(format!(
                                "custom_http: expected {expected_status}, got {}",
                                resp.status()
                            ));
                        }
                    }
                    Err(e) => {
                        details.push(format!("custom_http: request failed — {e}"));
                    }
                }
            }
        }
    }

    let confidence = if checks_total > 0 {
        checks_passed as f64 / checks_total as f64
    } else {
        0.0
    };

    let status = if checks_total == 0 {
        VerificationStatus::Partial
    } else if confidence >= domain.verification.min_confidence {
        VerificationStatus::Verified
    } else if checks_passed > 0 {
        VerificationStatus::Partial
    } else {
        VerificationStatus::NotFound
    };

    VerificationResult {
        title: finding.title.clone(),
        status,
        confidence,
        details,
    }
}

fn titles_match(a: &str, b: &str) -> bool {
    let normalize = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ")
    };
    normalize(a) == normalize(b)
}

// ============================================================================
// domain_workflow
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainWorkflowInput {
    domain: String,
    workflow: String,
    #[serde(default)]
    sop_id: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DomainWorkflowTool;

#[async_trait]
impl Tool for DomainWorkflowTool {
    fn name(&self) -> &'static str {
        "domain_workflow"
    }

    fn description(&self) -> &'static str {
        "Create an SOP from a domain's workflow template."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "string", "description": "Domain containing the workflow template" },
                "workflow": { "type": "string", "description": "Workflow template name" },
                "sop_id": { "type": "string", "description": "Custom SOP ID (auto-generated if omitted)" }
            },
            "required": ["domain", "workflow"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainWorkflowInput =
            serde_json::from_str(input).context("domain_workflow expects valid JSON")?;

        let domain = DomainStore::load(&ctx.workspace_root, &req.domain).await?;

        let template = domain
            .workflows
            .iter()
            .find(|w| w.name == req.workflow)
            .ok_or_else(|| {
                let available: Vec<&str> =
                    domain.workflows.iter().map(|w| w.name.as_str()).collect();
                anyhow!(
                    "workflow '{}' not found in domain '{}'. Available: {}",
                    req.workflow,
                    req.domain,
                    available.join(", ")
                )
            })?;

        let sop_id = req
            .sop_id
            .unwrap_or_else(|| format!("{}-{}", req.domain, req.workflow));

        // Create the SOP via the existing sop_execute tool pattern.
        let sop_input = serde_json::json!({
            "id": sop_id,
            "steps": template.steps,
            "approval_required": template.approval_required,
        });

        SopExecuteTool.execute(&sop_input.to_string(), ctx).await
    }
}

// ============================================================================
// domain_learn
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainLearnInput {
    domain: String,
    lesson_type: String,
    content: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DomainLearnTool;

#[async_trait]
impl Tool for DomainLearnTool {
    fn name(&self) -> &'static str {
        "domain_learn"
    }

    fn description(&self) -> &'static str {
        "Capture a lesson learned from working with a domain (e.g., source quality, query strategy, verification insight)."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "string", "description": "Domain this lesson relates to" },
                "lesson_type": { "type": "string", "description": "Category: source_quality, query_strategy, verification_insight, domain_knowledge" },
                "content": { "type": "string", "description": "The lesson learned" }
            },
            "required": ["domain", "lesson_type", "content"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainLearnInput =
            serde_json::from_str(input).context("domain_learn expects valid JSON")?;

        if req.content.trim().is_empty() {
            return Err(anyhow!("lesson content must not be empty"));
        }

        let mut store = LessonStore::load(&ctx.workspace_root).await?;
        let id = store.add_lesson(&req.domain, &req.lesson_type, &req.content);
        store.save(&ctx.workspace_root).await?;

        Ok(ToolResult {
            output: format!(
                "captured lesson id={id} domain={} type={}",
                req.domain, req.lesson_type
            ),
        })
    }
}

// ============================================================================
// domain_lessons
// ============================================================================

#[derive(Debug, Deserialize)]
struct DomainLessonsInput {
    domain: String,
    #[serde(default)]
    lesson_type: Option<String>,
    #[serde(default = "default_lesson_limit")]
    max_results: usize,
}

fn default_lesson_limit() -> usize {
    10
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DomainLessonsTool;

#[async_trait]
impl Tool for DomainLessonsTool {
    fn name(&self) -> &'static str {
        "domain_lessons"
    }

    fn description(&self) -> &'static str {
        "Recall lessons learned for a domain, sorted by relevance (with time-decay)."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "domain": { "type": "string", "description": "Domain to recall lessons for" },
                "lesson_type": { "type": "string", "description": "Optional filter by lesson type" },
                "max_results": { "type": "integer", "description": "Max lessons to return (default 10)" }
            },
            "required": ["domain"]
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DomainLessonsInput =
            serde_json::from_str(input).context("domain_lessons expects valid JSON")?;

        let mut store = LessonStore::load(&ctx.workspace_root).await?;

        // Recall lessons and collect their data before mutating the store.
        let recalled: Vec<(String, String, String, String, u32, String)> = {
            let lessons = store.recall(&req.domain, req.lesson_type.as_deref(), req.max_results);
            lessons
                .iter()
                .map(|l| {
                    (
                        l.id.clone(),
                        l.lesson_type.clone(),
                        l.content.clone(),
                        l.domain.clone(),
                        l.use_count,
                        l.created_at.clone(),
                    )
                })
                .collect()
        };

        if recalled.is_empty() {
            return Ok(ToolResult {
                output: "no lessons found".to_string(),
            });
        }

        // Increment use_count for recalled lessons.
        for (id, ..) in &recalled {
            store.increment_use_count(id);
        }
        store.save(&ctx.workspace_root).await?;

        let mut lines = Vec::new();
        for (_, lesson_type, content, domain, use_count, created_at) in &recalled {
            lines.push(format!(
                "- [{lesson_type}] {content} (domain={domain}, uses={use_count}, created={created_at})"
            ));
        }

        Ok(ToolResult {
            output: lines.join("\n"),
        })
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn now_iso8601() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple ISO 8601 without chrono dependency.
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let remaining = now % secs_per_day;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    // Calculate year/month/day from days since epoch.
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;
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
            "agentzero-domain-tools-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn domain_create_and_list() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = DomainCreateTool
            .execute(
                r#"{"name": "test-domain", "description": "A test domain", "sources": [{"kind": "web_search", "label": "Web"}]}"#,
                &ctx,
            )
            .await
            .expect("create should succeed");
        assert!(result.output.contains("created domain=test-domain"));
        assert!(result.output.contains("sources=1"));

        let result = DomainListTool
            .execute("{}", &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("name=test-domain"));
        assert!(result.output.contains("enabled"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn domain_create_rejects_duplicate() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        DomainCreateTool
            .execute(r#"{"name": "dup", "description": "First"}"#, &ctx)
            .await
            .expect("first create should succeed");

        let err = DomainCreateTool
            .execute(r#"{"name": "dup", "description": "Second"}"#, &ctx)
            .await
            .expect_err("duplicate should fail");
        assert!(err.to_string().contains("already exists"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn domain_update_adds_source() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        DomainCreateTool
            .execute(r#"{"name": "updatable", "description": "To update"}"#, &ctx)
            .await
            .expect("create should succeed");

        let result = DomainUpdateTool
            .execute(
                r#"{"name": "updatable", "add_sources": [{"kind": "arxiv", "label": "arXiv"}]}"#,
                &ctx,
            )
            .await
            .expect("update should succeed");
        assert!(result.output.contains("sources=1"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn domain_info_returns_json() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        DomainCreateTool
            .execute(r#"{"name": "info-test", "description": "For info"}"#, &ctx)
            .await
            .expect("create should succeed");

        let result = DomainInfoTool
            .execute(r#"{"name": "info-test"}"#, &ctx)
            .await
            .expect("info should succeed");
        assert!(result.output.contains("\"name\": \"info-test\""));
        assert!(result.output.contains("\"description\": \"For info\""));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn domain_list_empty() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = DomainListTool
            .execute("{}", &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("no domains found"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn domain_workflow_creates_sop() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        DomainCreateTool
            .execute(
                r#"{
                    "name": "wf-test",
                    "description": "Workflow test",
                    "workflows": [{
                        "name": "review",
                        "description": "A review",
                        "steps": ["Search", "Analyze", "Report"],
                        "approval_required": [1]
                    }]
                }"#,
                &ctx,
            )
            .await
            .expect("create should succeed");

        let result = DomainWorkflowTool
            .execute(r#"{"domain": "wf-test", "workflow": "review"}"#, &ctx)
            .await
            .expect("workflow should succeed");
        assert!(result.output.contains("created sop=wf-test-review"));
        assert!(result.output.contains("steps=3"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn domain_workflow_not_found() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        DomainCreateTool
            .execute(r#"{"name": "no-wf", "description": "No workflows"}"#, &ctx)
            .await
            .expect("create should succeed");

        let err = DomainWorkflowTool
            .execute(r#"{"domain": "no-wf", "workflow": "nonexistent"}"#, &ctx)
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("not found"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn now_iso8601_format() {
        let ts = now_iso8601();
        assert!(ts.contains("T"));
        assert!(ts.ends_with("Z"));
        assert_eq!(ts.len(), 20);
    }

    #[test]
    fn titles_match_case_insensitive() {
        assert!(titles_match(
            "Attention Is All You Need",
            "attention is all you need"
        ));
    }

    #[test]
    fn titles_match_ignores_punctuation() {
        assert!(titles_match("GPT-4: A Large Model", "GPT4 A Large Model"));
    }
}
