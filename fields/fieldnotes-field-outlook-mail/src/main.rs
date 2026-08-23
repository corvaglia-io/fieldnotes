//! The `outlook_mail` Field: a read-only connector that collects Outlook mail
//! through Microsoft Graph, speaking Fieldnotes' Field process protocol (A2)
//! over standard input and output as a child process.
//!
//! # What this Field does
//!
//! It reads one mail folder of one mailbox through
//! [`fieldnotes_msgraph`], maps each message onto A1's
//! `mail` Note type, stages each retained file attachment's original bytes for
//! core to hash, emits an authoritative tombstone for each message Graph's
//! delta feed reports as removed, and offers a resume cursor carrying Graph's
//! own delta token.
//!
//! # What this Field never does
//!
//! It never writes to the mailbox: the transport can express no HTTP method
//! but `GET`, so a write is structurally impossible rather than merely
//! undocumented. It never acquires, stores, or refreshes a credential -- the
//! access token arrives on the protected channel for exactly this run's
//! requests -- and it never logs, diagnoses, or persists that token anywhere.
//! It never computes a record ID, a capture time, a content hash, a canonical
//! key order, a filename, or an artifact path: core owns all of those, and the
//! record and artifact types this Field builds structurally exclude them.
//!
//! # Operation
//!
//! ```text
//! fieldnotes-field-outlook-mail describe
//! fieldnotes-field-outlook-mail collect
//! ```
//!
//! Protocol data is read from and written to standard input and output;
//! diagnostics are the only thing this Field ever writes to standard error.
//!
//! # Determinism
//!
//! No library module in this crate reads a wall clock, an environment
//! variable, or an operating-system randomness source. This `main` is the
//! composition root and the only place that does: it supplies the real HTTP
//! transport, the real monotonic retry clock, the retry-jitter byte source,
//! and the real wall clock a tombstone's observation instant needs. Fixture
//! mode substitutes recorded responses, a virtual retry clock, a fixed jitter
//! source, and a frozen wall clock, so a fixture-backed run is byte-identical
//! every time.

mod api;
mod attachment;
mod base64;
mod body;
mod collect;
mod config;
mod constants;
mod credential;
mod cursor;
mod describe;
mod errors;
mod mail;
mod manifest;
mod record;
mod replay;
mod scope;

use std::io::{BufReader, stdin};
use std::process::ExitCode;

use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode as ProtocolExit};
use fieldnotes_field_protocol::host::Operation;
use fieldnotes_field_protocol::message::CollectRequest;
use fieldnotes_msgraph::{GraphClient, SystemRetryClock, UreqTransport};

use collect::{GraphAccess, SetupFailure};

/// This executable's name, used to prefix every line it writes to standard
/// error.
pub(crate) const FIELD_NAME: &str = "fieldnotes-field-outlook-mail";

/// The frozen instant fixture mode reports as a tombstone's observation
/// instant, so a fixture-backed run is byte-identical every time.
/// 2026-08-23T00:00:00Z.
const FIXTURE_OBSERVED_AT_MILLIS: u64 = 1_787_443_200_000;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let operation = match fieldnotes_field_sdk::dispatch::parse_operation(&arguments, FIELD_NAME) {
        Ok(operation) => operation,
        Err(code) => return ExitCode::from(code),
    };
    match operation {
        Operation::Describe => describe::run(BufReader::new(stdin())),
        Operation::Collect => run_collect(),
    }
}

/// Chooses this run's Graph transport, clocks, and randomness, then runs the
/// collection.
fn run_collect() -> ExitCode {
    match std::env::var(constants::FIXTURE_SCRIPT_VARIABLE).ok() {
        Some(script) => run_collect_from_fixtures(&script),
        None => run_collect_from_graph(),
    }
}

/// Runs a collection against the real Graph endpoint, with the access token
/// obtained on the protected credential channel.
fn run_collect_from_graph() -> ExitCode {
    collect::run(BufReader::new(stdin()), &SystemWallClock, |request| {
        let token = credential::acquire(&request.run_id, request.credential.as_ref())
            .map_err(setup_failure_from_credential)?;
        Ok(GraphAccess {
            client: GraphClient::new(
                UreqTransport::new(),
                SystemRetryClock::new(),
                JitterRandom::from_process_entropy(),
            ),
            token,
        })
    })
}

