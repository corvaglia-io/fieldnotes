//! Shared deterministic test support for the Fieldnotes workspace.
//!
//! Injected clocks and random sources for deterministic ID generation, plus
//! helpers for locating the frozen golden fixture corpus.

pub mod tempdir;

use std::path::PathBuf;

use fieldnotes_domain::{Clock, RandomSource};

pub use tempdir::TempDir;

/// The absolute path of the repository's `tests/fixtures` directory.
#[must_use]
pub fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
}

/// A clock that always reports the same injected instant.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn unix_millis(&self) -> u64 {
        self.0
    }
}

/// A deterministic byte source that counts upward from an injected seed.
#[derive(Debug, Clone)]
pub struct CountingRandom {
    next: u8,
}

impl CountingRandom {
    /// Starts the sequence at `seed`.
    #[must_use]
    pub fn new(seed: u8) -> Self {
        CountingRandom { next: seed }
    }
}

impl RandomSource for CountingRandom {
    fn fill_bytes(&mut self, buffer: &mut [u8]) {
        for slot in buffer {
            *slot = self.next;
            self.next = self.next.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_domain::{IdError, RecordIdGenerator, RecordKind};

    #[test]
    fn injected_generator_is_reproducible() -> Result<(), IdError> {
        let make = || RecordIdGenerator::new(FixedClock(1_787_381_100_000), CountingRandom::new(1));
        let mut first = make();
        let mut second = make();
        assert_eq!(
            first.generate(RecordKind::Entity)?,
            second.generate(RecordKind::Entity)?
        );
        Ok(())
    }

    #[test]
    fn fixtures_root_points_at_the_corpus() {
        assert!(fixtures_root().join("notebooks").is_dir());
    }
}
