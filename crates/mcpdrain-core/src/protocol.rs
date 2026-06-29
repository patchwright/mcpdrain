//! Minimal MCP stdio framing awareness.
//!
//! The MCP stdio transport carries one JSON-RPC message per line (newline-
//! delimited, no embedded newlines). For proxying we do **not** parse payloads
//! — bytes flow through untouched — but we model the frame boundary so a future
//! `replay` capture can record each message.
//!
//! (Note: this is *not* LSP `Content-Length` framing. MCP stdio is
//! newline-delimited JSON.)

/// Direction a frame travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameDir {
    /// Client → server (a request).
    In,
    /// Server → client (a response or notification).
    Out,
}

/// A single newline-delimited message observed on the wire.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Frame {
    pub dir: FrameDir,
    pub bytes: usize,
    pub line: String,
}
