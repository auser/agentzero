use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

const DEFAULT_TIMEOUT_MS: u64 = 60_000;

// ---------------------------------------------------------------------------
// Common types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MediaEndpointConfig {
    pub api_url: String,
    pub api_key_env: String,
    pub timeout_ms: u64,
}

fn media_output_dir(workspace_root: &str) -> PathBuf {
    PathBuf::from(workspace_root)
        .join(".agentzero")
        .join("media")
}

fn resolve_api_key(env_var: &str) -> anyhow::Result<String> {
    std::env::var(env_var).with_context(|| format!("API key env var {env_var} is not set"))
}

fn output_filename(prefix: &str, ext: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{prefix}_{ts}.{ext}")
}

// ---------------------------------------------------------------------------
// TTS Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TtsConfig {
    pub endpoint: MediaEndpointConfig,
    pub model: String,
    pub default_voice: String,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            endpoint: MediaEndpointConfig {
                api_url: "https://api.openai.com/v1/audio/speech".into(),
                api_key_env: "OPENAI_API_KEY".into(),
                timeout_ms: DEFAULT_TIMEOUT_MS,
            },
            model: "tts-1".into(),
            default_voice: "alloy".into(),
        }
    }
}

#[tool(
    name = "tts",
    description = "Convert text to speech audio. Saves the audio file to the workspace and returns the file path."
)]
pub struct TtsTool {
    client: reqwest::Client,
    config: TtsConfig,
}

impl Default for TtsTool {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            config: TtsConfig::default(),
        }
    }
}

impl TtsTool {
    pub fn new(config: TtsConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }
}

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct TtsInput {
    /// The text to convert to speech
    text: String,
    /// Voice to use (e.g. "alloy", "echo", "fable", "onyx", "nova", "shimmer")
    #[serde(default)]
    voice: Option<String>,
    /// Audio format: mp3, wav, opus, aac, flac
    #[schema(enum_values = ["mp3", "wav", "opus", "aac", "flac"])]
    #[serde(default = "default_audio_format")]
    format: String,
}

fn default_audio_format() -> String {
    "mp3".into()
}

#[async_trait]
impl Tool for TtsTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(TtsInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: TtsInput =
            serde_json::from_str(input).context("tts expects JSON with \"text\" field")?;

        if parsed.text.trim().is_empty() {
            return Err(anyhow!("text must not be empty"));
        }

        let api_key = resolve_api_key(&self.config.endpoint.api_key_env)?;
        let voice = parsed
            .voice
            .unwrap_or_else(|| self.config.default_voice.clone());

        let body = serde_json::json!({
            "model": self.config.model,
            "input": parsed.text,
            "voice": voice,
            "response_format": parsed.format,
        });

        let timeout = std::time::Duration::from_millis(self.config.endpoint.timeout_ms);
        let resp = self
            .client
            .post(&self.config.endpoint.api_url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .context("TTS request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("TTS API returned {status}: {body}"));
        }

        let bytes = resp.bytes().await.context("failed to read TTS response")?;

        let out_dir = media_output_dir(&ctx.workspace_root);
        tokio::fs::create_dir_all(&out_dir)
            .await
            .context("failed to create media output directory")?;

        let filename = output_filename("tts", &parsed.format);
        let file_path = out_dir.join(&filename);
        tokio::fs::write(&file_path, &bytes)
            .await
            .context("failed to save audio file")?;

        Ok(ToolResult {
            output: format!(
                "Audio saved to: {}\nSize: {} bytes\nVoice: {voice}\nFormat: {}",
                file_path.display(),
                bytes.len(),
                parsed.format
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// Image Generation Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ImageGenConfig {
    pub endpoint: MediaEndpointConfig,
    pub model: String,
    pub default_size: String,
}

impl Default for ImageGenConfig {
    fn default() -> Self {
        Self {
            endpoint: MediaEndpointConfig {
                api_url: "https://api.openai.com/v1/images/generations".into(),
                api_key_env: "OPENAI_API_KEY".into(),
                timeout_ms: DEFAULT_TIMEOUT_MS,
            },
            model: "dall-e-3".into(),
            default_size: "1024x1024".into(),
        }
    }
}

#[tool(
    name = "image_generate",
    description = "Generate an image from a text prompt. Saves the image to the workspace and returns the file path."
)]
pub struct ImageGenTool {
    client: reqwest::Client,
    config: ImageGenConfig,
}

impl Default for ImageGenTool {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            config: ImageGenConfig::default(),
        }
    }
}

