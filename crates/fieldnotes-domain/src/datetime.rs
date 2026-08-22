//! Offset-bearing RFC 3339 datetimes and `YYYY-MM-DD` calendar dates.
//!
//! Every datetime carries an explicit numeric UTC offset. Canonical rendering
//! uses an uppercase `T`, a `+HH:MM` or `-HH:MM` offset (`-00:00` is invalid),
//! and omits fractional seconds when zero or otherwise emits one to nine
//! digits with trailing zeroes removed. Note filenames render the instant in
//! UTC at whole-second precision as `YYYYMMDDTHHMMSSZ`.

use core::cmp::Ordering;
use core::fmt;

/// Errors produced while parsing dates and datetimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DatetimeError {
    /// The value does not match the approved lexical form.
    Malformed,
    /// The datetime has no explicit numeric UTC offset.
    OffsetRequired,
    /// The offset is the invalid `-00:00` spelling.
    NegativeZeroOffset,
    /// A calendar or clock component is out of range.
    OutOfRange,
}

impl fmt::Display for DatetimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatetimeError::Malformed => write!(f, "value does not match the approved lexical form"),
            DatetimeError::OffsetRequired => {
                write!(f, "datetime requires an explicit numeric UTC offset")
            }
            DatetimeError::NegativeZeroOffset => write!(f, "the -00:00 offset is invalid"),
            DatetimeError::OutOfRange => write!(f, "calendar or clock component out of range"),
        }
    }
}

impl std::error::Error for DatetimeError {}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (year, month as u8, day as u8)
}

fn parse_fixed_digits(text: &str, digits: usize) -> Option<u32> {
    if text.len() != digits || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// A `YYYY-MM-DD` calendar date without a time or timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Parses the exact `YYYY-MM-DD` form and validates the calendar day.
    pub fn parse(text: &str) -> Result<Self, DatetimeError> {
        let bytes = text.as_bytes();
        // The grammar is pure ASCII; this keeps the byte offsets below on char
        // boundaries so multibyte input cannot panic the slices.
        if bytes.len() != 10 || !bytes.is_ascii() || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(DatetimeError::Malformed);
        }
        let year = parse_fixed_digits(&text[..4], 4).ok_or(DatetimeError::Malformed)?;
        let month = parse_fixed_digits(&text[5..7], 2).ok_or(DatetimeError::Malformed)?;
        let day = parse_fixed_digits(&text[8..10], 2).ok_or(DatetimeError::Malformed)?;
        #[allow(clippy::cast_possible_truncation)]
        let (year, month, day) = (year as u16, month as u8, day as u8);
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(i32::from(year), month) {
            return Err(DatetimeError::OutOfRange);
        }
        Ok(Date { year, month, day })
    }

    /// The year component.
    #[must_use]
    pub fn year(&self) -> u16 {
        self.year
    }

    /// The month component (1-12).
    #[must_use]
    pub fn month(&self) -> u8 {
        self.month
    }

    /// The day-of-month component.
    #[must_use]
    pub fn day(&self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// An RFC 3339 datetime with an explicit numeric UTC offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Datetime {
    date: Date,
    hour: u8,
    minute: u8,
    second: u8,
    /// Sub-second component in nanoseconds; canonical rendering trims zeros.
    nanos: u32,
    /// Offset from UTC in minutes. `-00:00` is rejected at parse time.
    offset_minutes: i16,
}

