use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 65536;

#[derive(Debug, Clone)]
pub struct CodeInterpreterConfig {
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    pub allowed_languages: Vec<String>,
}

impl Default for CodeInterpreterConfig {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            allowed_languages: vec!["python".into(), "javascript".into()],
        }
    }
}

#[tool(
    name = "code_interpreter",
    description = "Execute Python or JavaScript code in a sandboxed subprocess. Returns stdout, stderr, exit code, and paths to any output files. Use for data analysis, computation, chart generation, and scripting tasks."
)]
#[derive(Default)]
pub struct CodeInterpreterTool {
    config: CodeInterpreterConfig,
}

impl CodeInterpreterTool {
    pub fn new(config: CodeInterpreterConfig) -> Self {
        Self { config }
    }

    fn runtime_for_language(lang: &str) -> anyhow::Result<(&'static str, &'static str)> {
        match lang {
            "python" => Ok(("python3", ".py")),
            "javascript" => Ok(("node", ".js")),
            _ => Err(anyhow!(
                "unsupported language: {lang}. Allowed: python, javascript"
            )),
        }
    }

    fn sandbox_dir(workspace_root: &str) -> PathBuf {
        PathBuf::from(workspace_root)
            .join(".agentzero")
            .join("sandbox")
    }

    fn output_dir(workspace_root: &str) -> PathBuf {
        Self::sandbox_dir(workspace_root).join("output")
    }

    async fn read_limited<R>(mut reader: R, max_bytes: usize) -> anyhow::Result<(Vec<u8>, bool)>
    where
        R: AsyncRead + Unpin,
    {
        let mut bytes = Vec::new();
        let mut limited = (&mut reader).take((max_bytes + 1) as u64);
        limited
            .read_to_end(&mut bytes)
            .await
            .context("failed to capture output")?;

        let truncated = bytes.len() > max_bytes;
        if truncated {
            bytes.truncate(max_bytes);
        }
        Ok((bytes, truncated))
    }

    fn render_stream(name: &str, bytes: &[u8], truncated: bool, max_bytes: usize) -> String {
        let mut out = format!("{name}:\n{}", String::from_utf8_lossy(bytes));
        if truncated {
            out.push_str(&format!("\n<truncated at {max_bytes} bytes>"));
        }
        out
    }

    fn collect_output_files(output_dir: &PathBuf) -> Vec<String> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(output_dir) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type() {
                    if ft.is_file() {
                        files.push(entry.path().display().to_string());
                    }
                }
            }
        }
        files.sort();
        files
    }
}

#[derive(Debug, ToolSchema, Deserialize)]
#[allow(dead_code)]
struct InterpreterInput {
    /// Programming language: "python" or "javascript"
    #[schema(enum_values = ["python", "javascript"])]
    language: String,
    /// The code to execute
    code: String,
}