impl ImageGenTool {
    pub fn new(config: ImageGenConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }
}

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct ImageGenInput {
    /// Text description of the image to generate
    prompt: String,
    /// Image size: 1024x1024, 1792x1024, or 1024x1792
    #[schema(enum_values = ["1024x1024", "1792x1024", "1024x1792"])]
    #[serde(default)]
    size: Option<String>,
    /// Image style: natural or vivid
    #[schema(enum_values = ["natural", "vivid"])]
    #[serde(default)]
    style: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageGenResponse {
    data: Vec<ImageGenData>,
}

#[derive(Debug, Deserialize)]
struct ImageGenData {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    b64_json: Option<String>,
}

#[async_trait]
impl Tool for ImageGenTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(ImageGenInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: ImageGenInput = serde_json::from_str(input)
            .context("image_generate expects JSON with \"prompt\" field")?;

        if parsed.prompt.trim().is_empty() {
            return Err(anyhow!("prompt must not be empty"));
        }

        let api_key = resolve_api_key(&self.config.endpoint.api_key_env)?;
        let size = parsed
            .size
            .unwrap_or_else(|| self.config.default_size.clone());

        let mut body = serde_json::json!({
            "model": self.config.model,
            "prompt": parsed.prompt,
            "size": size,
            "n": 1,
            "response_format": "b64_json",
        });
        if let Some(style) = &parsed.style {
            body["style"] = serde_json::Value::String(style.clone());
        }

        let timeout = std::time::Duration::from_millis(self.config.endpoint.timeout_ms);
        let resp = self
            .client
            .post(&self.config.endpoint.api_url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .context("image generation request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Image generation API returned {status}: {body}"));
        }

        let gen_resp: ImageGenResponse = resp
            .json()
            .await
            .context("failed to parse image generation response")?;

        let data = gen_resp
            .data
            .first()
            .ok_or_else(|| anyhow!("no image data in response"))?;

        let out_dir = media_output_dir(&ctx.workspace_root);
        tokio::fs::create_dir_all(&out_dir)
            .await
            .context("failed to create media output directory")?;

        let filename = output_filename("image", "png");
        let file_path = out_dir.join(&filename);

        if let Some(b64) = &data.b64_json {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("failed to decode base64 image")?;
            tokio::fs::write(&file_path, &bytes)
                .await
                .context("failed to save image file")?;
        } else if let Some(url) = &data.url {
            let img_bytes = self
                .client
                .get(url)
                .timeout(timeout)
                .send()
                .await
                .context("failed to download image")?
                .bytes()
                .await
                .context("failed to read image bytes")?;
            tokio::fs::write(&file_path, &img_bytes)
                .await
                .context("failed to save image file")?;
        } else {
            return Err(anyhow!(
                "image response contained neither URL nor base64 data"
            ));
        }

        Ok(ToolResult {
            output: format!(
                "Image saved to: {}\nSize: {size}\nPrompt: {}",
                file_path.display(),
                parsed.prompt
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// Video Generation Tool
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VideoGenConfig {
    pub endpoint: MediaEndpointConfig,
    pub model: String,
    pub poll_interval_ms: u64,
    pub max_poll_attempts: u32,
}

impl Default for VideoGenConfig {
    fn default() -> Self {
        Self {
            endpoint: MediaEndpointConfig {
                api_url: "https://api.minimax.chat/v1/video_generation".into(),
                api_key_env: "MINIMAX_API_KEY".into(),
                timeout_ms: 300_000, // 5 min for video
            },
            model: "MiniMax-Hailuo-2.3".into(),
            poll_interval_ms: 5_000,
            max_poll_attempts: 60,
        }
    }
}

#[tool(
    name = "video_generate",
    description = "Generate a video from a text prompt. Submits the job and polls for completion. Saves the video to the workspace and returns the file path."
)]
pub struct VideoGenTool {
    client: reqwest::Client,
    config: VideoGenConfig,
}

impl Default for VideoGenTool {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            config: VideoGenConfig::default(),
        }
    }
}

