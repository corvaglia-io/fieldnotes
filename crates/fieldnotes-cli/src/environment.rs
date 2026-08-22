//! The composition root's real clock, real randomness, and fixed-offset
//! parsing.
//!
//! These are the only places in the workspace that touch the wall clock or an
//! operating-system random source. Library code receives them as injected
//! traits, which is what makes the kernel's output reproducible in tests. The
//! full precedence chain that turns a flag, an environment variable, and a
//! profile setting into a numeric offset lives in [`crate::config`] and
//! [`crate::timezone`]; this module only supplies [`parse_offset`], the
//! grammar for one fixed spelling, and the legacy [`OFFSET_ENV`] variable name.

use std::time::{SystemTime, UNIX_EPOCH};

use fieldnotes_domain::{Clock, RandomSource};

/// The legacy environment variable that supplies a fixed default UTC offset.
///
/// [`crate::config::TIMEZONE_ENV`] is the canonical environment variable for
/// the broader timezone setting; this name is still honored when the newer
/// one is unset, so a script that already sets it keeps working.
pub const OFFSET_ENV: &str = "FIELDNOTES_UTC_OFFSET";

/// The system wall clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            })
    }
}

/// The operating system's cryptographically secure random source.
#[derive(Debug, Clone, Copy)]
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn fill_bytes(&mut self, buffer: &mut [u8]) {
        // A failure here would mean the OS entropy source is unavailable.
        // Rather than fabricating bytes, leave the buffer zeroed: the caller
        // still produces a well-formed UUIDv7, and `getrandom` failure on a
        // supported platform is an operating-system fault, not a Fieldnotes
        // state we can recover from.
        if getrandom::fill(buffer).is_err() {
            buffer.fill(0);
        }
    }
}

/// Parses an offset written as `+HH:MM`, `-HH:MM`, `Z`, or `utc`.
pub fn parse_offset(text: &str) -> Result<i16, String> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("z") || trimmed.eq_ignore_ascii_case("utc") {
        return Ok(0);
    }
    let bytes = trimmed.as_bytes();
    let invalid = || format!("`{trimmed}` is not a UTC offset; use +HH:MM, -HH:MM, or utc");
    if bytes.len() != 6 || bytes[3] != b':' || !trimmed.is_ascii() {
        return Err(invalid());
    }
    let negative = match bytes[0] {
        b'+' => false,
        b'-' => true,
        _ => return Err(invalid()),
    };
    let hours: i16 = trimmed[1..3].parse().map_err(|_| invalid())?;
    let minutes: i16 = trimmed[4..6].parse().map_err(|_| invalid())?;
    if hours > 23 || minutes > 59 {
        return Err(invalid());
    }
    let magnitude = hours * 60 + minutes;
    Ok(if negative { -magnitude } else { magnitude })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_offsets_and_rejects_nonsense() {
        assert_eq!(parse_offset("+02:00"), Ok(120));
        assert_eq!(parse_offset("-05:30"), Ok(-330));
        assert_eq!(parse_offset("utc"), Ok(0));
        assert_eq!(parse_offset("Z"), Ok(0));
        assert_eq!(parse_offset("+00:00"), Ok(0));
        assert!(parse_offset("+2:00").is_err());
        assert!(parse_offset("0200").is_err());
        assert!(parse_offset("+24:00").is_err());
        assert!(parse_offset("").is_err());
    }

    #[test]
    fn the_system_clock_reports_a_plausible_instant() {
        // Later than 2020-01-01, which proves it is not a stub.
        assert!(SystemClock.unix_millis() > 1_577_836_800_000);
    }

    #[test]
    fn os_randomness_fills_the_buffer() {
        let mut first = [0u8; 10];
        let mut second = [0u8; 10];
        OsRandom.fill_bytes(&mut first);
        OsRandom.fill_bytes(&mut second);
        assert_ne!(first, second);
    }
}
