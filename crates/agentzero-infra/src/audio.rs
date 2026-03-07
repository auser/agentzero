//! Audio marker processing for AgentZero.
//!
//! Handles `[AUDIO:path]` markers in user messages: parses them, and—when an
//! audio config with an API key is present—transcribes each file via a
//! Whisper-compatible endpoint before the message reaches the LLM.
//!
//! The transcribe-first approach keeps providers audio-agnostic: audio is
//! always converted to text before the agent sees it.

use agentzero_config::AudioConfig;
use std::path::Path;

// ─────────────────────────────────────────────────────────────────────────────
// Marker prefix
// ─────────────────────────────────────────────────────────────────────────────

const MARKER_PREFIX: &str = "[AUDIO:";

// ─────────────────────────────────────────────────────────────────────────────
// Data types
// ─────────────────────────────────────────────────────────────────────────────

/// A parsed audio reference extracted from a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRef {
    /// File path of the audio file.
    pub path: String,
    /// Byte offset where the `[AUDIO:…]` marker begins.
    pub start: usize,
    /// Byte offset immediately after the closing `]`.
    pub end: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Parse all `[AUDIO:<path>]` markers from `text`.
pub fn parse_audio_markers(text: &str) -> Vec<AudioRef> {
    let mut refs = Vec::new();
    let mut search_from = 0;

    while search_from < text.len() {
        let start = match text[search_from..].find(MARKER_PREFIX) {
            Some(pos) => search_from + pos,
            None => break,
        };

        let content_start = start + MARKER_PREFIX.len();
        let end = match text[content_start..].find(']') {
            Some(pos) => content_start + pos + 1,
            None => break, // Unclosed marker — stop scanning
        };

        let path = text[content_start..end - 1].trim().to_string();
        if !path.is_empty() {
            refs.push(AudioRef { path, start, end });
        }

        search_from = end;
    }

    refs
}