impl Datetime {
    /// Parses `YYYY-MM-DDTHH:MM:SS[.fraction](+|-)HH:MM`.
    ///
    /// A missing or non-numeric offset (including `Z`) is
    /// [`DatetimeError::OffsetRequired`]; `-00:00` is
    /// [`DatetimeError::NegativeZeroOffset`].
    pub fn parse(text: &str) -> Result<Self, DatetimeError> {
        let bytes = text.as_bytes();
        // The date/time prefix is pure ASCII; requiring that before slicing
        // keeps every fixed offset below on a char boundary so multibyte input
        // cannot panic.
        if bytes.len() < 19 || !bytes[..19].is_ascii() {
            return Err(DatetimeError::Malformed);
        }
        let date = Date::parse(&text[..10])?;
        if bytes[10] != b'T' || bytes[13] != b':' || bytes[16] != b':' {
            return Err(DatetimeError::Malformed);
        }
        let hour = parse_fixed_digits(&text[11..13], 2).ok_or(DatetimeError::Malformed)?;
        let minute = parse_fixed_digits(&text[14..16], 2).ok_or(DatetimeError::Malformed)?;
        let second = parse_fixed_digits(&text[17..19], 2).ok_or(DatetimeError::Malformed)?;
        if hour > 23 || minute > 59 || second > 59 {
            return Err(DatetimeError::OutOfRange);
        }

        let mut position = 19;
        let mut nanos: u32 = 0;
        if bytes.get(position) == Some(&b'.') {
            position += 1;
            let start = position;
            while position < bytes.len() && bytes[position].is_ascii_digit() {
                position += 1;
            }
            let digits = position - start;
            if digits == 0 || digits > 9 {
                return Err(DatetimeError::Malformed);
            }
            let fraction: u32 = text[start..position]
                .parse()
                .map_err(|_| DatetimeError::Malformed)?;
            nanos = fraction
                * 10u32.pow(9 - u32::try_from(digits).map_err(|_| DatetimeError::Malformed)?);
        }

        let offset = &text[position..];
        if offset.is_empty() || offset == "Z" || offset == "z" {
            return Err(DatetimeError::OffsetRequired);
        }
        let offset_bytes = offset.as_bytes();
        let negative = match offset_bytes[0] {
            b'+' => false,
            b'-' => true,
            _ => return Err(DatetimeError::OffsetRequired),
        };
        if offset_bytes.len() != 6 || !offset_bytes.is_ascii() || offset_bytes[3] != b':' {
            return Err(DatetimeError::Malformed);
        }
        let offset_hour = parse_fixed_digits(&offset[1..3], 2).ok_or(DatetimeError::Malformed)?;
        let offset_minute = parse_fixed_digits(&offset[4..6], 2).ok_or(DatetimeError::Malformed)?;
        if offset_hour > 23 || offset_minute > 59 {
            return Err(DatetimeError::OutOfRange);
        }
        let magnitude = offset_hour * 60 + offset_minute;
        if negative && magnitude == 0 {
            return Err(DatetimeError::NegativeZeroOffset);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let offset_minutes = if negative {
            -(magnitude as i16)
        } else {
            magnitude as i16
        };

        #[allow(clippy::cast_possible_truncation)]
        Ok(Datetime {
            date,
            hour: hour as u8,
            minute: minute as u8,
            second: second as u8,
            nanos,
            offset_minutes,
        })
    }

    /// Builds a datetime from a Unix-epoch millisecond instant rendered in an
    /// explicit numeric UTC offset.
    ///
    /// This is how an injected clock instant becomes a canonical
    /// explicit-offset value: the kernel never formats datetimes by hand and
    /// never depends on a platform timezone database. `offset_minutes` is the
    /// source-local or client-local offset at that instant, in minutes east of
    /// UTC; zero renders as `+00:00`, so the invalid `-00:00` spelling is
    /// unrepresentable.
    pub fn from_unix_millis(unix_millis: i64, offset_minutes: i16) -> Result<Self, DatetimeError> {
        if !(-1439..=1439).contains(&offset_minutes) {
            return Err(DatetimeError::OutOfRange);
        }
        let local_millis = unix_millis
            .checked_add(i64::from(offset_minutes) * 60_000)
            .ok_or(DatetimeError::OutOfRange)?;
        let seconds = local_millis.div_euclid(1000);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let millis = local_millis.rem_euclid(1000) as u32;
        let days = seconds.div_euclid(86_400);
        let seconds_of_day = seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let year = u16::try_from(year).map_err(|_| DatetimeError::OutOfRange)?;
        #[allow(clippy::cast_possible_truncation)]
        Ok(Datetime {
            date: Date { year, month, day },
            hour: (seconds_of_day / 3600) as u8,
            minute: (seconds_of_day % 3600 / 60) as u8,
            second: (seconds_of_day % 60) as u8,
            nanos: millis * 1_000_000,
            offset_minutes,
        })
    }

    /// The calendar date in the datetime's own offset.
    #[must_use]
    pub fn date(&self) -> Date {
        self.date
    }

    /// The offset from UTC in minutes.
    #[must_use]
    pub fn offset_minutes(&self) -> i16 {
        self.offset_minutes
    }

    /// The sub-second component in nanoseconds.
    #[must_use]
    pub fn nanos(&self) -> u32 {
        self.nanos
    }

    /// The instant as whole Unix-epoch seconds plus nanoseconds.
    #[must_use]
    pub fn instant(&self) -> (i64, u32) {
        let days = days_from_civil(
            i64::from(self.date.year),
            i64::from(self.date.month),
            i64::from(self.date.day),
        );
        let seconds = days * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
            - i64::from(self.offset_minutes) * 60;
        (seconds, self.nanos)
    }

    /// The instant in Unix-epoch milliseconds, truncating sub-millisecond digits.
    #[must_use]
    pub fn unix_millis(&self) -> i64 {
        let (seconds, nanos) = self.instant();
        seconds * 1000 + i64::from(nanos / 1_000_000)
    }

    /// Whether two values denote the same instant regardless of offset spelling.
    #[must_use]
    pub fn same_instant(&self, other: &Datetime) -> bool {
        self.instant() == other.instant()
    }

    /// Total order by instant, ignoring offset spelling.
    #[must_use]
    pub fn cmp_instant(&self, other: &Datetime) -> Ordering {
        self.instant().cmp(&other.instant())
    }

    /// The same instant rendered in UTC with a `+00:00` offset.
    ///
    /// Returns [`DatetimeError::OutOfRange`] if the UTC year leaves 0000-9999.
    pub fn to_utc(&self) -> Result<Datetime, DatetimeError> {
        let (seconds, nanos) = self.instant();
        let days = seconds.div_euclid(86_400);
        let seconds_of_day = seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let year = u16::try_from(year).map_err(|_| DatetimeError::OutOfRange)?;
        #[allow(clippy::cast_possible_truncation)]
        Ok(Datetime {
            date: Date { year, month, day },
            hour: (seconds_of_day / 3600) as u8,
            minute: (seconds_of_day % 3600 / 60) as u8,
            second: (seconds_of_day % 60) as u8,
            nanos,
            offset_minutes: 0,
        })
    }

    /// The UTC whole-second filename rendering `YYYYMMDDTHHMMSSZ`.
    pub fn filename_utc(&self) -> Result<String, DatetimeError> {
        let utc = self.to_utc()?;
        Ok(format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            utc.date.year, utc.date.month, utc.date.day, utc.hour, utc.minute, utc.second
        ))
    }
}

