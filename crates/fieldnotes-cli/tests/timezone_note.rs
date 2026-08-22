//! Proves the DST-correctness requirement where it actually matters: a real
//! Note, written through the real `create_note` use case, carries a
//! different offset for two instants roughly six months apart in a
//! daylight-saving zone.
//!
//! [`fieldnotes_cli`]'s own zone-to-offset resolver (`TimeZoneSpec`, in
//! `src/timezone.rs`) is private to that binary crate, so this test
//! recomputes the same handful of lines directly against `jiff` — the same
//! library the binary depends on — rather than reaching into a private
//! module. That keeps this an independent check that the whole pipeline
//! (a resolved offset feeding `Kernel` feeding `create_note`) is wired
//! correctly, not a restatement of the production resolver.

use std::path::Path;

use fieldnotes_app::{Kernel, NoteOutcome, NoteRequest, create_note, init};
use fieldnotes_store::Notebook;
use fieldnotes_test_support::{CountingRandom, FixedClock, TempDir};

/// 2026-01-15T00:00:00Z: Zurich is on standard time, `+01:00`.
const JANUARY_2026_UTC_MILLIS: i64 = 1_768_435_200_000;
/// 2026-07-15T00:00:00Z: Zurich is on daylight saving, `+02:00`.
const JULY_2026_UTC_MILLIS: i64 = 1_784_073_600_000;

fn zurich_offset_minutes(unix_millis: i64) -> i16 {
    let zone = jiff::tz::TimeZone::get("Europe/Zurich").unwrap_or(jiff::tz::TimeZone::UTC);
    let seconds = unix_millis.div_euclid(1000);
    let timestamp = jiff::Timestamp::from_second(seconds).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let offset_seconds = zone.to_offset(timestamp).seconds();
    i16::try_from(offset_seconds / 60).unwrap_or(0)
}

fn write_one_note(
    root: &Path,
    unix_millis: i64,
    offset_minutes: i16,
) -> Result<NoteOutcome, Box<dyn std::error::Error>> {
    let millis = u64::try_from(unix_millis)?;
    let mut kernel = Kernel::new(FixedClock(millis), CountingRandom::new(1), offset_minutes)?;
    let outcome = init(&mut kernel, root, None)?;
    let notebook = Notebook::open(&outcome.root)?;
    let request = NoteRequest::text("Hello from a DST-correctness test.");
    let note = create_note(&mut kernel, &notebook, &request)?;
    Ok(note)
}

#[test]
fn a_note_written_six_months_apart_in_a_dst_zone_carries_the_correct_offset()
-> Result<(), Box<dyn std::error::Error>> {
    let january_offset = zurich_offset_minutes(JANUARY_2026_UTC_MILLIS);
    let july_offset = zurich_offset_minutes(JULY_2026_UTC_MILLIS);
    assert_eq!(
        january_offset, 60,
        "Zurich in January is standard time, +01:00"
    );
    assert_eq!(
        july_offset, 120,
        "Zurich in July is daylight saving, +02:00"
    );
    assert_ne!(
        january_offset, july_offset,
        "a DST-observing zone must not resolve to the same offset year-round"
    );

    let temp = TempDir::new("timezone-note")?;
    let january_note = write_one_note(
        &temp.path().join("january"),
        JANUARY_2026_UTC_MILLIS,
        january_offset,
    )?;
    let july_note = write_one_note(&temp.path().join("july"), JULY_2026_UTC_MILLIS, july_offset)?;

    assert_eq!(
        january_note.occurred_at.to_string(),
        "2026-01-15T01:00:00+01:00"
    );
    assert_eq!(
        january_note.captured_at.to_string(),
        "2026-01-15T01:00:00+01:00"
    );
    assert_eq!(
        july_note.occurred_at.to_string(),
        "2026-07-15T02:00:00+02:00"
    );
    assert_eq!(
        july_note.captured_at.to_string(),
        "2026-07-15T02:00:00+02:00"
    );

    // Both Notes are real, durable files, not just in-memory values.
    assert!(january_note.write.path.is_file());
    assert!(july_note.write.path.is_file());
    Ok(())
}
