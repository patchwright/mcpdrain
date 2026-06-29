//! `mcpdrain` — deadlock-proof stdio guardian for MCP servers.

mod cli;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    init_tracing();
    match cli::run().await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("mcpdrain: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("warn"))
        .unwrap_or_default();
    let json = std::env::var("MCPDRAIN_JSON")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    // Best-effort init: if a subscriber is already set (e.g. in tests), ignore.
    let _ = if json {
        fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .json()
            .try_init()
    } else {
        fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init()
    };
}
