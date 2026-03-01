use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ImageInfoInput {
    path: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ImageInfoTool;

impl ImageInfoTool {
    fn resolve_path(input_path: &str, workspace_root: &str) -> anyhow::Result<PathBuf> {
        if input_path.trim().is_empty() {
            return Err(anyhow!("path is required"));
        }
        let relative = Path::new(input_path);
        if relative.is_absolute() {
            return Err(anyhow!("absolute paths are not allowed"));
        }
        if relative
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(anyhow!("path traversal is not allowed"));
        }
        let joined = Path::new(workspace_root).join(relative);
        let canonical_root = Path::new(workspace_root)
            .canonicalize()
            .context("unable to resolve workspace root")?;
        let canonical = joined
            .canonicalize()
            .with_context(|| format!("file not found: {input_path}"))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(anyhow!("path is outside workspace"));
        }
        Ok(canonical)
    }

    fn detect_format(header: &[u8]) -> &'static str {
        if header.starts_with(b"\x89PNG") {
            "PNG"
        } else if header.starts_with(b"\xFF\xD8\xFF") {
            "JPEG"
        } else if header.starts_with(b"GIF8") {
            "GIF"
        } else if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"WEBP" {
            "WebP"
        } else if header.starts_with(b"BM") {
            "BMP"
        } else {
            "unknown"
        }
    }

    fn png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
        if data.len() < 24 {
            return None;
        }
        let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        Some((width, height))
    }

    fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
        let mut i = 2;
        while i + 9 < data.len() {
            if data[i] != 0xFF {
                return None;
            }
            let marker = data[i + 1];
            if marker == 0xC0 || marker == 0xC2 {
                let height = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let width = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                return Some((width, height));
            }
            let seg_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            i += 2 + seg_len;
        }
        None
    }

    fn gif_dimensions(data: &[u8]) -> Option<(u32, u32)> {
        if data.len() < 10 {
            return None;
        }
        let width = u16::from_le_bytes([data[6], data[7]]) as u32;
        let height = u16::from_le_bytes([data[8], data[9]]) as u32;
        Some((width, height))
    }
}

#[async_trait]
impl Tool for ImageInfoTool {
    fn name(&self) -> &'static str {
        "image_info"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: ImageInfoInput =
            serde_json::from_str(input).context("image_info expects JSON: {\"path\": \"...\"}")?;

        let file_path = Self::resolve_path(&req.path, &ctx.workspace_root)?;
        let data = tokio::fs::read(&file_path)
            .await
            .with_context(|| format!("failed to read file: {}", req.path))?;

        let format = Self::detect_format(&data);
        let dimensions = match format {
            "PNG" => Self::png_dimensions(&data),
            "JPEG" => Self::jpeg_dimensions(&data),
            "GIF" => Self::gif_dimensions(&data),
            _ => None,
        };

        let file_size = data.len();
        let mut output = format!("format={format}\nsize={file_size} bytes");
        if let Some((w, h)) = dimensions {
            output.push_str(&format!("\nwidth={w}\nheight={h}"));
        }

        Ok(ToolResult { output })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-image-info-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn image_info_detects_png() {
        let dir = temp_dir();
        // Minimal valid PNG header (8-byte magic + 13-byte IHDR chunk with 1x1 dims)
        let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        // IHDR length (13)
        png.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D]);
        // "IHDR"
        png.extend_from_slice(b"IHDR");
        // width=100, height=50
        png.extend_from_slice(&100u32.to_be_bytes());
        png.extend_from_slice(&50u32.to_be_bytes());
        // bit depth, color type, compression, filter, interlace
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        fs::write(dir.join("test.png"), &png).unwrap();

        let tool = ImageInfoTool;
        let result = tool
            .execute(
                r#"{"path": "test.png"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("should succeed");
        assert!(result.output.contains("format=PNG"));
        assert!(result.output.contains("width=100"));
        assert!(result.output.contains("height=50"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn image_info_rejects_path_traversal() {
        let dir = temp_dir();
        let tool = ImageInfoTool;
        let err = tool
            .execute(
                r#"{"path": "../escape.png"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("path traversal should fail");
        assert!(err.to_string().contains("path traversal"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn image_info_non_image_file() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "hello world").unwrap();
        let tool = ImageInfoTool;
        let result = tool
            .execute(
                r#"{"path": "test.txt"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("should succeed even for non-image");
        assert!(result.output.contains("format=unknown"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn detect_format_from_headers() {
        assert_eq!(ImageInfoTool::detect_format(b"\x89PNG\r\n\x1a\n"), "PNG");
        assert_eq!(ImageInfoTool::detect_format(b"\xFF\xD8\xFF\xE0"), "JPEG");
        assert_eq!(ImageInfoTool::detect_format(b"GIF89a"), "GIF");
        assert_eq!(ImageInfoTool::detect_format(b"hello"), "unknown");
    }
}