impl fmt::Display for Datetime {
    /// Canonical rendering: uppercase `T`, trimmed fractional seconds, and a
    /// numeric `+HH:MM`/`-HH:MM` offset.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}T{:02}:{:02}:{:02}",
            self.date, self.hour, self.minute, self.second
        )?;
        if self.nanos != 0 {
            let mut fraction = format!("{:09}", self.nanos);
            while fraction.ends_with('0') {
                fraction.pop();
            }
            write!(f, ".{fraction}")?;
        }
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let magnitude = self.offset_minutes.unsigned_abs();
        write!(f, "{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_canonical_forms() -> Result<(), DatetimeError> {
        let value = Datetime::parse("2026-08-22T11:36:14+02:00")?;
        assert_eq!(value.to_string(), "2026-08-22T11:36:14+02:00");
        assert_eq!(value.offset_minutes(), 120);
        let utc = Datetime::parse("2026-08-22T09:36:14+00:00")?;
        assert!(value.same_instant(&utc));
        assert_eq!(value.to_utc()?.to_string(), "2026-08-22T09:36:14+00:00");
        Ok(())
    }

    #[test]
    fn fractional_seconds_trim_trailing_zeros() -> Result<(), DatetimeError> {
        let value = Datetime::parse("2026-08-22T11:36:14.100+02:00")?;
        assert_eq!(value.to_string(), "2026-08-22T11:36:14.1+02:00");
        let zero = Datetime::parse("2026-08-22T11:36:14.000+02:00")?;
        assert_eq!(zero.to_string(), "2026-08-22T11:36:14+02:00");
        let nine = Datetime::parse("2026-08-22T11:36:14.123456789+02:00")?;
        assert_eq!(nine.to_string(), "2026-08-22T11:36:14.123456789+02:00");
        assert_eq!(
            Datetime::parse("2026-08-22T11:36:14.1234567890+02:00"),
            Err(DatetimeError::Malformed)
        );
        Ok(())
    }

    #[test]
    fn rejects_multibyte_input_without_slicing_mid_codepoint() {
        // Every offset below lands inside a multibyte character; parsing must
        // reject rather than panic.
        for text in [
            "2026-08-22T11:36:1é+02:00",
            "2026-08-2é11:36:14+02:00",
            "aaaaaaaaa€aaaaaaaaaa",
            "2026-08-22T11:36:14é02:00",
            "2026-08-22T11:36:14.5é+02:00",
        ] {
            assert!(
                Datetime::parse(text).is_err(),
                "expected rejection for {text}"
            );
        }
        for text in ["2026-08-é2", "20é6-08-22", "2026-0é-22"] {
            assert!(Date::parse(text).is_err(), "expected rejection for {text}");
        }
    }

    #[test]
    fn rejects_missing_and_negative_zero_offsets() {
        assert_eq!(
            Datetime::parse("2026-08-22T11:36:14"),
            Err(DatetimeError::OffsetRequired)
        );
        assert_eq!(
            Datetime::parse("2026-08-22T11:36:14Z"),
            Err(DatetimeError::OffsetRequired)
        );
        assert_eq!(
            Datetime::parse("2026-08-22T11:36:14-00:00"),
            Err(DatetimeError::NegativeZeroOffset)
        );
        assert_eq!(
            Datetime::parse("2026-08-22T25:00:00+00:00"),
            Err(DatetimeError::OutOfRange)
        );
    }

    #[test]
    fn filename_rendering_crosses_utc_date_boundaries() -> Result<(), DatetimeError> {
        let late = Datetime::parse("2026-08-23T00:30:00+02:00")?;
        assert_eq!(late.filename_utc()?, "20260822T223000Z");
        let early = Datetime::parse("2026-08-22T01:15:00-05:00")?;
        assert_eq!(early.filename_utc()?, "20260822T061500Z");
        Ok(())
    }

    #[test]
    fn unix_millis_matches_fixture_instance() -> Result<(), DatetimeError> {
        let created = Datetime::parse("2026-08-22T08:45:00+02:00")?;
        assert_eq!(created.unix_millis(), 1_787_381_100_000);
        Ok(())
    }

    #[test]
    fn builds_canonical_values_from_an_injected_clock_instant() -> Result<(), DatetimeError> {
        // The same instant in two offsets, and the UTC spelling of zero.
        let utc = Datetime::from_unix_millis(1_787_381_100_000, 0)?;
        assert_eq!(utc.to_string(), "2026-08-22T06:45:00+00:00");
        let local = Datetime::from_unix_millis(1_787_381_100_000, 120)?;
        assert_eq!(local.to_string(), "2026-08-22T08:45:00+02:00");
        assert!(utc.same_instant(&local));
        // Sub-second precision is retained to the millisecond.
        let fraction = Datetime::from_unix_millis(1_787_381_100_250, -300)?;
        assert_eq!(fraction.to_string(), "2026-08-22T01:45:00.25-05:00");
        // A negative instant stays on the correct civil day.
        let epoch = Datetime::from_unix_millis(-1, 0)?;
        assert_eq!(epoch.to_string(), "1969-12-31T23:59:59.999+00:00");
        assert_eq!(
            Datetime::from_unix_millis(0, 1440),
            Err(DatetimeError::OutOfRange)
        );
        Ok(())
    }

    #[test]
    fn dates_validate_the_calendar() {
        assert!(Date::parse("2026-08-22").is_ok());
        assert!(Date::parse("2024-02-29").is_ok());
        assert_eq!(Date::parse("2026-02-29"), Err(DatetimeError::OutOfRange));
        assert_eq!(Date::parse("2026-13-01"), Err(DatetimeError::OutOfRange));
        assert_eq!(Date::parse("2026-8-22"), Err(DatetimeError::Malformed));
    }
}
