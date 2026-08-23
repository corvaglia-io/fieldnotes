//! Proves core's collection-window wiring end to end, against a real child
//! process: `crates/fieldnotes-app/src/bin/fieldnotes-field-window-fixture.rs`,
//! a minimal stub Field owned by this crate (see that file's own
//! documentation for why it exists alongside
//! `fields/fieldnotes-field-fixture` rather than reusing it).
//!
//! What is proved here, each against the real binary:
//!
//! 1. A first run — no durable cursor yet — sends a bounded window, at the
//!    seven-day default.
//! 2. A run with a committed cursor sends none.
//! 3. `--window`'s equivalent, [`SyncOptions::window_days`], overrides the
//!    default span.
//! 4. A Field whose manifest declares no window support gets none, even on a
//!    first run.
//! 5. A windowed run never gains deletion authority, even when it otherwise
//!    claims a complete snapshot.
//! 6. The window's endpoints carry an explicit numeric offset.
//!
//! No tenant, network, or credential is used anywhere in this file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fieldnotes_app::{
    AppError, DEFAULT_WINDOW_DAYS, FieldRunOutcome, Kernel, SyncMode, SyncOptions, add_field, init,
    sync, validate_field_id,
};
use fieldnotes_domain::Datetime;
use fieldnotes_store::Notebook;
use fieldnotes_test_support::{CountingRandom, FixedClock, TempDir};

/// 2026-08-22T08:45:00Z in Unix milliseconds, matching every other fixed
/// instant this workspace's tests use.
const FIXED_MILLIS: u64 = 1_787_381_100_000;

/// The environment variable the stub Field reads to declare (or withdraw)
/// window support. See its own module documentation.
const SUPPORTS_WINDOW_VAR: &str = "FIELDNOTES_WINDOW_FIXTURE_SUPPORTS_WINDOW";

fn executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fieldnotes-field-window-fixture"))
}

fn temp(label: &str) -> TempDir {
    match TempDir::new(label) {
        Ok(temp) => temp,
        Err(error) => panic!("could not create a temporary directory: {error}"),
    }
}

fn kernel() -> Kernel<FixedClock, CountingRandom> {
    match Kernel::new(FixedClock(FIXED_MILLIS), CountingRandom::new(1), 0) {
        Ok(kernel) => kernel,
        Err(error) => panic!("the fixed test kernel must build: {error}"),
    }
}

/// A notebook with the stub Field configured under `local_<label>`, with no
/// credential and, when `field_environment` is non-empty, that environment
/// forwarded to the child exactly as `SyncOptions::field_environment`
/// documents.
fn notebook_with_stub_field(
    temp: &TempDir,
    label: &str,
) -> Result<(Notebook, Kernel<FixedClock, CountingRandom>), AppError> {
    let root = temp.path().join("notebook");
    let mut kernel = kernel();
    init(&mut kernel, &root, Some("window-sync-tests"))?;
    let notebook = Notebook::open(&root)?;
    add_field(
        &notebook,
        &validate_field_id("local", label)?,
        executable(),
        BTreeMap::new(),
        true,
    )?;
    Ok((notebook, kernel))
}

fn options(window_days: Option<u64>, field_environment: BTreeMap<String, String>) -> SyncOptions {
    SyncOptions {
        window_days,
        field_environment,
        ..SyncOptions::default()
    }
}

/// Parses a window endpoint's rendered offset, so a span can be computed and
/// so the explicit-numeric-offset contract can be checked on the exact bytes
/// core reported, not merely trusted from the type that produced them.
fn parse_endpoint(text: &str) -> Datetime {
    match Datetime::parse(text) {
        Ok(datetime) => datetime,
        Err(error) => panic!("a reported window endpoint must parse: {text}: {error}"),
    }
}

