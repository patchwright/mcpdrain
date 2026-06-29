# mcpdrain

> Your MCP server doesn't hang because it's broken — it hangs because OS pipe buffers are tiny. `mcpdrain` is the single binary that sits between your client and server and makes the hang impossible.

`mcpdrain` is a **deadlock-proof stdio guardian for MCP (Model Context Protocol) servers.** Drop it between any MCP client and any stdio MCP server: it concurrently drains stdin, stdout, **and** stderr so the server can never block on a full pipe buffer, and it recovers from client-side stalls.

## The 8 KB problem

Every stdio MCP server hangs forever the moment a response exceeds the OS pipe capacity while the client isn't concurrently draining **every** stream. The pipe capacities are small and baked into the kernel:

| OS      | pipe capacity |
|---------|---------------|
| Linux   | ~64 KiB       |
| macOS   | ~16 KiB       |
| Windows | 4–64 KiB      |

A single `tools/list` response, a big `read`, or even a chatty `stderr` log is enough. The deadlock is silent — your client just says "Running…" forever. The ecosystem's usual answer is *"abandon stdio, switch to SSE."* `mcpdrain`'s answer is: **stdio is fine; I'll make it safe to keep using.**

```bash
$ mcpdrain doctor
{"pipe_capacity_bytes":65536}
OS pipe capacity ≈ 65536 bytes (64 KiB).
```

## Install

```bash
# Cargo
cargo install mcpdrain

# Homebrew
brew install patchwright/tap/mcpdrain   # (formula pending first release)

# curl | sh
curl -fsSL https://raw.githubusercontent.com/patchwright/mcpdrain/main/install.sh | sh
```

Prebuilt musl/macOS-universal/Windows binaries land on the first release.

## 30-second usage

Wrap your existing server command — nothing else changes:

```bash
# Before (hangs on a 65 KB tools/list response):
claude-code --mcp-server "npx -y @modelcontextprotocol/server-filesystem /repo"

# After (deadlock-proof):
mcpdrain run -- npx -y @modelcontextprotocol/server-filesystem /repo
```

Point your client at the `mcpdrain run -- <server>` command in your MCP config (Claude Code, Cursor, Codex CLI, Windsurf, Claude Desktop — any of them). It is transparent: bytes flow through untouched.

```bash
# Diagnose a hang:
mcpdrain doctor
# {"pipe_capacity_bytes":65536}

# Tune the stall watchdog (restart a stalled server eagerly):
mcpdrain run --stall 10 --restart eager -- <your-server>
```

## How it works

The #1 cause of the hang is an **undrained stderr**: the client reads stdout, the server fills the stderr pipe, the server blocks writing stderr, never finishes stdout, and the client waits forever.

```
client stdin  ─►  mcpdrain  ─►  server stdin
client stdout ◄─  mcpdrain  ◄─  server stdout   (spooled: never blocks the server)
                 mcpdrain  ◄─  server stderr    (always drained → ring buffer)
```

`mcpdrain` runs a dedicated reader for **each** stream so the server can never block, decouples server-drain from client-write with a bounded spool, and watches the client-facing write: if it stalls for longer than `--stall` seconds, `mcpdrain` aborts the deadlocked server (with diagnostics) instead of hanging forever.

## What's intentionally not in v0.1

- **Not an MCP server.** ~52% of published MCP servers are already abandoned; `mcpdrain` is infrastructure you run, not another server to register.
- **No plugin system / GUI / polyglot bindings.** It's a process you run. The core is a trait-based library crate so these are additive later.
- **Windows is best-effort.** POSIX pipe semantics (where the documented hangs live) ship first; named-pipe support is tracked for v0.2.
- **No request replay yet.** v0.2 adds `mcpdrain replay` for session capture/replay (handy for audit/debugging).

## Development

```bash
cargo test --all           # incl. the deadlock-reproduction integration test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
MSRV: 1.75 · License: Apache-2.0
```

The credibility test (`crates/mcpdrain-core/tests/deadlock.rs`) spawns a server that floods 256 KiB to **stderr** then emits 256 KiB to stdout, proxies it through `mcpdrain`, and asserts the full response is delivered. A client that doesn't drain stderr concurrently deadlocks on this; `mcpdrain` does not.

## Status

Pre-release. v0.1 scope: `run` + `doctor`. See [CHANGELOG.md](CHANGELOG.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
