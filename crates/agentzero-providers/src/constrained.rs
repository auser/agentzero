//! Constrained decoding for local LLM providers.
//!
//! Uses `outlines-core` to guarantee that model output conforms to a JSON
//! schema by masking invalid tokens at each generation step. This eliminates
//! malformed tool calls from small models — the model can only produce tokens
//! that lead to valid JSON.
//!
//! The integration works by:
//! 1. Converting a JSON schema → regex via `outlines_core::json_schema`
//! 2. Building an FSA `Index` from the regex + model vocabulary
//! 3. At each generation step, querying `allowed_tokens(state)` to mask logits
//! 4. After sampling, advancing the state with `next_state(state, token_id)`

use anyhow::{Context, Result};
use candle_core::{Device, Tensor};
use outlines_core::prelude::*;
use tracing::debug;

/// A constrained decoder that ensures generated tokens conform to a JSON schema.
///
/// Built from a JSON schema string and a model vocabulary. The `Index` is
/// precomputed once and reused across generation steps.
pub struct ConstrainedDecoder {
    index: Index,
    state: StateId,
}

impl ConstrainedDecoder {
    /// Build a constrained decoder from a JSON schema and tokenizer vocabulary.
    ///
    /// This is the expensive step — it converts the schema to a regex, then
    /// builds a DFA and maps every vocabulary token to valid state transitions.
    /// Call this once per schema, not per generation.
    pub fn from_schema(schema: &str, tokenizer: &tokenizers::Tokenizer) -> Result<Self> {
        let regex = outlines_core::json_schema::regex_from_str(schema, None, None)
            .context("failed to convert JSON schema to regex")?;

        let vocab = build_vocabulary(tokenizer)?;

        let index =
            Index::new(&regex, &vocab).context("failed to build constrained decoding index")?;

        debug!(
            states = index.transitions().len(),
            vocab_size = index.vocab_size(),
            "built constrained decoding index"
        );

        Ok(Self {
            state: index.initial_state(),
            index,
        })
    }

    /// Build a constrained decoder from a pre-built regex pattern.
    pub fn from_regex(regex: &str, tokenizer: &tokenizers::Tokenizer) -> Result<Self> {
        let vocab = build_vocabulary(tokenizer)?;
        let index =
            Index::new(regex, &vocab).context("failed to build constrained decoding index")?;

        Ok(Self {
            state: index.initial_state(),
            index,
        })
    }

    /// Apply the constraint mask to logits, setting disallowed tokens to -inf.
    ///
    /// Returns the masked logits tensor. Only tokens that lead to valid states
    /// are kept; all others are set to `f32::NEG_INFINITY` so they have zero
    /// probability after softmax.
    pub fn mask_logits(&self, logits: &Tensor, device: &Device) -> Result<Tensor> {
        let allowed = self.index.allowed_tokens(&self.state);

        match allowed {
            Some(token_ids) if !token_ids.is_empty() => {
                let vocab_size = logits.dim(0)?;
                // Build a mask: -inf for disallowed, 0.0 for allowed
                let mut mask_data = vec![f32::NEG_INFINITY; vocab_size];
                for tid in token_ids {
                    let id = tid as usize;
                    if id < vocab_size {
                        mask_data[id] = 0.0;
                    }
                }
                let mask = Tensor::new(mask_data, device)?;
                let masked = logits.add(&mask)?;
                Ok(masked)
            }
            _ => {
                // No valid transitions from this state — return logits unchanged.
                // This can happen at the end of valid generation.
                Ok(logits.clone())
            }
        }
    }

    /// Advance the decoder state after sampling a token.
    ///
    /// Returns `true` if the state was advanced, `false` if the token was not
    /// a valid transition (which shouldn't happen if masking was applied).
    pub fn advance(&mut self, token_id: u32) -> bool {
        if let Some(next) = self.index.next_state(&self.state, &token_id) {
            self.state = next;
            true
        } else {
            false
        }
    }

    /// Check if the current state is a final (accepting) state.
    pub fn is_finished(&self) -> bool {
        self.index.is_final_state(&self.state)
    }

    /// Reset the decoder to the initial state (for reuse with a new generation).
    pub fn reset(&mut self) {
        self.state = self.index.initial_state();
    }
}

