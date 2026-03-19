# OSINT / Research Analyst

Five-agent swarm for open-source intelligence gathering and analysis.

## Agents

| Agent | Role | Key Tools |
|-------|------|-----------|
| **source-finder** | Identify and prioritize relevant sources | web_search, web_fetch |
| **data-collector** | Fetch and extract structured data from sources | web_fetch, http_request, write_file |
| **fact-checker** | Cross-reference claims against independent sources | web_search, web_fetch |
| **analyst** | Synthesize verified facts into analytical insights | memory_recall, read_file |
| **report-writer** | Compile findings into a professional intelligence brief | write_file, memory_recall |

## Pipeline

```
source-finder → data-collector → fact-checker → analyst → report-writer
```

## Quick Start

```bash
export ANTHROPIC_API_KEY=your-key-here
agentzero gateway --config examples/osint-analyst/agentzero.toml
```

Then send a research query:
```
Investigate recent developments in quantum computing startups
```

## Output

The report-writer produces a markdown intelligence brief with:
- Executive Summary
- Key Findings
- Detailed Analysis
- Sources
- Recommendations