/// Remove all `[AUDIO:…]` markers from `text`, collapsing extra whitespace.
pub fn strip_audio_markers(text: &str) -> String {
    let refs = parse_audio_markers(text);
    if refs.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    for r in &refs {
        result.push_str(&text[last_end..r.start]);
        last_end = r.end;
    }
    result.push_str(&text[last_end..]);

    // Collapse whitespace left by removed markers
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ─────────────────────────────────────────────────────────────────────────────
// Transcription
// ─────────────────────────────────────────────────────────────────────────────

/// Replace `[AUDIO:…]` markers with `[Transcription of audio]: <text>`.
///
/// - If `config` is `None` or has no `api_key`, markers are stripped and a
///   warning is logged.
/// - Each audio file is transcribed via the Whisper-compatible endpoint
///   configured in `config.api_url`.
pub async fn process_audio_markers(
    text: &str,
    config: Option<&AudioConfig>,
) -> anyhow::Result<String> {
    let refs = parse_audio_markers(text);
    if refs.is_empty() {
        return Ok(text.to_string());
    }

    let cfg = match config {
        Some(c) => c,
        None => {
            tracing::warn!(
                markers = refs.len(),
                "audio markers found but no [audio] config; stripping markers"
            );
            return Ok(strip_audio_markers(text));
        }
    };

    let api_key = match cfg.api_key.as_deref() {
        Some(k) if !k.is_empty() => k.to_string(),
        _ => {
            tracing::warn!(
                markers = refs.len(),
                "audio markers found but no API key configured; stripping markers"
            );
            return Ok(strip_audio_markers(text));
        }
    };

    // Process markers in reverse order so byte offsets remain valid as we
    // replace each one.
    let mut result = text.to_string();
    for r in refs.iter().rev() {
        let path = r.path.clone();
        let api_url = cfg.api_url.clone();
        let api_key_c = api_key.clone();
        let language = cfg.language.clone();
        let model = cfg.model.clone();

        let transcript =
            transcribe_audio_async(&path, &api_url, &api_key_c, language.as_deref(), &model)
                .await
                .map_err(|e| anyhow::anyhow!("failed to transcribe '{}': {e}", r.path))?;

        let replacement = format!("[Transcription of audio]: {transcript}");
        result.replace_range(r.start..r.end, &replacement);
    }

    Ok(result)
}

/// Transcribe an audio file via a Whisper-compatible multipart API.
async fn transcribe_audio_async(
    path: &str,
    api_url: &str,
    api_key: &str,
    language: Option<&str>,
    model: &str,
) -> anyhow::Result<String> {
    let audio_bytes = tokio::fs::read(path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read audio file '{}': {e}", path))?;

    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();

    let file_part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name(filename)
        .mime_str("audio/octet-stream")
        .map_err(|e| anyhow::anyhow!("failed to build multipart part: {e}"))?;

    let mut form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", model.to_string())
        .text("response_format", "text");

    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }

    let client = reqwest::Client::new();
    let response = client
        .post(api_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("transcription request to '{api_url}' failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "transcription API returned {status}: {body}"
        ));
    }

    let transcript = response
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("failed to read transcription response: {e}"))?;

    Ok(transcript.trim().to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_marker() {
        let refs = parse_audio_markers("Transcribe this [AUDIO:/tmp/voice.wav] please");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/tmp/voice.wav");
    }

    #[test]
    fn parse_multiple_markers() {
        let text = "[AUDIO:/a.wav] and [AUDIO:/b.mp3]";
        let refs = parse_audio_markers(text);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "/a.wav");
        assert_eq!(refs[1].path, "/b.mp3");
    }

    #[test]
    fn parse_no_markers_returns_empty() {
        assert!(parse_audio_markers("no markers here").is_empty());
    }

    #[test]
    fn parse_empty_marker_skipped() {
        assert!(parse_audio_markers("[AUDIO:]").is_empty());
    }

    #[test]
    fn parse_unclosed_marker_skipped() {
        assert!(parse_audio_markers("[AUDIO:/broken").is_empty());
    }

    #[test]
    fn strip_removes_marker_and_collapses_whitespace() {
        let text = "Hello [AUDIO:/tmp/a.wav] world";
        assert_eq!(strip_audio_markers(text), "Hello world");
    }

    #[test]
    fn strip_no_markers_unchanged() {
        assert_eq!(strip_audio_markers("hello world"), "hello world");
    }

    #[test]
    fn strip_multiple_markers() {
        let text = "[AUDIO:/a.wav] text [AUDIO:/b.mp3]";
        assert_eq!(strip_audio_markers(text), "text");
    }

    #[tokio::test]
    async fn process_no_markers_returns_original() {
        let result = process_audio_markers("plain text", None).await.unwrap();
        assert_eq!(result, "plain text");
    }

    #[tokio::test]
    async fn process_markers_no_config_strips_them() {
        let result = process_audio_markers("Hello [AUDIO:/a.wav] world", None)
            .await
            .unwrap();
        assert_eq!(result, "Hello world");
    }

    #[tokio::test]
    async fn process_markers_no_api_key_strips_them() {
        let config = AudioConfig {
            api_key: None,
            ..AudioConfig::default()
        };
        let result = process_audio_markers("Hello [AUDIO:/a.wav] world", Some(&config))
            .await
            .unwrap();
        assert_eq!(result, "Hello world");
    }

    #[tokio::test]
    async fn process_markers_empty_api_key_strips_them() {
        let config = AudioConfig {
            api_key: Some(String::new()),
            ..AudioConfig::default()
        };
        let result = process_audio_markers("Hello [AUDIO:/a.wav] world", Some(&config))
            .await
            .unwrap();
        assert_eq!(result, "Hello world");
    }

    #[tokio::test]
    async fn process_markers_missing_file_returns_error() {
        let config = AudioConfig {
            api_key: Some("test-key".to_string()),
            ..AudioConfig::default()
        };
        let err = process_audio_markers("Hello [AUDIO:/nonexistent/file.wav] world", Some(&config))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("failed to transcribe"),
            "expected transcription error, got: {err}"
        );
    }
}
