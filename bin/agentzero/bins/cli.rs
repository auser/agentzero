use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    agentzero_security::redaction::install_redacting_panic_hook();
    let _ = agentzero_security::policy::baseline_version();

    match agentzero_cli::parse_cli_from(std::env::args_os()) {
        Ok(cli) => match agentzero_cli::execute(cli).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!(
                    "error: {}",
                    agentzero_security::redaction::redact_error_chain(err.as_ref())
                );
                ExitCode::from(1)
            }
        },
        Err(err) => {
            if err.use_stderr() {
                let _ = err.print();
                ExitCode::from(2)
            } else {
                let _ = err.print();
                ExitCode::SUCCESS
            }
        }
    }
}
