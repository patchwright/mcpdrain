# Security Policy

`mcpdrain` sits in a **trust boundary**: it relays arbitrary tool traffic between an
MCP client and an MCP server, and it spawns and can kill the server process. This
document describes what that means and how to report problems.

## Trust model

- `mcpdrain` does **not** inspect or modify the content of relayed messages in v0.1
  (bytes flow through untouched). It only observes byte counts and timing.
- `mcpdrain` executes the command you pass to `run -- <cmd>` with your privileges.
  Treat the command argument as you would any `sh -c`: only proxy servers you trust.
- The stall watchdog kills the server it spawned (its own process group) when a
  client-facing write stalls. It does **not** restart on a schedule, so a malicious
  server cannot trigger an unbounded restart loop by simply being slow; it can at
  most cause one abort per stall interval.

## Reporting a vulnerability

Please report security issues privately rather than as a public issue.

- Open a **GitHub private security advisory** on this repository, or
- Email: `security@invariantwatch.dev` (PGP key on request).

You should expect an acknowledgement within 72 hours. Please do not disclose the
issue publicly until a fix is released.

## Scope

In scope: any way `mcpdrain` can be made to execute unintended commands, leak data
across proxied sessions, fail to kill its child processes, or be coerced into a
denial-of-service (e.g. restart storm). Out of scope: bugs in the MCP servers you
proxy, or behavior explicitly documented as v0.1 limitations.

## Hardening checklist (CI-enforced)

- `cargo audit` runs on every push and PR.
- `RUSTFLAGS=-D warnings` + `clippy -D warnings` gate all builds.
- The child server is started in its own process group for clean teardown.