#[test]
fn a_first_run_sends_the_default_window_and_a_cursored_run_sends_none() -> Result<(), AppError> {
    let temp = temp("window-sync-first-then-cursored");
    let (notebook, mut kernel) = notebook_with_stub_field(&temp, "first_then_cursored")?;
    let field_id = "local_first_then_cursored";

    // Run 1: no durable cursor exists yet, so the window-supporting manifest
    // gets a window, at the seven-day default.
    let first = sync(
        &mut kernel,
        &notebook,
        Some(field_id),
        &options(None, BTreeMap::new()),
    )?;
    let first_report = match first.fields.as_slice() {
        [report] => report,
        other => panic!("exactly one Field report was expected, got {}", other.len()),
    };
    assert_eq!(first_report.outcome, FieldRunOutcome::Complete);
    assert!(
        first_report.cursor_committed,
        "the first run must commit a cursor"
    );
    let window = match &first_report.window {
        Some(window) => window,
        None => panic!("a first run against a window-supporting Field must report a window"),
    };
    let from = parse_endpoint(&window.from);
    let to = parse_endpoint(&window.to);
    let span_millis = to.unix_millis() - from.unix_millis();
    assert_eq!(
        span_millis,
        i64::try_from(DEFAULT_WINDOW_DAYS).unwrap_or(0) * 86_400 * 1000,
        "the default window must span exactly seven days"
    );
    assert!(
        !window.from.ends_with('Z') && !window.to.ends_with('Z'),
        "a window endpoint must never render a bare Z: from={} to={}",
        window.from,
        window.to
    );
    assert!(
        window.from.contains('+') || window.from.contains('-'),
        "a window endpoint must carry an explicit numeric offset: {}",
        window.from
    );

    // Run 2: the cursor `sync` just committed is now durable, so the same
    // window-supporting manifest gets no window at all.
    let second = sync(
        &mut kernel,
        &notebook,
        Some(field_id),
        &options(None, BTreeMap::new()),
    )?;
    let second_report = match second.fields.as_slice() {
        [report] => report,
        other => panic!("exactly one Field report was expected, got {}", other.len()),
    };
    assert_eq!(second_report.outcome, FieldRunOutcome::Complete);
    assert_eq!(
        second_report.window, None,
        "a run with a durable cursor must send no window"
    );
    Ok(())
}

#[test]
fn window_days_overrides_the_default_span() -> Result<(), AppError> {
    let temp = temp("window-sync-override");
    let (notebook, mut kernel) = notebook_with_stub_field(&temp, "override")?;

    let outcome = sync(
        &mut kernel,
        &notebook,
        Some("local_override"),
        &options(Some(1), BTreeMap::new()),
    )?;
    let report = match outcome.fields.as_slice() {
        [report] => report,
        other => panic!("exactly one Field report was expected, got {}", other.len()),
    };
    let window = match &report.window {
        Some(window) => window,
        None => panic!("a first run against a window-supporting Field must report a window"),
    };
    let span_millis =
        parse_endpoint(&window.to).unix_millis() - parse_endpoint(&window.from).unix_millis();
    assert_eq!(
        span_millis,
        86_400 * 1000,
        "a one-day override must span exactly one day"
    );
    Ok(())
}

#[test]
fn a_field_declaring_no_window_support_gets_none_on_a_first_run() -> Result<(), AppError> {
    let temp = temp("window-sync-unsupported");
    let (notebook, mut kernel) = notebook_with_stub_field(&temp, "unsupported")?;

    let mut field_environment = BTreeMap::new();
    field_environment.insert(SUPPORTS_WINDOW_VAR.to_owned(), "false".to_owned());
    let outcome = sync(
        &mut kernel,
        &notebook,
        Some("local_unsupported"),
        &options(None, field_environment),
    )?;
    let report = match outcome.fields.as_slice() {
        [report] => report,
        other => panic!("exactly one Field report was expected, got {}", other.len()),
    };
    assert_eq!(report.outcome, FieldRunOutcome::Complete);
    assert_eq!(
        report.window, None,
        "a Field declaring no window support must receive none even on a first run"
    );
    Ok(())
}

#[test]
fn a_windowed_snapshot_run_never_gains_deletion_authority() -> Result<(), AppError> {
    let temp = temp("window-sync-deletion-authority");
    let (notebook, mut kernel) = notebook_with_stub_field(&temp, "deletion")?;

    let mut run_options = options(None, BTreeMap::new());
    run_options.mode = SyncMode::Snapshot;
    run_options.snapshot_scope = Some("window-fixture:root/demo".to_owned());
    let outcome = sync(&mut kernel, &notebook, Some("local_deletion"), &run_options)?;
    let report = match outcome.fields.as_slice() {
        [report] => report,
        other => panic!("exactly one Field report was expected, got {}", other.len()),
    };
    // The stub Field claims a complete snapshot and the run otherwise
    // completes cleanly, so without a window this would have authorized
    // deletion; asserting completion first is what makes the deletion
    // refusal below attributable to the window and nothing else.
    assert_eq!(report.outcome, FieldRunOutcome::Complete);
    assert!(
        report.window.is_some(),
        "the first snapshot run must be windowed"
    );
    assert_eq!(
        report.deletion.authorized_scope, None,
        "a windowed run must never gain deletion authority"
    );
    assert!(
        report
            .deletion
            .refusals
            .iter()
            .any(|reason| reason.contains("window")),
        "the deletion refusal must name the window as a reason: {:?}",
        report.deletion.refusals
    );
    Ok(())
}
