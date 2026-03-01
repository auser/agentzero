use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
struct DocxReadInput {
    path: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DocxReadTool;

impl DocxReadTool {
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

    fn extract_text_from_docx(path: &Path) -> anyhow::Result<String> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open file: {}", path.display()))?;
        let mut archive =
            zip::ZipArchive::new(file).context("file is not a valid DOCX (ZIP) archive")?;

        let mut document_xml = match archive.by_name("word/document.xml") {
            Ok(f) => f,
            Err(_) => return Err(anyhow!("word/document.xml not found in DOCX")),
        };

        let mut xml = String::new();
        document_xml
            .read_to_string(&mut xml)
            .context("failed to read document.xml")?;

        // Extract text from XML by stripping tags and collecting <w:t> content.
        let mut text = String::new();
        let mut in_paragraph = false;
        let mut paragraph_has_text = false;

        for token in xml.split('<') {
            if token.is_empty() {
                continue;
            }
            let (tag_part, rest) = token.split_once('>').unwrap_or((token, ""));

            if tag_part.starts_with("w:p ") || tag_part == "w:p" {
                if in_paragraph && paragraph_has_text {
                    text.push('\n');
                }
                in_paragraph = true;
                paragraph_has_text = false;
            } else if tag_part == "/w:p" {
                if in_paragraph && paragraph_has_text {
                    text.push('\n');
                }
                in_paragraph = false;
                paragraph_has_text = false;
            }

            if tag_part.starts_with("w:t") && !rest.is_empty() {
                text.push_str(rest);
                paragraph_has_text = true;
            }
        }

        if text.len() > MAX_OUTPUT_BYTES {
            text.truncate(MAX_OUTPUT_BYTES);
            text.push_str(&format!("\n<truncated at {} bytes>", MAX_OUTPUT_BYTES));
        }

        Ok(text)
    }
}

#[async_trait]
impl Tool for DocxReadTool {
    fn name(&self) -> &'static str {
        "docx_read"
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: DocxReadInput =
            serde_json::from_str(input).context("docx_read expects JSON: {\"path\": \"...\"}")?;

        let file_path = Self::resolve_path(&req.path, &ctx.workspace_root)?;

        let output = tokio::task::spawn_blocking(move || Self::extract_text_from_docx(&file_path))
            .await
            .context("docx extraction task panicked")??;

        if output.trim().is_empty() {
            Ok(ToolResult {
                output: "(no text content extracted)".to_string(),
            })
        } else {
            Ok(ToolResult { output })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
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
            "agentzero-docx-read-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn create_minimal_docx(dir: &Path, filename: &str, text: &str) -> PathBuf {
        let path = dir.join(filename);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("word/document.xml", options).unwrap();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body></w:document>"#
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    #[tokio::test]
    async fn docx_read_extracts_text() {
        let dir = temp_dir();
        create_minimal_docx(&dir, "test.docx", "Hello from DOCX");

        let tool = DocxReadTool;
        let result = tool
            .execute(
                r#"{"path": "test.docx"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect("should extract text");
        assert!(result.output.contains("Hello from DOCX"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn docx_read_rejects_path_traversal() {
        let dir = temp_dir();
        let tool = DocxReadTool;
        let err = tool
            .execute(
                r#"{"path": "../escape.docx"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("path traversal should fail");
        assert!(err.to_string().contains("path traversal"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn docx_read_rejects_non_docx_file() {
        let dir = temp_dir();
        fs::write(dir.join("test.txt"), "plain text").unwrap();
        let tool = DocxReadTool;
        let err = tool
            .execute(
                r#"{"path": "test.txt"}"#,
                &ToolContext::new(dir.to_string_lossy().to_string()),
            )
            .await
            .expect_err("non-docx should fail");
        assert!(err.to_string().contains("not a valid DOCX"));
        fs::remove_dir_all(dir).ok();
    }
}
