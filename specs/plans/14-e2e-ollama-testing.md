# Sprint 42 Phase E: E2E Testing with Local LLM (Ollama)

## Setup

1. Checkout `feat/e2e-ollama` branch from `main`
2. Keep `specs/SPRINT.md` up-to-date throughout implementation

## Context

Sprint 42 is complete except Phase E (E2E Testing with Local LLM). Ollama is installed locally with `llama3.2:latest` (2GB). Tests must not break normal CI (`just test`).

## Gating strategy: `#[ignore]` + runtime health check

- All Ollama tests use `#[ignore]` -- skipped by `cargo test` and `just test`
- A `require_ollama()` helper checks `http://localhost:11434/api/tags` -- if Ollama is down, test prints diagnostic and returns early (graceful degradation)
- Run locally: `just test-ollama`
- Run in CI: separate `e2e-ollama` job installs Ollama + pulls model

## Files to create/modify

| File | Change |
|------|--------|
| `crates/agentzero-infra/tests/e2e_ollama.rs` | **New**: 5 test functions + helpers |
| `.config/nextest.toml` | Add `ollama` test group (serial, 60s timeout) |
| `justfile` | Add `test-ollama` recipe |
| `.github/workflows/ci.yml` | Add `e2e-ollama` job |
| `specs/SPRINT.md` | Mark Phase E items done |

## Test functions (all `#[tokio::test]` + `#[ignore]`)

1. **`ollama_basic_completion`** -- `provider.complete("What is 2+2? Reply with just the number.")` -> response contains "4"
2. **`ollama_streaming_completion`** -- Stream "Count from 1 to 5", assert 2+ chunks received, accumulated text contains digits
3. **`ollama_tool_use`** -- `RuntimeExecution` with Ollama + `EchoTool`, prompt forces tool call, assert tool was invoked
4. **`ollama_multi_turn_conversation`** -- Turn 1: "My favorite color is blue", Turn 2: "What is my favorite color?" -> contains "blue"
5. **`ollama_router_classification`** -- `AgentRouter` with Ollama, routes "review my PR" to `code-review` agent (not `image-gen`)

## Helpers (top of `e2e_ollama.rs`)

```rust
const OLLAMA_MODEL: &str = "llama3.2:latest";
const OLLAMA_BASE_URL: &str = "http://localhost:11434";

fn ollama_provider() -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(OLLAMA_BASE_URL.into(), String::new(), OLLAMA_MODEL.into())
}

async fn require_ollama() -> bool {
    // GET http://localhost:11434/api/tags with 2s timeout
    // Returns false + eprintln if unavailable
}
```

## Nextest config addition

```toml
[test-groups.ollama]
max-threads = 1  # Ollama handles one inference at a time

[[profile.default.overrides]]
filter = "test(e2e_ollama)"
test-group = "ollama"
slow-timeout = { period = "60s", terminate-after = 2 }
```

## CI workflow addition (`.github/workflows/ci.yml`)

New `e2e-ollama` job (not blocking `checks`):
- Install Ollama via `curl -fsSL https://ollama.com/install.sh | sh`
- `ollama serve &` + `ollama pull llama3.2:latest`
- `cargo nextest run --workspace --run-ignored ignored-only -E 'test(e2e_ollama)' --profile ci`

## Justfile recipe

```just
# Run Ollama e2e tests (requires Ollama running locally)
test-ollama:
    cargo nextest run --workspace --run-ignored ignored-only -E 'test(e2e_ollama)'
```

## Verification

1. `just test` -- 2407+ tests pass, Ollama tests skipped (no impact)
2. `just test-ollama` -- 5 tests pass (with Ollama running)
3. `cargo clippy --workspace --all-targets -- -D warnings` -- 0 warnings
4. Update `specs/SPRINT.md` Phase E items to `[x]`
