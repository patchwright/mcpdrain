# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - Unreleased

### Added
- **Server stderr is now forwarded** to mcpdrain's stderr (still drained, so it
  can never deadlock) — previously the server's logs were drained and silently
  discarded, leaving operators blind when debugging their server.
- **Exit-code propagation:** mcpdrain exits with the wrapped server's own exit
  code (`128 + signal` when the server was signaled), so a crashed or failed
  server is visible to a client or CI instead of being masked as success.
- **SIGINT/SIGTERM handling:** mcpdrain shuts the server down gracefully and
  exits `130`, instead of orphaning a server that runs in its own process group.

### Changed
- `core::proxy()` takes a `client_err` stream (the forwarding target for the
  server's stderr). **API change** for library users; the `mcpdrain` binary is
  unaffected.
- Server teardown signals the whole process **group** (SIGTERM then SIGKILL), so
  forked helpers (`sh -c '…'`, `npx` shims) are torn down too. Previously a
  grandchild could keep the stdout pipe open and hang mcpdrain's cleanup.
- MSRV pinned consistently at 1.85 (clippy.toml aligned with Cargo.toml).

### Fixed
- Cleanup could hang indefinitely if a forked grandchild held the stdout pipe
  open. Teardown now reaps the process group and bounds the drain-task joins.

## [0.1.0] - 2026-06-29

### Added
- `mcpdrain run -- <server>` — deadlock-proof stdio proxy for any MCP server.
- `mcpdrain doctor` — reports the host OS pipe capacity and the deadlock threshold.
- Concurrent three-stream drain (stdin/stdout/stderr): the server can never
  block on a full pipe because `mcpdrain` always drains every stream.
- Bounded spool decoupling server-drain from client-write, with a stall watchdog
  that detects and aborts a client-side deadlock.
- Structured diagnostics to stderr (JSON when `MCPDRAIN_JSON=1`).