impl VideoGenTool {
    pub fn new(config: VideoGenConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }
}

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct VideoGenInput {
    /// Text description of the video to generate
    prompt: String,
    /// Desired video duration in seconds (default: 5)
    #[serde(default)]
    duration_secs: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct VideoSubmitResponse {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VideoStatusResponse {
    #[serde(default)]
    status: String,
    #[serde(default, alias = "file_id")]
    video_url: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[async_trait]
impl Tool for VideoGenTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(VideoGenInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: VideoGenInput = serde_json::from_str(input)
            .context("video_generate expects JSON with \"prompt\" field")?;

        if parsed.prompt.trim().is_empty() {
            return Err(anyhow!("prompt must not be empty"));
        }

        let api_key = resolve_api_key(&self.config.endpoint.api_key_env)?;

        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": parsed.prompt,
            "duration": parsed.duration_secs.unwrap_or(5),
        });

        // Submit job.
        let resp = self
            .client
            .post(&self.config.endpoint.api_url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("video generation submit failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Video generation API returned {status}: {body}"));
        }

        let submit: VideoSubmitResponse = resp
            .json()
            .await
            .context("failed to parse video submit response")?;

        let task_id = submit
            .task_id
            .or(submit.id)
            .ok_or_else(|| anyhow!("no task_id in video generation response"))?;

        // Poll for completion.
        let poll_url = format!("{}/{task_id}", self.config.endpoint.api_url);
        let poll_interval = std::time::Duration::from_millis(self.config.poll_interval_ms);

        for attempt in 0..self.config.max_poll_attempts {
            if ctx.is_cancelled() {
                return Ok(ToolResult {
                    output: "Video generation cancelled.".to_string(),
                });
            }

            tokio::time::sleep(poll_interval).await;

            let poll_resp = self
                .client
                .get(&poll_url)
                .header("Authorization", format!("Bearer {api_key}"))
                .send()
                .await
                .with_context(|| format!("poll attempt {attempt} failed"))?;

            if !poll_resp.status().is_success() {
                continue;
            }

            let status_resp: VideoStatusResponse =
                poll_resp.json().await.unwrap_or(VideoStatusResponse {
                    status: "unknown".into(),
                    video_url: None,
                    error: None,
                });

            match status_resp.status.as_str() {
                "completed" | "success" | "done" => {
                    let video_url = status_resp
                        .video_url
                        .ok_or_else(|| anyhow!("video completed but no URL returned"))?;

                    let video_bytes = self
                        .client
                        .get(&video_url)
                        .send()
                        .await
                        .context("failed to download video")?
                        .bytes()
                        .await
                        .context("failed to read video bytes")?;

                    let out_dir = media_output_dir(&ctx.workspace_root);
                    tokio::fs::create_dir_all(&out_dir)
                        .await
                        .context("failed to create media output directory")?;

                    let filename = output_filename("video", "mp4");
                    let file_path = out_dir.join(&filename);
                    tokio::fs::write(&file_path, &video_bytes)
                        .await
                        .context("failed to save video file")?;

                    return Ok(ToolResult {
                        output: format!(
                            "Video saved to: {}\nSize: {} bytes\nPrompt: {}",
                            file_path.display(),
                            video_bytes.len(),
                            parsed.prompt
                        ),
                    });
                }
                "failed" | "error" => {
                    let err_msg = status_resp.error.unwrap_or_else(|| "unknown error".into());
                    return Err(anyhow!("video generation failed: {err_msg}"));
                }
                _ => continue, // still processing
            }
        }

        Err(anyhow!(
            "video generation timed out after {} poll attempts",
            self.config.max_poll_attempts
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;

    fn ctx() -> ToolContext {
        ToolContext::new(".".to_string())
    }

    #[test]
    fn tts_rejects_invalid_json() {
        let tool = TtsTool::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute("not json", &ctx()));
        assert!(result.is_err());
    }

    #[test]
    fn tts_rejects_empty_text() {
        let tool = TtsTool::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(r#"{"text": ""}"#, &ctx()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("text must not be empty"));
    }

    #[test]
    fn tts_rejects_missing_api_key() {
        // Temporarily ensure the env var is unset.
        let key = "OPENAI_API_KEY_TEST_NONEXISTENT_12345";
        let tool = TtsTool::new(TtsConfig {
            endpoint: MediaEndpointConfig {
                api_key_env: key.into(),
                ..TtsConfig::default().endpoint
            },
            ..TtsConfig::default()
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(r#"{"text": "hello"}"#, &ctx()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not set"));
    }

    #[test]
    fn image_gen_rejects_empty_prompt() {
        let tool = ImageGenTool::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(r#"{"prompt": ""}"#, &ctx()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("prompt must not be empty"));
    }

    #[test]
    fn video_gen_rejects_empty_prompt() {
        let tool = VideoGenTool::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(r#"{"prompt": ""}"#, &ctx()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("prompt must not be empty"));
    }
}
