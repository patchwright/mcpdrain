# mcpdrain

[![crates.io](https://img.shields.io/crates/v/mcpdrain.svg)](https://crates.io/crates/mcpdrain)
[![downloads](https://img.shields.io/crates/d/mcpdrain.svg)](https://crates.io/crates/mcpdrain)
[![CI](https://github.com/patchwright/mcpdrain/actions/workflows/ci.yml/badge.svg)](https://github.com/patchwright/mcpdrain/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/mcpdrain.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://www.rust-lang.org)

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
# Cargo (any platform with a Rust toolchain)
cargo install mcpdrain

# curl | sh — prebuilt binary, no toolchain needed
curl -fsSL https://raw.githubusercontent.com/patchwright/mcpdrain/main/install.sh | sh
```

Prebuilt static binaries ship for `x86_64`/`aarch64` Linux (musl) and macOS on
each release. A Homebrew tap and Windows support are planned for a later release.

## 30-second usage

Wrap your existing server command — nothing else changes:

```bash
# Before (hangs on a 65 KB tools/list response):
claude-code --mcp-server "npx -y @modelcontextprotocol/server-filesystem /repo"

# After (deadlock-proof):
mcpdrain run -- npx -y @modelcontextprotocol/server-filesystem /repo
```

## Use it with your MCP client

Every stdio MCP client configures servers the same way — a `command` plus
`args`. To protect a server, set `command` to `mcpdrain` and prepend
`"run", "--"` to whatever you had. Nothing else changes; mcpdrain is transparent.

**Before** (`claude_desktop_config.json`, `.cursor/mcp.json`, `mcp.json`, …):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    }
  }
}
```

**After** — wrap it:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "mcpdrain",
      "args": ["run", "--", "npx", "-y", "@modelcontextprotocol/server-filesystem", "/path/to/dir"]
    }
  }
}
```

The same `command` → `mcpdrain`, `args` → `["run", "--", <old command>, <old args…>]`
rule works for **Claude Desktop, Claude Code, Cursor, Windsurf, Codex CLI**, and
anything else that speaks MCP over stdio. mcpdrain forwards stdin/stdout
untouched and passes the server's stderr through to its own, so your logs stay
visible while the deadlock is gone. It also propagates the server's exit code, so
a crashed server is still a visible failure rather than a silent success.

It works with **any** stdio MCP server — there is nothing to register and no
per-server support; if your client launches it as a subprocess, mcpdrain can wrap
it.

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
client stdin   ─►  mcpdrain  ─►  server stdin
client stdout  ◄─  mcpdrain  ◄─  server stdout   (spooled: never blocks the server)
operator stderr ◄─ mcpdrain  ◄─  server stderr   (always drained, then forwarded)
```

`mcpdrain` runs a dedicated reader for **each** stream so the server can never block, decouples server-drain from client-write with a bounded spool, and watches the client-facing write: if it stalls for longer than `--stall` seconds, `mcpdrain` aborts the deadlocked server (with diagnostics) instead of hanging forever. The server's stderr is always drained (that is what prevents the deadlock) and then forwarded to mcpdrain's own stderr, so logs are never lost. On exit, mcpdrain returns the server's exit code (`128 + signal` if it was signaled); on `SIGINT`/`SIGTERM` it tears the server down cleanly instead of orphaning it.

## Scope and roadmap

- **Not an MCP server.** ~52% of published MCP servers are already abandoned; `mcpdrain` is infrastructure you run, not another server to register.
- **No plugin system / GUI / polyglot bindings.** It's a process you run. The core is a trait-based library crate so these are additive later.
- **Linux + macOS today; Windows next.** POSIX pipe semantics (where the documented hangs live) ship first; named-pipe support is the main item tracked for a later release.
- **No request replay yet.** A future `mcpdrain replay` will capture/replay a session (handy for audit/debugging); the session-capture config is already wired.

## Development

```bash
cargo test --all           # incl. the deadlock-reproduction integration test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
MSRV: 1.85 · License: Apache-2.0
```

The credibility test (`crates/mcpdrain-core/tests/deadlock.rs`) spawns a server that floods 256 KiB to **stderr** then emits 256 KiB to stdout, proxies it through `mcpdrain`, and asserts the full response is delivered. A client that doesn't drain stderr concurrently deadlocks on this; `mcpdrain` does not.

## Status

v0.2 — `run` + `doctor`, with stderr passthrough, exit-code propagation, and
signal-clean shutdown. See [CHANGELOG.md](CHANGELOG.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
