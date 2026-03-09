# agent-browser

Playwright-based browser automation CLI for AgentZero. This is the backend for the `browser` tool — it accepts JSON actions via `--action` and drives a headless Chromium browser.

## Install

```bash
cd scripts/agent-browser
npm install
```

This automatically installs Chromium via Playwright's `postinstall` script.

## Usage

```bash
# Navigate to a page
agent-browser --action '{"action":"navigate","url":"https://example.com"}'

# Extract text with CSS selector
agent-browser --action '{"action":"get_text","selector":"h1"}'

# Take a screenshot
agent-browser --action '{"action":"screenshot"}'

# Get page snapshot (all visible text)
agent-browser --action '{"action":"snapshot"}'

# Close the browser
agent-browser --action '{"action":"close"}'
```

## Actions

| Action | Fields | Description |
|--------|--------|-------------|
| `navigate` | `url` | Navigate to a URL |
| `snapshot` | — | Get all visible text on the page |
| `click` | `selector` | Click an element |
| `fill` | `selector`, `value` | Fill an input field |
| `type` | `selector`, `text` | Type text into an element |
| `get_text` | `selector` | Extract text from elements matching selector |
| `get_title` | — | Get the page title |
| `get_url` | — | Get the current URL |
| `screenshot` | `path` (optional) | Save a screenshot |
| `wait` | `selector` or `ms` | Wait for element or time |
| `press` | `key` | Press a keyboard key |
| `hover` | `selector` | Hover over an element |
| `scroll` | `direction` (`up`/`down`) | Scroll the page |
| `close` | — | Close the browser |

## Configuration

In `agentzero.toml`:

```toml
[browser]
enabled = true
agent_browser_command = "./scripts/agent-browser/index.js"
```

Or add `scripts/agent-browser` to your PATH and use the default command name:

```bash
npm link  # from scripts/agent-browser/
```

## How It Works

Each invocation launches a fresh Chromium browser with a persistent user data directory (`~/.agent-browser/user-data/`). This means cookies and localStorage survive across calls, but page content does not — the Rust `BrowserTool` handles sequencing by issuing `navigate` before other actions. Use the `close` action to clean up stored state.

## Test

```bash
npm test
```
