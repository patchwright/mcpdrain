# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `mcpdrain run -- <server>` — deadlock-proof stdio proxy for any MCP server.
- `mcpdrain doctor` — reports the host OS pipe capacity and the deadlock threshold.
- Concurrent three-stream drain (stdin/stdout/stderr): the server can never
  block on a full pipe because `mcpdrain` always drains every stream.
- Bounded spool decoupling server-drain from client-write, with a stall watchdog
  that detects and aborts a client-side deadlock.
- Structured diagnostics to stderr (JSON when `MCPDRAIN_JSON=1`).
