//! mcpdrain-core — deadlock-proof stdio proxy for MCP servers.
//!
//! Every stdio MCP server can hang forever when a response exceeds the OS pipe
//! capacity while the client isn't concurrently draining **every** stream (most
//! often stderr). This crate sits between any MCP client and server, drains all
//! three streams concurrently so the server can never block, spools output to
//! the client at the client's own pace, and aborts a client-side stall instead
//! of hanging forever.
//!
//! Start at [`proxy`] / [`run`] and see [`config::Config`].

pub mod buffer;
pub mod config;
pub mod drain;
pub mod protocol;
pub mod supervisor;

pub use buffer::{pipe_capacity_bytes, DEFAULT_PIPE_CAPACITY};
pub use config::{Config, RestartPolicy};
pub use drain::{proxy, run, ProxyStats};
