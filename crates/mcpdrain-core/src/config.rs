//! Proxy configuration.
//!
//! The CLI builds a [`Config`] from flags; a future TOML file will deserialize
//! into the same struct (`#[serde]` is wired now so that path is additive).

use std::path::PathBuf;
use std::time::Duration;

/// When the supervisor should act on a stalled client-facing write.
///
/// In v0.1 the action is *diagnose + abort the deadlocked server* (the CLI
/// exits with code 2). v0.2 adds respawn + in-flight request replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartPolicy {
    /// Act the instant a stall is detected (safest; may abort a transiently slow client).
    #[default]
    Eager,
    /// Wait one grace period before acting.
    Lazy,
    /// Never act; only emit diagnostics. Useful for debugging the client.
    Never,
}

/// Proxy configuration.
///
/// `serde::Deserialize` is derived so a future config file can feed this
/// directly. `Duration` serializes via serde's built-in impl.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Server command + args, e.g. `["npx", "-y", "@modelcontextprotocol/server-filesystem", "/repo"]`.
    pub command: Vec<String>,
    /// How long a client-facing write may stall before the supervisor acts.
    pub stall_timeout: Duration,
    /// Bytes of server stdout to spool while the client catches up.
    pub spool_capacity: usize,
    /// When to act on a stalled client-facing write.
    pub restart_policy: RestartPolicy,
    /// Optional ring-buffered stderr capture path (default: discarded).
    pub stderr_log: Option<PathBuf>,
    /// Capture the full session (newline-delimited JSON) for future `replay`.
    pub capture: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command: Vec::new(),
            stall_timeout: Duration::from_secs(15),
            spool_capacity: 8 * 1024 * 1024,
            restart_policy: RestartPolicy::default(),
            stderr_log: None,
            capture: false,
        }
    }
}

impl Config {
    /// Build a config from the server command (program + args).
    pub fn new<I, S>(command: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            command: command.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn stall_timeout(mut self, d: Duration) -> Self {
        self.stall_timeout = d;
        self
    }

    #[must_use]
    pub fn restart_policy(mut self, p: RestartPolicy) -> Self {
        self.restart_policy = p;
        self
    }

    #[must_use]
    pub fn spool_capacity(mut self, n: usize) -> Self {
        self.spool_capacity = n;
        self
    }

    #[must_use]
    pub fn capture(mut self, on: bool) -> Self {
        self.capture = on;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = Config::default();
        assert!(c.command.is_empty());
        assert_eq!(c.stall_timeout, Duration::from_secs(15));
        assert_eq!(c.restart_policy, RestartPolicy::Eager);
        assert!(!c.capture);
    }

    #[test]
    fn builder_chains() {
        let c = Config::new(["echo", "hi"])
            .stall_timeout(Duration::from_secs(3))
            .restart_policy(RestartPolicy::Never)
            .capture(true);
        assert_eq!(c.command, vec!["echo", "hi"]);
        assert_eq!(c.stall_timeout, Duration::from_secs(3));
        assert_eq!(c.restart_policy, RestartPolicy::Never);
        assert!(c.capture);
    }
}
