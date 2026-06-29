//! Stall watchdog policy.
//!
//! The proxy's writer task updates a "last progress" timestamp whenever the
//! client accepts bytes. The main loop polls it: if the writer has made no
//! progress for `stall_timeout` while bytes remain, the situation is a
//! client-side deadlock and [`should_act`] decides what to do about it.

use crate::RestartPolicy;

/// Returns true when a detected stall should trigger supervisor action
/// (v0.1: abort the deadlocked server with diagnostics; v0.2: respawn + replay).
#[inline]
pub fn should_act(policy: RestartPolicy, stalled: bool) -> bool {
    stalled && policy != RestartPolicy::Never
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_inacts() {
        assert!(!should_act(RestartPolicy::Never, true));
    }

    #[test]
    fn eager_and_lazy_act_on_stall() {
        assert!(should_act(RestartPolicy::Eager, true));
        assert!(should_act(RestartPolicy::Lazy, true));
    }

    #[test]
    fn no_action_when_not_stalled() {
        assert!(!should_act(RestartPolicy::Eager, false));
    }
}
