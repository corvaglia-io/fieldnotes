//! The injected environment every use case runs in.
//!
//! Nothing in the library reads the wall clock or an operating-system random
//! source directly: the composition root supplies both, so a test can replay
//! the same operations and get byte-identical files.

use fieldnotes_domain::{
    Clock, Datetime, FieldId, FieldStemRegistry, RandomSource, RecordId, RecordIdGenerator,
    RecordKind,
};

use crate::error::AppError;

/// The built-in Field that produces user-authored Notes.
pub const SELF_FIELD: &str = "self";

/// A clock, an ID generator, and the client-local UTC offset.
#[derive(Debug)]
pub struct Kernel<C, R> {
    ids: RecordIdGenerator<C, R>,
    offset_minutes: i16,
}

impl<C: Clock, R: RandomSource> Kernel<C, R> {
    /// Builds a kernel from an injected clock, random source, and the numeric
    /// UTC offset that generated datetimes are rendered in.
    ///
    /// The offset is explicit because A1 requires every datetime to carry one
    /// and forbids a timezone-less value. Zero renders as `+00:00`.
    pub fn new(clock: C, random: R, offset_minutes: i16) -> Result<Self, AppError> {
        if !(-1439..=1439).contains(&offset_minutes) {
            return Err(AppError::InvalidOffset {
                minutes: i32::from(offset_minutes),
            });
        }
        Ok(Kernel {
            ids: RecordIdGenerator::new(clock, random),
            offset_minutes,
        })
    }

    /// The configured UTC offset in minutes east of UTC.
    #[must_use]
    pub fn offset_minutes(&self) -> i16 {
        self.offset_minutes
    }

    /// Generates a record ID together with the creation datetime that agrees
    /// with the ID's own UUIDv7 timestamp.
    ///
    /// Deriving the datetime from the generated ID rather than from a second
    /// clock reading is what makes the two agree to the millisecond, which the
    /// instance-metadata contract requires and the Note corpus follows for
    /// `captured_at`.
    pub fn new_record(&mut self, kind: RecordKind) -> Result<(RecordId, Datetime), AppError> {
        let id = self.ids.generate(kind)?;
        let millis = i64::try_from(id.uuid().unix_millis())
            .map_err(|_| AppError::Datetime(fieldnotes_domain::DatetimeError::OutOfRange))?;
        let created_at = Datetime::from_unix_millis(millis, self.offset_minutes)?;
        Ok((id, created_at))
    }

    /// Generates a Field process run identifier and the instant it names.
    ///
    /// A run identifier is core's own name for one bounded child process. It is
    /// opaque to the Field, is never a notebook record ID, and never appears in
    /// a notebook file, which is why it carries no record-kind prefix. It still
    /// comes from this kernel's injected clock and randomness source, so a test
    /// that fixes both replays the same run identifier and the same deadline.
    pub fn new_run(&mut self) -> Result<(String, Datetime), AppError> {
        let uuid = self.ids.generate_uuid()?;
        let millis = i64::try_from(uuid.unix_millis())
            .map_err(|_| AppError::Datetime(fieldnotes_domain::DatetimeError::OutOfRange))?;
        let started_at = Datetime::from_unix_millis(millis, self.offset_minutes)?;
        Ok((uuid.to_string(), started_at))
    }

    /// Generates a single-use authorization token for one run's protected
    /// credential channel: 64 lowercase hexadecimal characters.
    ///
    /// A2 section 12's `grant_id` is not credential material — it authorizes
    /// nothing outside this run's channel — but it is the value that
    /// distinguishes this run's Field from any other local process that finds
    /// the endpoint, so it comes from two freshly generated identifiers rather
    /// than being derived from the run identifier the Field already knows.
    pub fn new_grant_id(&mut self) -> Result<String, AppError> {
        let mut hex = String::with_capacity(64);
        for _ in 0..2 {
            let uuid = self.ids.generate_uuid()?;
            for character in uuid.to_string().chars().filter(|c| *c != '-') {
                hex.push(character.to_ascii_lowercase());
            }
        }
        Ok(hex)
    }
}

/// The built-in `self` Field ID.
pub fn self_field() -> Result<FieldId, AppError> {
    FieldId::parse(SELF_FIELD, FieldStemRegistry::v1()).map_err(|_| {
        AppError::Record(fieldnotes_format::ValidationError::InvalidFieldId {
            value: SELF_FIELD.to_owned(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_test_support::{CountingRandom, FixedClock};

    #[test]
    fn generated_ids_and_datetimes_agree_and_replay() -> Result<(), AppError> {
        let build = || Kernel::new(FixedClock(1_787_381_100_000), CountingRandom::new(1), 120);
        let mut first = build()?;
        let mut second = build()?;
        let (id, created_at) = first.new_record(RecordKind::Instance)?;
        assert_eq!(created_at.unix_millis(), 1_787_381_100_000);
        assert_eq!(created_at.to_string(), "2026-08-22T08:45:00+02:00");
        assert_eq!(second.new_record(RecordKind::Instance)?, (id, created_at));
        Ok(())
    }

    #[test]
    fn an_impossible_offset_is_rejected() {
        assert!(matches!(
            Kernel::new(FixedClock(0), CountingRandom::new(0), 1440),
            Err(AppError::InvalidOffset { .. })
        ));
    }
}
