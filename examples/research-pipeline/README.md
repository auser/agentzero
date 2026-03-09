# Research-to-Brief Pipeline

A multi-step AI pipeline that turns any topic into a polished research brief. Four specialized agents work in sequence, each using a model optimized for its role.

## Pipeline Flow

```
  "Research AI regulation in the EU"
                │
    ┌───────────▼───────────┐
    │     Researcher         │   Haiku 4.5 (fast, cheap)
    │  web_search + web_fetch│   → research/raw-findings.md
    └───────────┬───────────┘
                │
    ┌───────────▼───────────┐
    │      Scraper           │   Haiku 4.5 (fast)
    │  browser + web_fetch   │   → research/detailed-data.md
    └───────────┬───────────┘
                │
    ┌───────────▼───────────┐
    │      Analyst           │   Sonnet 4.6 (powerful)
    │  read + synthesize     │   → research/analysis.md
    └───────────┬───────────┘
                │
    ┌───────────▼───────────┐
    │      Writer            │   Sonnet 4.6 (powerful)
    │  read + write          │   → output/brief.md
    └───────────┘
```

## How It Works

1. **Researcher** searches the web using multiple queries, fetches promising results, and compiles raw findings with sources
2. **Scraper** deep-dives into the top URLs using browser automation to extract detailed metrics, quotes, and structured data
3. **Analyst** synthesizes everything into structured analysis with themes, insights, and recommendations
4. **Writer** produces a polished, publication-ready brief with executive summary, findings, and action items

### Model Strategy

| Agent | Model | Why |
|-------|-------|-----|
| Researcher | Haiku 4.5 | Fast + cheap — broad search doesn't need deep reasoning |
| Scraper | Haiku 4.5 | Fast — extraction is mechanical, not analytical |
| Analyst | Sonnet 4.6 | Powerful — synthesis requires deep reasoning |
| Writer | Sonnet 4.6 | Powerful — polished writing requires nuance |

This keeps costs low for the high-volume search phase while using powerful models where quality matters.

### Error Handling

The pipeline uses `on_step_error = "skip"` so that if the Scraper fails (e.g., `agent-browser` not installed), the pipeline continues with the Analyst using just the raw findings. This makes the browser step optional.

## Quick Start

```bash
# 1. Copy config
cp examples/research-pipeline/agentzero.toml ./agentzero.toml

# 2. Authenticate (one-time — opens browser for OAuth)
agentzero auth login --provider anthropic
# Or set an API key directly: export ANTHROPIC_API_KEY="sk-ant-api..."

# 3. Start the pipeline (agent-browser deps install automatically on first use)
agentzero gateway

# 4. Pair a client (in another terminal — use the pairing code shown at startup)
curl -X POST http://localhost:42617/pair -H "X-Pairing-Code: <code-from-startup>"
# Returns: {"paired":true,"token":"<your-bearer-token>"}

# 5. Send a research request:
curl -X POST http://localhost:42617/api/chat \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <your-bearer-token>" \
  -d '{"message": "Research the current state of AI regulation in the EU"}'
```

## Output

The pipeline produces files in your workspace:

```
research/
  raw-findings.md      # Researcher's web search results
  detailed-data.md     # Scraper's extracted data
  analysis.md          # Analyst's synthesis
output/
  brief.md             # Final polished brief
```

## Customization

### Adding a fact-checker step

Insert a fact-checker agent between Analyst and Writer:

```toml
[swarm.agents.factchecker]
name = "Fact Checker"
description = "Verifies claims and data points from the analysis against original sources."
keywords = ["verify", "fact-check"]
provider = "anthropic"
model = "claude-sonnet-4-6"
allowed_tools = ["read_file", "write_file", "web_fetch", "web_search"]
subscribes_to = ["task.analysis.complete"]
produces = ["task.factcheck.complete"]
max_iterations = 15
system_prompt = "You verify every claim in research/analysis.md against its cited source..."
```

Then update the pipeline steps:
```toml
steps = ["researcher", "scraper", "analyst", "factchecker", "writer"]
```

And update the Writer's `subscribes_to`:
```toml
subscribes_to = ["task.factcheck.complete"]
```

### Using local models for privacy

Route all analysis through a local model:

```toml
[swarm.agents.analyst]
provider = "ollama"
model = "llama3"
base_url = "http://localhost:11434"
privacy_boundary = "local_only"
```

### Changing search providers

For better search quality, use Brave or Perplexity:

```toml
[web_search]
provider = "brave"
# Set BRAVE_API_KEY in your .env
```

### Platform-specific scraping

The Scraper agent uses `agent-browser` (Playwright-based) for dynamic content. For platforms that block automation (TikTok, Instagram, etc.), consider:

1. Using an external scraping service via `http_request` tool
2. Using platform APIs where available (TikTok Research API, Meta Graph API)
3. Using the shell tool to run specialized scrapers (e.g., `yt-dlp` for video metadata)

### Template-free output formatting

Instead of a template engine, the Writer agent's system prompt defines the output format directly. To change the brief format, edit the Writer's `system_prompt` in the config. The LLM follows the format specification reliably — no template engine needed.

## Prerequisites

- **Required**: Anthropic account — run `agentzero auth login --provider anthropic` or set `ANTHROPIC_API_KEY`
- **Optional**: `agent-browser` for the Scraper step — npm dependencies install automatically on first use (pipeline works without it, requires Node.js + npm)
- **Optional**: Brave/Perplexity API key for better search results (DuckDuckGo works without keys)
