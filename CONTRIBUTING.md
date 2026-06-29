# Contributing to mcpdrain

Thanks for considering a contribution. mcpdrain aims to do one boring thing —
keep a stdio MCP server from deadlocking on a full pipe — correctly and with no
surprises. That goal shapes what gets merged.

## Ground rules

- **Keep the core small.** mcpdrain is infrastructure, not a framework. New
  surface area needs to earn its keep; a smaller, more correct tool beats a
  larger one.
- **Correctness over features.** A change that touches the drain/spool/teardown
  path must come with a test that fails without it.
- **No new runtime dependencies** without a clear reason. The binary is meant to
  be a tiny, static, drop-in wrapper.

## Development

```bash
cargo test --all                                    # incl. the deadlock test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

CI runs the same three on Linux and macOS plus an MSRV (1.85) check and a
`cargo audit`. Please run them locally before opening a PR.

The credibility test lives at `crates/mcpdrain-core/tests/deadlock.rs`: it floods
256 KiB to stderr, then emits 256 KiB on stdout, and asserts mcpdrain delivers
the full response (a non-draining client deadlocks on this). If you change the
proxy, keep that test green and add one that covers your case.

## Reporting a hang

If mcpdrain didn't prevent a hang, that's the most valuable bug report. Include:

- `mcpdrain --version` and `mcpdrain doctor` output,
- your OS and the exact `mcpdrain run -- …` command,
- the server (and its command), and what the client was doing,
- a minimal reproduction if you can manage one.

## Pull requests

- One concern per PR; keep the diff focused.
- Update `CHANGELOG.md` under `[Unreleased]`.
- Describe what broke and how you proved the fix.

By contributing you agree your work is licensed under the project's Apache-2.0
license.
