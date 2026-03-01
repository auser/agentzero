use crate::cli::{Cli, CompletionShell};
use crate::command_core::{AgentZeroCommand, CommandContext};
use async_trait::async_trait;
use clap::CommandFactory;

pub struct CompletionsCommand;

#[async_trait]
impl AgentZeroCommand for CompletionsCommand {
    type Options = CompletionShell;

    async fn run(_ctx: &CommandContext, shell: Self::Options) -> anyhow::Result<()> {
        let script = generate_completion_script(shell);
        if script.trim().is_empty() {
            anyhow::bail!("generated empty completion script");
        }
        print!("{script}");
        Ok(())
    }
}

fn generate_completion_script(shell: CompletionShell) -> String {
    use clap_complete::{generate, shells};
    let mut cmd = Cli::command();
    let mut out = Vec::<u8>::new();

    match shell {
        CompletionShell::Bash => generate(shells::Bash, &mut cmd, "agentzero", &mut out),
        CompletionShell::Elvish => generate(shells::Elvish, &mut cmd, "agentzero", &mut out),
        CompletionShell::Fish => generate(shells::Fish, &mut cmd, "agentzero", &mut out),
        CompletionShell::PowerShell => {
            generate(shells::PowerShell, &mut cmd, "agentzero", &mut out)
        }
        CompletionShell::Zsh => generate(shells::Zsh, &mut cmd, "agentzero", &mut out),
    }

    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{generate_completion_script, CompletionsCommand};
    use crate::cli::CompletionShell;
    use crate::command_core::{AgentZeroCommand, CommandContext};

    #[tokio::test]
    async fn completions_command_generates_bash_success_path() {
        let ctx = CommandContext {
            workspace_root: std::env::temp_dir(),
            data_dir: std::env::temp_dir(),
            config_path: std::env::temp_dir().join("agentzero.toml"),
        };
        CompletionsCommand::run(&ctx, CompletionShell::Bash)
            .await
            .expect("bash completions should succeed");
    }

    #[test]
    fn completion_script_is_not_empty_negative_path() {
        let script = generate_completion_script(CompletionShell::Zsh);
        assert!(!script.trim().is_empty());
    }
}
