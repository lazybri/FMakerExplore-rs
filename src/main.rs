use std::process::ExitCode;

use clap::{CommandFactory, Parser, error::ErrorKind};
use fmaker_explore::cli::Cli;
use fmaker_explore::error::Error;

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.retries < 0 {
        Cli::command()
            .error(ErrorKind::ValueValidation, "--retries must be non-negative")
            .exit();
    }
    let positive = |value: f64| value > 0.0 && value.is_finite();
    if !positive(cli.timeout) || !positive(cli.maker_wait) {
        Cli::command()
            .error(
                ErrorKind::ValueValidation,
                "--timeout and --maker-wait must be positive",
            )
            .exit();
    }

    match fmaker_explore::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Io(error)) => {
            let detail = if error.kind() == std::io::ErrorKind::StorageFull {
                "local disk is full".to_string()
            } else {
                error.to_string()
            };
            eprintln!("error: {detail}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
