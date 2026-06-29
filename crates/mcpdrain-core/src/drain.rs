//! Concurrent three-stream relay — the core deadlock-prevention primitive.
//!
//! A naïve MCP client that reads stdout and stderr sequentially deadlocks the
//! moment the server fills the pipe the client isn't currently draining. The
//! proxy breaks this unconditionally by draining **all** of the server's
//! streams concurrently — the server can therefore never block on a write — and
//! then feeding the client at the client's own pace from a bounded spool. If the
//! *client* stops accepting bytes the server produced, the timed write notices
//! and the supervisor aborts the deadlocked server instead of hanging forever.
//!
//! The fix is the decoupling: a dedicated reader per stream (server can never
//! block) + a spool (server drain never waits on the client) + a timed write
//! (a stalled client is detected, not inherited as a hang).

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::supervisor;

/// 8 KiB read/write granularity.
const CHUNK: usize = 8 * 1024;
/// Grace period to let the server exit on its own before force-killing.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// Aggregate counters for a proxied session.
#[derive(Debug, Clone, Default)]
pub struct ProxyStats {
    /// Bytes read from the client (requests).
    pub stdin_bytes: u64,
    /// Bytes delivered to the client (responses).
    pub stdout_bytes: u64,
    /// Bytes drained from the server's stderr (always drained).
    pub stderr_bytes: u64,
    /// Bytes the server produced on stdout.
    pub server_produced: u64,
    /// True if a client-facing write stalled and we aborted the server.
    pub stalled: bool,
    /// Number of times the server was respawned (v0.1: always 0).
    pub restarts: u32,
}

struct ChildIo {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

fn spawn_child(config: &Config) -> std::io::Result<ChildIo> {
    let (program, args) = config.command.split_first().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "no server command given")
    })?;
    let mut command = Command::new(program);
    command.args(args);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Own process group: clean teardown on stall without signalling the client.
    new_process_group(&mut command);

    let mut child = command.spawn()?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    Ok(ChildIo {
        child,
        stdin,
        stdout,
        stderr,
    })
}

/// Entry point for the `mcpdrain` binary: proxy process stdin/stdout.
pub async fn run(config: Config) -> std::io::Result<ProxyStats> {
    proxy(tokio::io::stdin(), tokio::io::stdout(), config).await
}

/// Proxy a client's stdin/stdout to a spawned MCP server, draining every server
/// stream concurrently so the server can never block on a full pipe.
///
/// - `client_in`  — bytes the client sends (requests) → forwarded to the server.
/// - `client_out` — bytes the server emits (responses) → forwarded to the client.
pub async fn proxy<R, W>(
    client_in: R,
    mut client_out: W,
    config: Config,
) -> std::io::Result<ProxyStats>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut stats = ProxyStats::default();
    let stall_timeout = config.stall_timeout;
    let restart_policy = config.restart_policy;

    // Bound the spool so a runaway server can't OOM us; the watchdog fires long
    // before it fills if the client is genuinely stalled.
    let (spool_tx, mut spool_rx) =
        mpsc::channel::<Vec<u8>>((config.spool_capacity / CHUNK).max(1) + 1);

    let ChildIo {
        mut child,
        stdin,
        stdout,
        stderr,
    } = spawn_child(&config)?;
    tracing::info!(target: "mcpdrain", pid = child.id(), "spawned server");

    // --- drain stderr unconditionally: undrained stderr is the #1 deadlock cause
    let stderr_task = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n as u64,
            }
        }
        total
    });

    // --- drain server stdout → spool. This task NEVER touches the client, so the
    //     server can never block on it. This is the heart of the fix.
    let stdout_task = tokio::spawn(async move {
        let mut stdout = stdout;
        let mut total = 0u64;
        loop {
            let mut buf = vec![0u8; CHUNK];
            match stdout.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total += n as u64;
                    buf.truncate(n);
                    if spool_tx.send(buf).await.is_err() {
                        break; // writer gone (client closed / shutting down)
                    }
                }
            }
        }
        total
    });

    // --- relay client stdin → server stdin
    let stdin_task = tokio::spawn(async move {
        let mut server_stdin = stdin;
        let mut client_in = client_in;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            match client_in.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total += n as u64;
                    if server_stdin.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    let _ = server_stdin.flush().await;
                }
            }
        }
        total
    });

    // --- write the spool to the client at the client's own pace, supervised.
    //     Timing the WRITE itself (not an idle poll) means we only ever flag a
    //     real stall: the server produced bytes and the client won't take them.
    while let Some(chunk) = spool_rx.recv().await {
        let wrote = tokio::time::timeout(stall_timeout, client_out.write_all(&chunk)).await;
        match wrote {
            Ok(Ok(())) => {
                let _ = client_out.flush().await;
                stats.stdout_bytes += chunk.len() as u64;
            }
            Ok(Err(e)) => return Err(e),
            Err(_elapsed) => {
                // True stall: server produced bytes, client won't accept them.
                stats.stalled = true;
                if supervisor::should_act(restart_policy, true) {
                    tracing::warn!(
                        target: "mcpdrain",
                        stdout_bytes = stats.stdout_bytes,
                        timeout = ?stall_timeout,
                        "client-facing write stalled; aborting deadlocked server"
                    );
                    let _ = child.start_kill();
                } else {
                    tracing::warn!(
                        target: "mcpdrain",
                        "client-facing write stalled; restart policy is Never — reporting and exiting"
                    );
                }
                break;
            }
        }
    }

    // Cleanup: don't block on a client that keeps stdin open; reap the child.
    stdin_task.abort();
    shutdown_child(&mut child).await;

    stats.server_produced = stdout_task.await.unwrap_or(0);
    stats.stderr_bytes = stderr_task.await.unwrap_or(0);
    stats.stdin_bytes = stdin_task.await.unwrap_or(0);

    Ok(stats)
}

/// Put the child in its own process group so we can clean it up without
/// signalling the client process (POSIX only).
#[cfg(unix)]
fn new_process_group(command: &mut Command) {
    // SAFETY: pre_exec runs after fork(), before exec(); setpgid is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            let _ = libc::setpgid(0, 0);
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn new_process_group(_command: &mut Command) {}

async fn shutdown_child(child: &mut Child) {
    // Give the server a moment to exit on its own (stdin closed → most servers
    // quit), then force-kill so a lingering server can never hang mcpdrain.
    match tokio::time::timeout(SHUTDOWN_GRACE, child.wait()).await {
        Ok(status) => tracing::debug!(target: "mcpdrain", ?status, "server exited"),
        Err(_) => {
            let _ = child.start_kill();
            if let Ok(status) = child.wait().await {
                tracing::debug!(target: "mcpdrain", ?status, "server killed");
            }
        }
    }
}
