//! CLI commands for `.azb` model bundle management.

use crate::cli::BundleCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;
use std::path::Path;

pub struct BundleCommand;

#[async_trait]
impl AgentZeroCommand for BundleCommand {
    type Options = BundleCommands;

    async fn run(_ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            BundleCommands::Create {
                source_dir,
                model_id,
                version,
                target,
                backend,
                output,
            } => run_create(&source_dir, &model_id, &version, &target, &backend, output),
            BundleCommands::Verify { bundle, public_key } => {
                run_verify(&bundle, public_key.as_deref())
            }
            BundleCommands::Install { bundle } => run_install(&bundle),
        }
    }
}

fn run_create(
    source_dir: &str,
    model_id: &str,
    version: &str,
    target: &str,
    backend: &str,
    output: Option<String>,
) -> anyhow::Result<()> {
    let source = Path::new(source_dir);
    if !source.is_dir() {
        anyhow::bail!("source directory does not exist: {source_dir}");
    }

    let output_path = output.unwrap_or_else(|| format!("{model_id}-{version}.azb"));
    let output_path = Path::new(&output_path);

    let manifest = agentzero_providers::bundle::create_bundle(
        source,
        model_id,
        version,
        target,
        backend,
        output_path,
    )?;

    eprintln!(
        "\x1b[1;32m✓ Bundle created:\x1b[0m {}",
        output_path.display()
    );
    eprintln!("  model_id: {}", manifest.model_id);
    eprintln!("  version:  {}", manifest.version);
    eprintln!("  target:   {}", manifest.target);
    eprintln!("  backend:  {}", manifest.backend);
    eprintln!("  files:    {}", manifest.files.len());
    for f in &manifest.files {
        eprintln!("    {} ({}, {})", f.path, f.role, &f.sha256[..12]);
    }

    Ok(())
}

fn run_verify(bundle_path: &str, public_key: Option<&str>) -> anyhow::Result<()> {
    let path = Path::new(bundle_path);
    let bundle = agentzero_providers::bundle::load_bundle(path)?;

    eprintln!("\x1b[1;32m✓ Bundle valid:\x1b[0m {bundle_path}");
    eprintln!("  model_id: {}", bundle.manifest.model_id);
    eprintln!("  version:  {}", bundle.manifest.version);
    eprintln!("  target:   {}", bundle.manifest.target);
    eprintln!("  backend:  {}", bundle.manifest.backend);
    eprintln!(
        "  files:    {} (all checksums verified)",
        bundle.manifest.files.len()
    );

    let key = public_key.unwrap_or("");
    let status = agentzero_providers::bundle::verify_signature(&bundle.manifest, key)?;
    match status {
        agentzero_providers::bundle::SignatureStatus::Valid => {
            eprintln!("  signature: \x1b[1;32mvalid\x1b[0m");
        }
        agentzero_providers::bundle::SignatureStatus::Unsigned => {
            eprintln!("  signature: unsigned");
        }
        agentzero_providers::bundle::SignatureStatus::Invalid(reason) => {
            eprintln!("  signature: \x1b[1;31minvalid\x1b[0m ({reason})");
        }
    }

    Ok(())
}

fn run_install(bundle_path: &str) -> anyhow::Result<()> {
    let path = Path::new(bundle_path);
    let bundle = agentzero_providers::bundle::load_bundle(path)?;

    let models_dir = agentzero_providers::bundle::models_dir()?;
    let dest = agentzero_providers::bundle::extract_bundle(&bundle, &models_dir)?;

    eprintln!("\x1b[1;32m✓ Bundle installed:\x1b[0m {}", dest.display());
    eprintln!("  model_id: {}", bundle.manifest.model_id);
    eprintln!("  version:  {}", bundle.manifest.version);
    eprintln!("  files:    {}", bundle.manifest.files.len());

    Ok(())
}