/// Build an `outlines_core::Vocabulary` from a HuggingFace tokenizer.
fn build_vocabulary(tokenizer: &tokenizers::Tokenizer) -> Result<Vocabulary> {
    let vocab_map = tokenizer.get_vocab(true);

    // Find the EOS token ID
    let eos_id = tokenizer
        .token_to_id("</s>")
        .or_else(|| tokenizer.token_to_id("<|im_end|>"))
        .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
        .or_else(|| tokenizer.token_to_id("<eos>"))
        .unwrap_or(2); // fallback to common default

    let mut vocab = Vocabulary::new(eos_id);
    for (token_str, id) in &vocab_map {
        if let Err(e) = vocab.try_insert(token_str.as_bytes().to_vec(), *id) {
            // Some tokens may fail to insert (duplicates); skip silently
            tracing::trace!(token = token_str, error = %e, "skipped vocabulary token");
        }
    }

    debug!(
        tokens = vocab.len(),
        eos_id = eos_id,
        "built vocabulary for constrained decoding"
    );

    Ok(vocab)
}

/// Build a JSON schema for tool call output format.
///
/// The schema ensures the model produces valid `{"name": "...", "arguments": {...}}`
/// objects matching one of the available tool definitions.
pub fn tool_call_schema(tool_names: &[&str]) -> String {
    if tool_names.is_empty() {
        // No tools — unconstrained
        return "{}".to_string();
    }

    let names_enum: Vec<String> = tool_names.iter().map(|n| format!("\"{}\"", n)).collect();

    format!(
        r#"{{
  "type": "object",
  "properties": {{
    "name": {{
      "type": "string",
      "enum": [{}]
    }},
    "arguments": {{
      "type": "object"
    }}
  }},
  "required": ["name", "arguments"]
}}"#,
        names_enum.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_schema_single_tool() {
        let schema = tool_call_schema(&["web_search"]);
        let v: serde_json::Value = serde_json::from_str(&schema).expect("valid JSON");
        assert_eq!(v["properties"]["name"]["enum"][0], "web_search");
        assert_eq!(v["required"][0], "name");
        assert_eq!(v["required"][1], "arguments");
    }

    #[test]
    fn tool_call_schema_multiple_tools() {
        let schema = tool_call_schema(&["web_search", "read_file", "shell"]);
        let v: serde_json::Value = serde_json::from_str(&schema).expect("valid JSON");
        let names = v["properties"]["name"]["enum"].as_array().expect("array");
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn tool_call_schema_empty_tools() {
        let schema = tool_call_schema(&[]);
        assert_eq!(schema, "{}");
    }

    #[test]
    fn tool_call_schema_produces_valid_json_schema() {
        let schema = tool_call_schema(&["shell"]);
        // Verify it can be parsed as a JSON schema by outlines-core
        let result = outlines_core::json_schema::regex_from_str(&schema, None, None);
        assert!(
            result.is_ok(),
            "schema should produce valid regex: {:?}",
            result.err()
        );
    }

    #[test]
    fn tool_call_schema_regex_matches_valid_json() {
        let schema = tool_call_schema(&["web_search", "shell"]);
        let regex =
            outlines_core::json_schema::regex_from_str(&schema, None, None).expect("valid regex");

        // The generated regex should be a valid regex that can be compiled
        let re = regex::Regex::new(&regex);
        assert!(re.is_ok(), "regex should compile: {:?}", re.err());

        // Valid tool call JSON should match the regex
        let re = re.expect("regex compiled");
        let valid = r#"{"name": "web_search", "arguments": {}}"#;
        assert!(re.is_match(valid), "should match valid tool call JSON");
    }

    #[test]
    fn tool_call_schema_many_tools() {
        // Stress test: schema with many tools should still produce valid regex
        let tools: Vec<&str> = (0..20)
            .map(|i| match i {
                0 => "web_search",
                1 => "read_file",
                2 => "shell",
                3 => "write_file",
                4 => "git_status",
                5 => "git_diff",
                6 => "list_dir",
                7 => "grep",
                8 => "find",
                9 => "curl",
                10 => "python",
                11 => "node",
                12 => "docker",
                13 => "kubectl",
                14 => "terraform",
                15 => "aws",
                16 => "gcloud",
                17 => "azure",
                18 => "ssh",
                _ => "rsync",
            })
            .collect();
        let schema = tool_call_schema(&tools);
        let result = outlines_core::json_schema::regex_from_str(&schema, None, None);
        assert!(
            result.is_ok(),
            "many-tool schema should produce valid regex"
        );
    }
}
