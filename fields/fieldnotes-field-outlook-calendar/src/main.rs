//! The `outlook_calendar` Field: a read-only, windowed connector that
//! collects calendar events from a signed-in mailbox's default calendar
//! through Microsoft Graph, speaking Fieldnotes' Field process protocol (A2)
//! over its own standard input and output as a child process.
//!
//! # What this Field does
//!
//! It requests `Calendars.Read` and, for each configured run, collects the
//! requested window through Graph's `calendarView/delta`, which expands a
//! recurring series into its instances server-side. It maps each event onto
//! A1's `event` Note type, carrying the interval, organizer, and
//! participants as registered shared properties and everything
//! source-specific under its own `outlook_calendar_` prefix. A later run
//! resumes from Graph's own opaque delta continuation, and Graph's
//! authoritative `@removed` marker becomes this Field's tombstone.
//!
//! # What this Field never does
//!
//! It never writes to Graph -- [`fieldnotes_msgraph`] exposes no way to
//! construct anything but a `GET`. It never claims to have enumerated the
//! whole calendar, so it never declares snapshot deletion authority. It
//! never computes a record ID, a capture time, a content hash, a canonical
//! key order, a filename, or an artifact path -- the record type this Field
//! builds ([`record`]) structurally excludes all of them. It never acquires,
//! refreshes, or reads a credential from anywhere but the one protected
//! channel A2 section 12 describes ([`credential`]).
//!
//! # Operation
//!
//! ```text
//! fieldnotes-field-outlook-calendar describe
//! fieldnotes-field-outlook-calendar collect
//! ```
//!
//! Protocol data is read from and written to standard input and output;
//! diagnostics are the only thing this Field ever writes to standard error.
//!
//! # The composition root
//!
//! This module is the only place in this Field that touches the network,
//! the wall clock, or an OS-influenced random source directly: [`collect`]
//! and every module it calls receive those as injected parameters. The one
//! exception a test needs is [`fixture_transport`], selected here -- and
//! only here -- via [`constants::FIXTURE_SCRIPT_ENV`], so a real
//! child-process test can exercise this binary's actual `main` without a
//! tenant, a network, or credentials, exactly as the fixture Field's own
//! `FIELDNOTES_FIXTURE_EXIT_CODE` environment override already does for a
//! different scenario axis.

mod collect;
mod config;
mod constants;
mod credential;
mod cursor;
mod describe;
mod fixture_transport;
mod graph;
mod manifest;
mod record;

use std::io::{BufReader, stdin};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use fieldnotes_field_protocol::codes::ExitCode as ProtocolExit;
use fieldnotes_msgraph::{RandomSource, SystemRetryClock, UreqTransport};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let operation = match fieldnotes_field_sdk::dispatch::parse_operation(
        &arguments,
        "fieldnotes-field-outlook-calendar",
    ) {
        Ok(operation) => operation,
        Err(code) => return ExitCode::from(code),
    };
    match operation {
        fieldnotes_field_protocol::host::Operation::Describe => {
            describe::run(BufReader::new(stdin()))
        }
        fieldnotes_field_protocol::host::Operation::Collect => run_collect(),
    }
}

fn run_collect() -> ExitCode {
    let observed_now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let random = RealRandomSource::new();
    let clock = SystemRetryClock::new();
    let input = BufReader::new(stdin());

    match std::env::var(constants::FIXTURE_SCRIPT_ENV) {
        Ok(path) => match fixture_transport::FileScriptedTransport::load(&path) {
            Ok(transport) => collect::run(input, transport, clock, random, observed_now_millis),
            Err(error) => {
                report(&format!(
                    "fieldnotes-field-outlook-calendar: could not load fixture script {path:?}: \
                     {error}"
                ));
                ExitCode::from(ProtocolExit::Internal.as_raw())
            }
        },
        Err(_) => collect::run(
            input,
            UreqTransport::new(),
            clock,
            random,
            observed_now_millis,
        ),
    }
}

/// A simple, non-cryptographic random source for Graph retry-jitter only.
///
/// This Field never generates a record ID or any other security-sensitive
/// value -- core owns every ID -- so the jitter
/// [`fieldnotes_msgraph::GraphClient`] asks for needs only enough spread to
/// avoid a retry thundering herd, never cryptographic quality (see that
/// crate's own `unit_interval` documentation). `getrandom` is reserved to
/// `fieldnotes-cli`'s composition root for the one thing in this workspace
/// that does need cryptographic randomness (A1 UUIDv7 generation); pulling
/// it into a second binary for a purpose that does not need it would be
/// exactly the kind of duplicated mechanism this workspace's
/// minimal-dependency preference exists to avoid. Seeded once, here in
/// `main`, from the wall clock and process ID -- an explicit, one-time
/// composition-root reading, never touched by library logic.
struct RealRandomSource {
    state: u64,
}

impl RealRandomSource {
    fn new() -> Self {
        let elapsed_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let seed = elapsed_nanos ^ (u64::from(std::process::id()) << 32);
        // A splitmix64 seed of zero degenerates immediately; force it odd.
        RealRandomSource { state: seed | 1 }
    }
}

impl RandomSource for RealRandomSource {
    fn fill_bytes(&mut self, buffer: &mut [u8]) {
        for chunk in buffer.chunks_mut(8) {
            // splitmix64: a small, well-known, non-cryptographic generator,
            // sufficient for retry-jitter spread and nothing else.
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut mixed = self.state;
            mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            mixed ^= mixed >> 31;
            let bytes = mixed.to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }
}

/// Writes one line to standard error, which carries logs and nothing else:
/// standard output carries protocol frames only.
pub(crate) fn report(message: &str) {
    fieldnotes_field_sdk::dispatch::report(message);
}