#[async_trait]
impl Tool for CodeInterpreterTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(InterpreterInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: InterpreterInput = serde_json::from_str(input)
            .context("code_interpreter expects JSON with \"language\" and \"code\" fields")?;

        if !self
            .config
            .allowed_languages
            .iter()
            .any(|l| l == &parsed.language)
        {
            return Err(anyhow!(
                "language '{}' is not allowed. Allowed: {:?}",
                parsed.language,
                self.config.allowed_languages
            ));
        }

        let (runtime, ext) = Self::runtime_for_language(&parsed.language)?;

        let sandbox_dir = Self::sandbox_dir(&ctx.workspace_root);
        let output_dir = Self::output_dir(&ctx.workspace_root);
        tokio::fs::create_dir_all(&sandbox_dir)
            .await
            .context("failed to create sandbox directory")?;
        tokio::fs::create_dir_all(&output_dir)
            .await
            .context("failed to create output directory")?;

        // Clear previous output files.
        if let Ok(entries) = std::fs::read_dir(&output_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }

        // Write code to a temp file.
        let script_path = sandbox_dir.join(format!("script{ext}"));
        tokio::fs::write(&script_path, &parsed.code)
            .await
            .context("failed to write script file")?;

        let mut cmd = Command::new(runtime);
        if parsed.language == "python" {
            cmd.arg("-u"); // unbuffered output
        }
        cmd.arg(&script_path);
        cmd.current_dir(&sandbox_dir);
        cmd.env("INTERPRETER_OUTPUT_DIR", &output_dir);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn {runtime}. Is it installed and on PATH?"))?;

        let stdout_reader = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("stdout not piped"))?;
        let stderr_reader = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("stderr not piped"))?;

        let max_out = self.config.max_output_bytes;
        let stdout_task = tokio::spawn(Self::read_limited(stdout_reader, max_out));
        let stderr_task = tokio::spawn(Self::read_limited(stderr_reader, max_out));

        let timeout = tokio::time::Duration::from_millis(self.config.timeout_ms);
        let status = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(result) => result.context("code execution failed")?,
            Err(_) => {
                let _ = child.kill().await;
                return Ok(ToolResult {
                    output: format!(
                        "ERROR: execution timed out after {}ms",
                        self.config.timeout_ms
                    ),
                });
            }
        };

        let (stdout, stdout_truncated) = stdout_task
            .await
            .context("failed joining stdout capture task")??;
        let (stderr, stderr_truncated) = stderr_task
            .await
            .context("failed joining stderr capture task")??;

        let output_files = Self::collect_output_files(&output_dir);

        let mut result = format!(
            "status={}\n{}\n{}",
            status,
            Self::render_stream("stdout", &stdout, stdout_truncated, max_out),
            Self::render_stream("stderr", &stderr, stderr_truncated, max_out),
        );

        if !output_files.is_empty() {
            result.push_str("\n\nOutput files:\n");
            for f in &output_files {
                result.push_str(&format!("  {f}\n"));
            }
        }

        // Clean up script file.
        let _ = tokio::fs::remove_file(&script_path).await;

        Ok(ToolResult { output: result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;
    use std::sync::atomic::{AtomicU64, Ordering};

    static CTX_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn ctx() -> ToolContext {
        let n = CTX_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!(
                "agentzero-code-interp-test-{}-{}",
                std::process::id(),
                n
            ))
            .display()
            .to_string();
        ToolContext::new(dir)
    }

    #[test]
    fn rejects_invalid_json() {
        let tool = CodeInterpreterTool::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute("not json", &ctx()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("code_interpreter expects JSON"));
    }

    #[test]
    fn rejects_unsupported_language() {
        let tool = CodeInterpreterTool::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(tool.execute(r#"{"language": "ruby", "code": "puts 'hello'"}"#, &ctx()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not allowed"));
    }

    #[tokio::test]
    #[cfg_attr(target_os = "windows", ignore)]
    async fn executes_python_hello() {
        let tool = CodeInterpreterTool::default();
        let result = tool
            .execute(
                r#"{"language": "python", "code": "print('hello from python')"}"#,
                &ctx(),
            )
            .await
            .expect("python should execute");
        assert!(result.output.contains("hello from python"));
        assert!(result.output.contains("status=exit status: 0"));
    }

    #[tokio::test]
    async fn enforces_timeout() {
        let tool = CodeInterpreterTool::new(CodeInterpreterConfig {
            timeout_ms: 500,
            ..Default::default()
        });
        let result = tool
            .execute(
                r#"{"language": "python", "code": "import time; time.sleep(60)"}"#,
                &ctx(),
            )
            .await
            .expect("timeout should not be an error, but a result");
        assert!(result.output.contains("timed out"));
    }

    #[tokio::test]
    async fn captures_stderr() {
        let tool = CodeInterpreterTool::default();
        let result = tool
            .execute(
                r#"{"language": "python", "code": "import sys; sys.stderr.write('err msg')"}"#,
                &ctx(),
            )
            .await
            .expect("should succeed");
        assert!(result.output.contains("err msg"));
    }

    #[tokio::test]
    async fn truncates_large_output() {
        let tool = CodeInterpreterTool::new(CodeInterpreterConfig {
            max_output_bytes: 32,
            ..Default::default()
        });
        let result = tool
            .execute(
                r#"{"language": "python", "code": "print('x' * 1000)"}"#,
                &ctx(),
            )
            .await
            .expect("should succeed");
        assert!(result.output.contains("<truncated at 32 bytes>"));
    }
}
