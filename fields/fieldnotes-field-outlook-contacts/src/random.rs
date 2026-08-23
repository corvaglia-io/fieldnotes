//! A non-cryptographic randomness source for Graph retry-backoff jitter.
//!
//! [`fieldnotes_msgraph::client::RetryPolicy`]'s jitter "does not need
//! cryptographic quality, only enough spread to avoid a retry thundering
//! herd" (see that crate's own module documentation). Fieldnotes' shared
//! cryptographically secure source (`getrandom`) is reserved for the
//! composition root's UUIDv7 generation per the root `Cargo.toml`'s "CLI
//! crate only" rule, and pulling it into every Field binary just for retry
//! jitter would be exactly the kind of dependency this workspace avoids
//! adding without a reason. This module is a small, dependency-free
//! splitmix64 generator instead, seeded once from the wall clock.
//!
//! Only [`crate::main`] -- this binary's own composition root -- ever
//! constructs one; every other module reaches randomness only through the
//! injected [`fieldnotes_msgraph::RandomSource`] trait.

use fieldnotes_msgraph::RandomSource;

/// A splitmix64 generator, seeded once at process start.
pub(crate) struct ProcessLocalRandom {
    state: u64,
}

impl ProcessLocalRandom {
    /// Seeds a generator from the current wall-clock instant.
    ///
    /// Not a security boundary: this randomness source exists only to
    /// spread retry attempts apart, never to protect a secret or generate an
    /// identifier.
    #[must_use]
    pub(crate) fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        ProcessLocalRandom {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl RandomSource for ProcessLocalRandom {
    fn fill_bytes(&mut self, buffer: &mut [u8]) {
        let mut filled = 0;
        while filled < buffer.len() {
            let word = self.next_u64().to_le_bytes();
            let take = word.len().min(buffer.len() - filled);
            buffer[filled..filled + take].copy_from_slice(&word[..take]);
            filled += take;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessLocalRandom;
    use fieldnotes_msgraph::RandomSource;

    #[test]
    fn successive_draws_differ() {
        let mut random = ProcessLocalRandom::new();
        let mut first = [0_u8; 8];
        let mut second = [0_u8; 8];
        random.fill_bytes(&mut first);
        random.fill_bytes(&mut second);
        assert_ne!(first, second);
    }

    #[test]
    fn fill_bytes_fills_a_buffer_of_any_length() {
        let mut random = ProcessLocalRandom::new();
        let mut buffer = [0_u8; 3];
        random.fill_bytes(&mut buffer);
        assert!(buffer.iter().any(|byte| *byte != 0) || buffer == [0, 0, 0]);
    }
}
