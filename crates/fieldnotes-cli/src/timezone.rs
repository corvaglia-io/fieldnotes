//! The timezone setting: how a numeric UTC offset is obtained for a given
//! instant.
//!
//! A1 section 3 requires "a reliable source-local offset when supplied,
//! otherwise the configured Field or client-local offset **at that
//! instant**". A fixed number cannot satisfy that for any zone with daylight
//! saving: `Europe/Zurich` is `+01:00` in January and `+02:00` in July, so a
//! profile that stored a frozen `+02:00` would silently mislabel half the
//! year. [`TimeZoneSpec`] therefore stores either a fixed offset (for users
//! who genuinely want one, unaffected by daylight saving) or a zone that is
//! re-resolved for every instant.
//!
//! Resolving a named zone needs an IANA time-zone database, which the Rust
//! standard library does not carry. This module is the one place in the
//! workspace that depends on [`jiff`] for that: `jiff` was chosen over
//! `time`'s `local-offset` feature because it can resolve an arbitrary named
//! zone (not only "the current system offset") for an arbitrary instant, on
//! macOS, Linux, and Windows, bundling its own tzdb fallback where the
//! platform has none, with only itself and its own small tzdb helper crates
//! in the dependency tree.

use std::fmt;

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::environment::parse_offset;

/// How generated datetimes obtain their UTC offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeZoneSpec {
    /// A fixed offset in minutes east of UTC, the same at every instant.
    ///
    /// This is what a user who explicitly wants a stable offset — regardless
    /// of daylight saving in whatever zone they happen to be visiting — asks
    /// for with `+02:00`, `-05:00`, `Z`, or `utc`.
    Fixed(i16),
    /// The operating system's current local timezone, resolved fresh for
    /// every instant.
    System,
    /// A named IANA timezone (for example `Europe/Zurich`), resolved fresh
    /// for every instant.
    Named(String),
}

/// Why a timezone spec could not be parsed or resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeZoneError(pub String);

impl fmt::Display for TimeZoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TimeZoneError {}

impl TimeZoneSpec {
    /// Parses `system`, a fixed `+HH:MM`/`-HH:MM`/`utc`/`Z` offset, or an IANA
    /// zone name such as `Europe/Zurich`.
    ///
    /// A zone name is validated against the timezone database immediately, so
    /// a typo is caught at `config set` (or on first use) instead of quietly
    /// defaulting every future Note to UTC.
    pub fn parse(text: &str) -> Result<Self, TimeZoneError> {
        let trimmed = text.trim();
        if trimmed.eq_ignore_ascii_case("system") {
            return Ok(TimeZoneSpec::System);
        }
        if let Ok(minutes) = parse_offset(trimmed) {
            return Ok(TimeZoneSpec::Fixed(minutes));
        }
        TimeZone::get(trimmed)
            .map(|_| TimeZoneSpec::Named(trimmed.to_owned()))
            .map_err(|_| {
                TimeZoneError(format!(
                    "`{trimmed}` is not `system`, a +HH:MM/-HH:MM/utc offset, or a \
                     known IANA timezone name such as `Europe/Zurich`"
                ))
            })
    }

    /// Resolves the offset in minutes east of UTC at `unix_millis`.
    ///
    /// This is the one place a stored zone becomes the numeric offset a
    /// [`fieldnotes_domain::Datetime`] carries, and it is evaluated fresh for
    /// every instant passed in: [`TimeZoneSpec::Fixed`] never changes, but
    /// [`TimeZoneSpec::Named`] and [`TimeZoneSpec::System`] cross
    /// daylight-saving transitions, which is the entire reason a zone is
    /// stored instead of a frozen number.
    pub fn resolve_minutes(&self, unix_millis: i64) -> Result<i16, TimeZoneError> {
        match self {
            TimeZoneSpec::Fixed(minutes) => Ok(*minutes),
            TimeZoneSpec::System => offset_minutes(&TimeZone::system(), unix_millis),
            TimeZoneSpec::Named(name) => {
                let zone = TimeZone::get(name).map_err(|error| {
                    TimeZoneError(format!("timezone `{name}` is no longer available: {error}"))
                })?;
                offset_minutes(&zone, unix_millis)
            }
        }
    }
}

