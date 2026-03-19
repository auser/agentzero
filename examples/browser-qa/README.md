# Browser Automation / QA

Three-agent swarm for automated browser testing and quality assurance.

## Agents

| Agent | Role | Key Tools |
|-------|------|-----------|
| **test-planner** | Create structured test plans from URL/description | web_search, write_file |
| **browser-runner** | Execute tests via browser, capture screenshots | browser_tool, screenshot, shell |
| **report-generator** | Compile results into QA report with pass/fail summary | read_file, write_file |

## Pipeline

```
test-planner → browser-runner → report-generator
```

## Quick Start

```bash
export ANTHROPIC_API_KEY=your-key-here
agentzero gateway --config examples/browser-qa/agentzero.toml
```

Then send a test request:
```
Test the login flow and navigation at https://example.com
```

## Output

- Test plan (markdown)
- Test results with PASS/FAIL per case
- Screenshots at key interaction points
- QA report with summary, failures, and recommendations
