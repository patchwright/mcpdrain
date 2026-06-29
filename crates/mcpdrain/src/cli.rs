//! Command-line interface.

use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use mcpdrain_core::{pipe_capacity_bytes, Config, RestartPolicy};

#[derive(Parser)]
#[command(
    name = "mcpdrain",
    version,
    about = "Deadlock-proof stdio guardian for MCP servers — stop your server hanging on a full pipe buffer.",
    long_about = "mcpdrain sits between any MCP client and any stdio MCP server, draining stdin, \
                  stdout, and stderr concurrently so the server can never block on a full pipe \
                  buffer. Wrap your existing server: `mcpdrain run -- <server>`."
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Proxy a server command, draining all stdio streams deadlock-proof.
    Run {
        /// Server command + args (place after `--`).
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
        /// Seconds a client-facing write may stall before acting.
        #[arg(long, default_value_t = 15)]
        stall: u64,
        /// Stall action: eager (abort immediately), lazy, or never (report only).
        #[arg(long, value_parser = parse_policy, default_value = "eager")]
        restart: RestartPolicy,
    },
    /// Report the host OS pipe capacity and the deadlock threshold.
    Doctor,
}

fn parse_policy(s: &str) -> Result<RestartPolicy, String> {
    match s.to_ascii_lowercase().as_str() {
        "eager" => Ok(RestartPolicy::Eager),
        "lazy" => Ok(RestartPolicy::Lazy),
        "never" => Ok(RestartPolicy::Never),
        other => Err(format!(
            "unknown restart policy '{other}' (expected eager|lazy|never)"
        )),
    }
}

pub async fn run() -> std::io::Result<ExitCode> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run {
            command,
            stall,
            restart,
        } => {
            if command.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no server command given (usage: mcpdrain run -- <cmd> [args])",
                ));
            }
            let config = Config::new(command)
                .stall_timeout(Duration::from_secs(stall))
                .restart_policy(restart);
            let stats = mcpdrain_core::run(config).await?;
            tracing::info!(target: "mcpdrain", ?stats, "session complete");
            if stats.stalled {
                eprintln!(
                    "mcpdrain: client-facing write stalled and the server was aborted. The \
                     client likely deadlocked; restart it, or re-run with --restart never to \
                     inspect."
                );
                Ok(ExitCode::from(2))
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
        Cmd::Doctor => {
            let cap = pipe_capacity_bytes();
            println!("{{\"pipe_capacity_bytes\":{}}}", cap);
            eprintln!("OS pipe capacity ≈ {} bytes ({} KiB).", cap, cap / 1024);
            eprintln!(
                "Any single MCP response larger than this can deadlock a client that does not \
                 drain stderr concurrently. mcpdrain prevents it: `mcpdrain run -- <your-server>`."
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}
