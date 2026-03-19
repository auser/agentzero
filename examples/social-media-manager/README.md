# Social Media Manager

Four-agent swarm for social media content strategy and production.

## Agents

| Agent | Role | Key Tools |
|-------|------|-----------|
| **content-strategist** | Research trends, develop content calendar | web_search, web_fetch, write_file |
| **copywriter** | Write platform-specific posts with A/B variants | read_file, write_file |
| **scheduler** | Optimize publishing schedule by platform | read_file, write_file |
| **analytics-reporter** | Compile executive summary with metrics estimates | read_file, write_file |

## Pipeline

```
content-strategist → copywriter → scheduler → analytics-reporter
```

## Quick Start

```bash
export ANTHROPIC_API_KEY=your-key-here
agentzero gateway --config examples/social-media-manager/agentzero.toml
```

Then send a campaign brief:
```
Create a week of social media content about our new product launch
```

## Output

The pipeline produces:
- Content strategy document
- Platform-specific posts (Twitter/X, LinkedIn, Instagram)
- Publishing schedule (CSV + markdown)
- Executive summary with recommendations