/// Resolves one zone's offset at one instant.
fn offset_minutes(zone: &TimeZone, unix_millis: i64) -> Result<i16, TimeZoneError> {
    let seconds = unix_millis.div_euclid(1000);
    let timestamp = Timestamp::from_second(seconds).map_err(|_| {
        TimeZoneError("instant is outside the representable timezone range".to_owned())
    })?;
    let offset_seconds = zone.to_offset(timestamp).seconds();
    i16::try_from(offset_seconds / 60)
        .map_err(|_| TimeZoneError("resolved offset is out of range".to_owned()))
}

impl fmt::Display for TimeZoneSpec {
    /// Canonical storage/display form: the same grammar [`TimeZoneSpec::parse`]
    /// accepts, so `config set timezone` and `config show` round-trip.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeZoneSpec::Fixed(minutes) => {
                let sign = if *minutes < 0 { '-' } else { '+' };
                let magnitude = minutes.unsigned_abs();
                write!(f, "{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
            }
            TimeZoneSpec::System => write!(f, "system"),
            TimeZoneSpec::Named(name) => write!(f, "{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-01-15T00:00:00Z: Zurich is on standard time, `+01:00`.
    const JANUARY_2026_UTC_MILLIS: i64 = 1_768_435_200_000;
    /// 2026-07-15T00:00:00Z: Zurich is on daylight saving, `+02:00`.
    const JULY_2026_UTC_MILLIS: i64 = 1_784_073_600_000;

    #[test]
    fn parses_fixed_offsets_and_the_system_keyword() -> Result<(), TimeZoneError> {
        assert_eq!(TimeZoneSpec::parse("+02:00")?, TimeZoneSpec::Fixed(120));
        assert_eq!(TimeZoneSpec::parse("-05:30")?, TimeZoneSpec::Fixed(-330));
        assert_eq!(TimeZoneSpec::parse("utc")?, TimeZoneSpec::Fixed(0));
        assert_eq!(TimeZoneSpec::parse("SYSTEM")?, TimeZoneSpec::System);
        Ok(())
    }

    #[test]
    fn parses_and_validates_a_known_iana_zone() -> Result<(), TimeZoneError> {
        assert_eq!(
            TimeZoneSpec::parse("Europe/Zurich")?,
            TimeZoneSpec::Named("Europe/Zurich".to_owned())
        );
        Ok(())
    }

    #[test]
    fn rejects_an_unknown_zone_name() {
        let error = TimeZoneSpec::parse("Nowhere/Imaginary");
        assert!(error.is_err(), "an unknown zone name must be rejected");
    }

    #[test]
    fn a_fixed_offset_never_changes_across_the_year() -> Result<(), TimeZoneError> {
        let spec = TimeZoneSpec::parse("+02:00")?;
        assert_eq!(spec.resolve_minutes(JANUARY_2026_UTC_MILLIS)?, 120);
        assert_eq!(spec.resolve_minutes(JULY_2026_UTC_MILLIS)?, 120);
        Ok(())
    }

    /// The important one: a daylight-saving zone must resolve to two
    /// different offsets roughly six months apart, proving the profile
    /// resolves an offset per instant rather than freezing one number.
    #[test]
    fn a_named_zone_resolves_a_different_offset_across_daylight_saving() -> Result<(), TimeZoneError>
    {
        let spec = TimeZoneSpec::parse("Europe/Zurich")?;
        let january = spec.resolve_minutes(JANUARY_2026_UTC_MILLIS)?;
        let july = spec.resolve_minutes(JULY_2026_UTC_MILLIS)?;
        assert_eq!(january, 60, "Zurich in January is standard time, +01:00");
        assert_eq!(july, 120, "Zurich in July is daylight saving, +02:00");
        assert_ne!(
            january, july,
            "a DST-observing zone must not resolve to the same offset year-round"
        );
        Ok(())
    }

    #[test]
    fn display_round_trips_through_parse() -> Result<(), TimeZoneError> {
        for text in ["+02:00", "-05:30", "+00:00"] {
            let spec = TimeZoneSpec::parse(text)?;
            assert_eq!(spec.to_string(), text);
        }
        assert_eq!(TimeZoneSpec::System.to_string(), "system");
        assert_eq!(
            TimeZoneSpec::parse("Europe/Zurich")?.to_string(),
            "Europe/Zurich"
        );
        Ok(())
    }
}
