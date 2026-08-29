use std::process::ExitCode;

use npc_runtime_host::cli::{run, Cli};

#[tokio::main]
async fn main() -> ExitCode {
    match run(<Cli as clap::Parser>::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Errors are sanitized by each boundary before reaching this point.
            use std::io::Write;
            let _ = std::io::stderr().write_all(format!("npc-runtime: {error}\n").as_bytes());
            ExitCode::FAILURE
        }
    }
}
