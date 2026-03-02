use crate::cli::RagCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;
use serde::Serialize;
use std::fs;

pub struct RagCommand;

#[async_trait]
impl AgentZeroCommand for RagCommand {
    type Options = RagCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            RagCommands::Ingest {
                id,
                text,
                file,
                json,
            } => {
                let content = resolve_ingest_text(text, file.as_deref())?;
                let kind = infer_media_kind(file.as_deref());
                let result = ingest(ctx, id, content).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&IngestOutput {
                            id: result.id,
                            indexed_chars: result.indexed_chars,
                            media_kind: kind,
                        })?
                    );
                } else {
                    println!(
                        "RAG ingest complete: id={} indexed_chars={} media_kind={}",
                        result.id, result.indexed_chars, kind
                    );
                }
            }
            RagCommands::Query { query, limit, json } => {
                let matches = query_index(ctx, &query, limit).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&matches)?);
                } else if matches.is_empty() {
                    println!("No RAG matches");
                } else {
                    println!("RAG matches ({}):", matches.len());
                    for m in matches {
                        println!("  - {}: {}", m.id, truncate(&m.text, 96));
                    }
                }
            }
        }

        Ok(())
    }
}

fn resolve_ingest_text(text: Option<String>, file: Option<&str>) -> anyhow::Result<String> {
    if let Some(text) = text {
        if !text.trim().is_empty() {
            return Ok(text);
        }
    }

    if let Some(file) = file {
        let content = fs::read_to_string(file)?;
        if content.trim().is_empty() {
            anyhow::bail!("rag ingest content cannot be empty");
        }
        return Ok(content);
    }

    anyhow::bail!("rag ingest requires non-empty --text or --file")
}

#[cfg(feature = "rag")]
async fn ingest(
    ctx: &CommandContext,
    id: String,
    text: String,
) -> anyhow::Result<agentzero_rag::RagIngestResult> {
    let index_path = ctx.data_dir.join("rag").join("index.jsonl");
    agentzero_rag::ingest_document(&index_path, agentzero_rag::RagDocument { id, text })
}

#[cfg(not(feature = "rag"))]
async fn ingest(_ctx: &CommandContext, _id: String, _text: String) -> anyhow::Result<IngestShim> {
    anyhow::bail!("rag command requested but agentzero-cli was built without `rag` feature")
}

#[cfg(not(feature = "rag"))]
#[derive(Debug)]
struct IngestShim {
    id: String,
    indexed_chars: usize,
}

#[cfg(feature = "rag")]
async fn query_index(
    ctx: &CommandContext,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<agentzero_rag::RagQueryMatch>> {
    let index_path = ctx.data_dir.join("rag").join("index.jsonl");
    agentzero_rag::query_documents(&index_path, query, limit)
}

#[cfg(not(feature = "rag"))]
async fn query_index(
    _ctx: &CommandContext,
    _query: &str,
    _limit: usize,
) -> anyhow::Result<Vec<QueryShim>> {
    anyhow::bail!("rag command requested but agentzero-cli was built without `rag` feature")
}

#[cfg(not(feature = "rag"))]
#[derive(Debug, Serialize)]
struct QueryShim {
    id: String,
    text: String,
}

fn truncate(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in input.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn infer_media_kind(file: Option<&str>) -> String {
    let Some(file) = file else {
        return "text".to_string();
    };

    #[cfg(feature = "rag")]
    {
        match agentzero_multimodal::infer_media_kind(file) {
            Ok(kind) => format!("{kind:?}").to_ascii_lowercase(),
            Err(_) => "unknown".to_string(),
        }
    }
    #[cfg(not(feature = "rag"))]
    {
        let _ = file;
        "unknown".to_string()
    }
}

#[derive(Debug, Serialize)]
struct IngestOutput {
    id: String,
    indexed_chars: usize,
    media_kind: String,
}

#[cfg(test)]
mod tests {
    use super::RagCommand;
    use crate::cli::RagCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-rag-cmd-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn rag_query_empty_index_success_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        #[cfg(feature = "rag")]
        {
            RagCommand::run(
                &ctx,
                RagCommands::Query {
                    query: "hello".to_string(),
                    limit: 5,
                    json: false,
                },
            )
            .await
            .expect("query should succeed");
        }

        #[cfg(not(feature = "rag"))]
        {
            let err = RagCommand::run(
                &ctx,
                RagCommands::Query {
                    query: "hello".to_string(),
                    limit: 5,
                    json: false,
                },
            )
            .await
            .expect_err("query should fail without rag feature");
            assert!(err.to_string().contains("built without `rag` feature"));
        }

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn rag_ingest_requires_text_or_file_negative_path() {
        let data_dir = temp_dir();
        let ctx = CommandContext {
            workspace_root: data_dir.clone(),
            data_dir: data_dir.clone(),
            config_path: data_dir.join("agentzero.toml"),
        };

        let err = RagCommand::run(
            &ctx,
            RagCommands::Ingest {
                id: "doc-1".to_string(),
                text: None,
                file: None,
                json: false,
            },
        )
        .await
        .expect_err("ingest without payload should fail");
        assert!(err
            .to_string()
            .contains("requires non-empty --text or --file"));

        fs::remove_dir_all(data_dir).expect("temp dir should be removed");
    }

    #[test]
    fn resolve_ingest_text_from_inline_success_path() {
        let text = super::resolve_ingest_text(Some("hello world".to_string()), None)
            .expect("inline text should resolve");
        assert_eq!(text, "hello world");
    }
}
