# Lead Generation

Four-agent swarm for B2B lead research and qualification.

## Agents

| Agent | Role | Key Tools |
|-------|------|-----------|
| **prospector** | Find companies matching ICP criteria | web_search, web_fetch |
| **enricher** | Enrich prospects with detailed company/contact data | web_fetch, http_request |
| **qualifier** | Score and prioritize leads (A/B/C/D) | read_file, write_file |
| **outreach-drafter** | Draft personalized email + LinkedIn outreach | read_file, write_file |

## Pipeline

```
prospector → enricher → qualifier → outreach-drafter
```

## Quick Start

```bash
export ANTHROPIC_API_KEY=your-key-here
agentzero gateway --config examples/lead-generation/agentzero.toml
```

Then send a prospecting query:
```
Find 10 SaaS startups in the AI space that recently raised Series A
```

## Output

- Prospect list with company details
- Enriched profiles (JSON) with decision-makers and tech stack
- Qualified lead rankings (A/B/C/D) with reasoning
- Personalized outreach drafts (email + LinkedIn) per lead
- Summary CSV with all leads
