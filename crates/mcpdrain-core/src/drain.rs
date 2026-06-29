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
    /// The server's own exit code if it exited by itself (`128 + signal` when it
    /// was killed by a signal, mirroring the shell). `None` when mcpdrain tore it
    /// down (stall abort, or a signal to mcpdrain) — that is not a server failure.
    pub server_exit_code: Option<i32>,
    /// True if mcpdrain itself received SIGINT/SIGTERM and shut the server down.
    pub interrupted: bool,
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

/// Entry point for the `mcpdrain` binary: proxy process stdin/stdout, and
/// forward the server's stderr through to mcpdrain's own stderr (drained, so it
/// can never deadlock, but still visible to the operator).
pub async fn run(config: Config) -> std::io::Result<ProxyStats> {
    proxy(
        tokio::io::stdin(),
        tokio::io::stdout(),
        tokio::io::stderr(),
        config,
    )
    .await
}

/// Proxy a client's stdin/stdout to a spawned MCP server, draining every server
/// stream concurrently so the server can never block on a full pipe.
///
/// - `client_in`  — bytes the client sends (requests) → forwarded to the server.
/// - `client_out` — bytes the server emits (responses) → forwarded to the client.
/// - `client_err` — the server's stderr is drained (always) and forwarded here,
///   so logs stay visible. Forwarding is best-effort: if this sink errors, the
///   stream is still drained to keep the deadlock guarantee.
pub async fn proxy<R, W, E>(
    client_in: R,
    mut client_out: W,
    client_err: E,
    config: Config,
) -> std::io::Result<ProxyStats>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
    E: AsyncWrite + Unpin + Send + 'static,
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

    // --- drain stderr unconditionally (undrained stderr is the #1 deadlock
    //     cause) AND forward it to the operator's stderr so logs aren't lost.
    //     If forwarding errors, keep draining — draining is what prevents the
    //     deadlock; visibility is best-effort.
    let stderr_task = tokio::spawn(async move {
        let mut stderr = stderr;
        let mut client_err = client_err;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        let mut forwarding = true;
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total += n as u64;
                    if forwarding && client_err.write_all(&buf[..n]).await.is_err() {
                        forwarding = false;
                    }
                }
            }
        }
        let _ = client_err.flush().await;
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
    //     A SIGINT/SIGTERM to mcpdrain breaks the loop so the child (which is in
    //     its own process group and would otherwise be orphaned) is torn down.
    let mut signals = Signals::install();
    loop {
        tokio::select! {
            biased;
            sig = signals.recv() => {
                stats.interrupted = true;
                tracing::warn!(target: "mcpdrain", signal = sig, "received signal; shutting down server");
                break;
            }
            chunk = spool_rx.recv() => {
                let Some(chunk) = chunk else { break };  // server stdout EOF: done
                match tokio::time::timeout(stall_timeout, client_out.write_all(&chunk)).await {
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
        }
    }

    // Cleanup: don't block on a client that keeps stdin open; reap the child and
    // capture its exit status (None when *we* tore it down — stall or signal).
    stdin_task.abort();
    let exited = shutdown_child(&mut child).await;
    stats.server_exit_code = exited.map(status_to_code);

    stats.server_produced = reap_count(stdout_task).await;
    stats.stderr_bytes = reap_count(stderr_task).await;
    stats.stdin_bytes = reap_count(stdin_task).await;

    Ok(stats)
}

/// Cross-platform SIGINT/SIGTERM listener. On non-unix only Ctrl-C is available.
struct Signals {
    #[cfg(unix)]
    term: tokio::signal::unix::Signal,
}

impl Signals {
    fn install() -> Self {
        #[cfg(unix)]
        {
            // SIGTERM in addition to Ctrl-C (SIGINT, via ctrl_c() below).
            let term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
            Signals { term }
        }
        #[cfg(not(unix))]
        {
            Signals {}
        }
    }

    /// Resolve when SIGINT or SIGTERM arrives; returns the signal name.
    async fn recv(&mut self) -> &'static str {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => "SIGINT",
                _ = self.term.recv() => "SIGTERM",
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            "SIGINT"
        }
    }
}

/// Map an exit status to a shell-style code: the real code, or `128 + signal`
/// when the process was terminated by a signal.
fn status_to_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
    }
    #[cfg(not(unix))]
    {
        status.code().unwrap_or(0)
    }
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

/// Signal the child's whole process *group* (it leads its own group via
/// `setpgid`), so a server that forked helpers — `sh -c 'real-server'`, an `npx`
/// shim, etc. — is torn down completely instead of leaving a grandchild holding
/// the stdout pipe open (which would hang mcpdrain's cleanup forever).
#[cfg(unix)]
fn signal_group(child: &Child, sig: i32) {
    if let Some(pid) = child.id() {
        // SAFETY: a negative pid targets the process group led by `pid`; we made
        // the child its own group leader, so this hits the server + its helpers.
        // Harmless (ESRCH) if the group is already gone.
        unsafe {
            libc::kill(-(pid as i32), sig);
        }
    }
}

/// Reap the child, returning its status if it exited on its own (within the
/// grace period), or `None` if mcpdrain had to force-kill it.
///
/// Order: if it already exited, reap it. Otherwise ask the whole process group
/// politely (SIGTERM) and wait the grace period; if still alive, SIGKILL the
/// group so a lingering server can never hang mcpdrain.
async fn shutdown_child(child: &mut Child) -> Option<std::process::ExitStatus> {
    if let Ok(Some(status)) = child.try_wait() {
        tracing::debug!(target: "mcpdrain", ?status, "server already exited");
        return Some(status);
    }
    #[cfg(unix)]
    signal_group(child, libc::SIGTERM);
    match tokio::time::timeout(SHUTDOWN_GRACE, child.wait()).await {
        Ok(Ok(status)) => {
            tracing::debug!(target: "mcpdrain", ?status, "server exited after SIGTERM");
            Some(status)
        }
        _ => {
            #[cfg(unix)]
            signal_group(child, libc::SIGKILL);
            #[cfg(not(unix))]
            let _ = child.start_kill();
            let _ = child.wait().await;
            tracing::debug!(target: "mcpdrain", "server force-killed");
            None
        }
    }
}

/// Await a drain task for its byte count, but never let cleanup hang: if the
/// task hasn't finished shortly after the child is gone, abort it and report 0.
async fn reap_count(mut task: tokio::task::JoinHandle<u64>) -> u64 {
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(Ok(n)) => n,
        Ok(Err(_)) => 0,
        Err(_) => {
            task.abort();
            0
        }
    }
}
