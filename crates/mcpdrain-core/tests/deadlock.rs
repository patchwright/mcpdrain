//! Credibility test: proves mcpdrain drains every stream concurrently.
//!
//! The fixture server floods 256 KiB to **stderr** (more than any host's pipe
//! capacity) and *then* emits 256 KiB + newline to stdout. A client that does
//! not drain stderr concurrently deadlocks here forever: the server fills the
//! stderr pipe, blocks on the write, and never reaches stdout. mcpdrain drains
//! both concurrently, so the full stdout response is delivered and the session
//! completes. A hang surfaces as the test's timeout failing the assertion.

use std::time::Duration;

use mcpdrain_core::{proxy, Config};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_stderr_drain_prevents_deadlock() {
    // 256 KiB floods — well above Linux (64 KiB) and macOS (16 KiB) capacity.
    // stderr first (this is what deadlocks a non-draining client), then stdout.
    let script = "yes x | head -c 262144 >&2; yes x | head -c 262144; printf '\\n'; sleep 0.2";
    let config = Config::new(["sh", "-c", script]).stall_timeout(Duration::from_secs(10));

    // &'static [u8] request; client_in EOFs immediately after the request line.
    let request: &'static [u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n";

    let (mut rx, tx) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move { proxy(request, tx, config).await });

    let mut received = Vec::new();
    let copied = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::io::copy(&mut rx, &mut received).await
    })
    .await;
    assert!(
        copied.is_ok(),
        "proxy hung — stderr was not drained concurrently (deadlock not prevented)"
    );

    let stats = proxy_task
        .await
        .expect("proxy task panicked")
        .expect("proxy returned an error");

    // 262144 bytes of "x\n" plus the final newline = 262145 bytes on stdout.
    assert_eq!(
        received.len(),
        262145,
        "full stdout response was not delivered"
    );
    assert!(
        stats.stdout_bytes >= 262144,
        "stdout undercount: {}",
        stats.stdout_bytes
    );
    assert!(
        stats.stderr_bytes >= 262144,
        "stderr was not drained: {}",
        stats.stderr_bytes
    );
    assert!(!stats.stalled, "a false stall was reported");
}
