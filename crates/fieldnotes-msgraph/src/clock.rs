//! Injected time for retry backoff.
//!
//! [`RetryPolicy`](crate::client::RetryPolicy)'s bounds are measured, and its
//! delays are slept, through [`RetryClock`] rather than
//! [`std::time::Instant`]/[`std::thread::sleep`] directly, so a test can
//! assert the exact bound-exhaustion behavior of a multi-minute retry
//! sequence without a test that runs for minutes.
//!
//! This mirrors [`fieldnotes_domain::Clock`]'s shape (an injected millisecond
//! reading) with the one extra primitive retry backoff needs beyond ID
//! generation: suspending execution. It is a monotonic tick count, not
//! calendar time, and is never interpreted as one.

use std::time::{Duration, Instant};

/// Injected control over elapsed-time measurement and delay during retry
/// backoff.
///
/// [`RetryClock::now_millis`] must be non-decreasing for a single clock
/// instance; [`crate::client::GraphClient`] uses differences between two
/// readings only, never the absolute value.
pub trait RetryClock {
    /// A monotonically non-decreasing tick count, in milliseconds, since an
    /// arbitrary reference point fixed for this clock's lifetime.
    fn now_millis(&self) -> u64;

    /// Suspends the caller for approximately `duration`.
    fn sleep(&self, duration: Duration);
}

/// The shipping [`RetryClock`]: [`std::time::Instant`] and
/// [`std::thread::sleep`].
///
/// This is the one exception to "no wall clock in library logic": it reads
/// a monotonic tick count, never calendar time, and every retry *decision*
/// (whether to wait, for how long, and when to give up) still runs through
/// the trait, so a test never needs it. Unlike this crate's rejection of a
/// production [`fieldnotes_domain::RandomSource`] — which would need a new
/// dependency this workspace reserves for the composition root, per
/// `getrandom`'s "CLI crate only" rule in the root `Cargo.toml` — a
/// monotonic-time reader needs nothing beyond `std`, and
/// `fieldnotes_field_protocol::host` already reads `Instant::now()` directly
/// below the CLI for its own timeout handling. Shipping it here saves every
/// Microsoft Field from redefining the same few lines.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRetryClock;

impl SystemRetryClock {
    /// A new clock. Stateless; every instance measures from its own first
    /// [`RetryClock::now_millis`] call onward via a process-wide monotonic
    /// origin, so two instances remain comparable.
    #[must_use]
    pub fn new() -> Self {
        SystemRetryClock
    }
}

impl RetryClock for SystemRetryClock {
    fn now_millis(&self) -> u64 {
        // `Instant` has no public epoch, so this crate fixes its own the
        // first time it is read and measures from there. `OnceLock`
        // guarantees the origin is captured exactly once per process.
        static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let origin = *ORIGIN.get_or_init(Instant::now);
        u64::try_from(Instant::now().saturating_duration_since(origin).as_millis())
            .unwrap_or(u64::MAX)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::{RetryClock, SystemRetryClock};
    use std::time::Duration;

    #[test]
    fn now_millis_is_non_decreasing() {
        let clock = SystemRetryClock::new();
        let first = clock.now_millis();
        let second = clock.now_millis();
        assert!(second >= first);
    }

    #[test]
    fn sleep_waits_at_least_the_requested_duration() {
        let clock = SystemRetryClock::new();
        let before = clock.now_millis();
        clock.sleep(Duration::from_millis(10));
        let after = clock.now_millis();
        assert!(after >= before + 10);
    }
}
