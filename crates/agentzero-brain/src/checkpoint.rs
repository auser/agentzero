//! `brain checkpoint` — safe git checkpointing.

use crate::{validate_path, BrainConfig, BrainError, BrainFs};

/// Options for the checkpoint command.
pub struct CheckpointOptions {
    /// Custom commit message.
    pub message: Option<String>,
    /// Initialize a git repo if none exists.
    pub init: bool,
    /// Show what would happen without executing.
    pub dry_run: bool,
}

/// Result of a checkpoint operation.
pub struct CheckpointResult {
    /// Summary of what happened (or would happen).
    pub summary: String,
    /// The git commands to run.
    pub commands: Vec<String>,
    /// Whether a git repo was detected.
    pub has_git: bool,
}

/// Generate git checkpoint commands for the vault.
pub fn brain_checkpoint(
    fs: &dyn BrainFs,
    root: &str,
    _config: &BrainConfig,
    opts: &CheckpointOptions,
) -> Result<CheckpointResult, BrainError> {
    validate_path(root)?;

    let git_path = format!("{root}/.git");
    let has_git = fs.file_exists(&git_path).unwrap_or(false);

    let now = fs.now();
    let date = now.split('T').next().unwrap_or(&now);

    if !has_git && !opts.init {
        return Ok(CheckpointResult {
            summary: format!(
                "No git repository found at {root}. Run `az brain checkpoint --init` or `git init {root}` to create one."
            ),
            commands: vec![format!("git init {root}")],
            has_git: false,
        });
    }

    let mut commands = Vec::new();

    if !has_git && opts.init {
        commands.push(format!("cd {root}"));
        commands.push("git init".to_string());
    } else {
        commands.push(format!("cd {root}"));
    }

    let default_msg = format!("Brain: checkpoint {date}");
    let commit_msg = opts.message.as_deref().unwrap_or(&default_msg);

    commands.push("git add -A".to_string());
    commands.push(format!("git commit -m \"{commit_msg}\""));

    let prefix = if opts.dry_run { "[dry-run] " } else { "" };

    let summary = if !has_git && opts.init {
        format!(
            "{prefix}Would initialize git and checkpoint at {root} with message: \"{commit_msg}\""
        )
    } else if opts.dry_run {
        format!("{prefix}Would checkpoint at {root} with message: \"{commit_msg}\"")
    } else {
        format!(
            "Checkpoint ready. Run these commands:\n\n{}",
            commands.join("\n")
        )
    };

    Ok(CheckpointResult {
        summary,
        commands,
        has_git,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init::{brain_init, InitOptions};
    use crate::tests::TestFs;

    fn setup_vault(fs: &TestFs) -> BrainConfig {
        let config = BrainConfig::default();
        let opts = InitOptions::default();
        brain_init(fs, "/vault", &config, &opts).expect("init");
        config
    }

    #[test]
    fn detects_non_git_repo() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let opts = CheckpointOptions {
            message: None,
            init: false,
            dry_run: false,
        };
        let result = brain_checkpoint(&fs, "/vault", &config, &opts).expect("checkpoint");
        assert!(!result.has_git);
        assert!(result.summary.contains("No git repository"));
    }

    #[test]
    fn generates_correct_git_commands() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        // Simulate git repo
        fs.set_file("/vault/.git", "");
        let opts = CheckpointOptions {
            message: None,
            init: false,
            dry_run: false,
        };
        let result = brain_checkpoint(&fs, "/vault", &config, &opts).expect("checkpoint");
        assert!(result.has_git);
        assert!(result.commands.iter().any(|c| c.contains("git add -A")));
        assert!(result.commands.iter().any(|c| c.contains("git commit")));
        assert!(result
            .commands
            .iter()
            .any(|c| c.contains("Brain: checkpoint")));
    }

    #[test]
    fn includes_custom_message() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/.git", "");
        let opts = CheckpointOptions {
            message: Some("my custom message".to_string()),
            init: false,
            dry_run: false,
        };
        let result = brain_checkpoint(&fs, "/vault", &config, &opts).expect("checkpoint");
        assert!(result
            .commands
            .iter()
            .any(|c| c.contains("my custom message")));
    }

    #[test]
    fn dry_run_produces_output_without_writes() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        fs.set_file("/vault/.git", "");
        let files_before = fs.files();
        let opts = CheckpointOptions {
            message: None,
            init: false,
            dry_run: true,
        };
        let result = brain_checkpoint(&fs, "/vault", &config, &opts).expect("checkpoint");
        assert!(result.summary.contains("[dry-run]"));
        let files_after = fs.files();
        assert_eq!(files_before, files_after);
    }

    #[test]
    fn init_generates_git_init_command() {
        let fs = TestFs::new();
        let config = setup_vault(&fs);
        let opts = CheckpointOptions {
            message: None,
            init: true,
            dry_run: false,
        };
        let result = brain_checkpoint(&fs, "/vault", &config, &opts).expect("checkpoint");
        assert!(!result.has_git);
        assert!(result.commands.iter().any(|c| c.contains("git init")));
    }
}