/// Runs a collection against recorded, sanitized Graph responses.
///
/// No network call, no credential grant, and no token: a recorded response
/// needs no authorization. See [`replay`].
fn run_collect_from_fixtures(script: &str) -> ExitCode {
    let script = std::path::PathBuf::from(script);
    collect::run(
        BufReader::new(stdin()),
        &FrozenWallClock(FIXTURE_OBSERVED_AT_MILLIS),
        |_request: &CollectRequest| {
            let transport =
                replay::ReplayTransport::load(&script).map_err(|error| SetupFailure {
                    code: DiagnosticCode::ConfigInvalid,
                    exit: ProtocolExit::ConfigInvalid,
                    message: error.to_string(),
                })?;
            Ok(GraphAccess {
                client: GraphClient::new(
                    transport,
                    replay::VirtualClock::new(),
                    replay::FixedJitter,
                ),
                token: replay::placeholder_token(),
            })
        },
    )
}

fn setup_failure_from_credential(error: credential::CredentialError) -> SetupFailure {
    let (code, exit) = error.classify();
    SetupFailure {
        code,
        exit,
        message: error.to_string(),
    }
}

/// The real wall clock, read only for a tombstone's observation instant.
struct SystemWallClock;

impl fieldnotes_domain::Clock for SystemWallClock {
    fn unix_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

/// A wall clock frozen at one instant, for fixture mode.
struct FrozenWallClock(u64);

impl fieldnotes_domain::Clock for FrozenWallClock {
    fn unix_millis(&self) -> u64 {
        self.0
    }
}

/// The retry-jitter byte source the Graph transport needs.
///
/// Retry jitter needs spread, not cryptographic quality -- the transport's own
/// documentation says so -- and this workspace reserves `getrandom` for the
/// CLI crate, so a Field's composition root seeds a small deterministic
/// generator from process entropy rather than adding a randomness dependency
/// to every connector. Every Microsoft Field will need exactly this; see the
/// crate's final report.
struct JitterRandom {
    state: u64,
}

impl JitterRandom {
    /// Seeds from the process identifier and the current instant, which
    /// differ between concurrently retrying processes and between runs. This
    /// is the composition root, the only place allowed to read either.
    fn from_process_entropy() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let seed = (now as u64) ^ (u64::from(std::process::id()) << 32);
        JitterRandom { state: seed | 1 }
    }
}

impl fieldnotes_domain::RandomSource for JitterRandom {
    fn fill_bytes(&mut self, buffer: &mut [u8]) {
        // SplitMix64: small, well-distributed, and entirely self-contained.
        for slot in buffer {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut word = self.state;
            word = (word ^ (word >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            word = (word ^ (word >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            *slot = (word ^ (word >> 31)) as u8;
        }
    }
}

/// Writes one line to standard error, which carries logs and nothing else:
/// standard output carries protocol frames only.
pub(crate) fn report(message: &str) {
    fieldnotes_field_sdk::dispatch::report(message);
}

#[cfg(test)]
mod tests {
    use super::{FrozenWallClock, JitterRandom};
    use fieldnotes_domain::{Clock, RandomSource};

    #[test]
    fn the_frozen_clock_never_moves() {
        let clock = FrozenWallClock(1_787_443_200_000);
        assert_eq!(clock.unix_millis(), clock.unix_millis());
    }

    #[test]
    fn the_jitter_source_spreads_its_output() {
        let mut random = JitterRandom { state: 12_345 };
        let mut buffer = [0u8; 32];
        random.fill_bytes(&mut buffer);
        let distinct: std::collections::BTreeSet<u8> = buffer.iter().copied().collect();
        assert!(
            distinct.len() > 8,
            "jitter must actually spread, got {distinct:?}"
        );
    }

    #[test]
    fn the_jitter_source_is_reproducible_from_its_seed() {
        let mut first = [0u8; 16];
        let mut second = [0u8; 16];
        JitterRandom { state: 99 }.fill_bytes(&mut first);
        JitterRandom { state: 99 }.fill_bytes(&mut second);
        assert_eq!(first, second);
    }
}
