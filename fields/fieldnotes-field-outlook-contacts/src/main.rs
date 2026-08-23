//! The Outlook Contacts Field: a bounded, read-only connector that collects
//! Notes from a Microsoft Graph mailbox's contacts folder, speaking
//! Fieldnotes' Field process protocol (A2) over standard input and output as
//! a child process.
//!
//! # What this Field does
//!
//! It walks a mailbox's default contacts folder through Graph's delta feed,
//! maps each contact onto A1's `contact` Note type, stages a contact's photo
//! as an artifact for core to hash when one exists and the run's retention
//! policy retains it, and offers a durable Graph delta cursor so a later run
//! resumes exactly where the last one left off. Because Graph's delta feed
//! itself reports a contact's removal explicitly, this Field also declares
//! authoritative tombstone deletion (A2 section 10) -- unlike
//! `fields/fieldnotes-field-local`, whose deletion authority instead comes
//! from a complete directory-walk snapshot.
//!
//! # What Fieldnotes does not do here
//!
//! This Field emits **evidence**, not identity resolution: a stated email
//! address or phone number becomes a namespaced `identities` anchor, which
//! the graph layer may later use as evidence when reasoning about people and
//! organizations, but this Field never merges, deduplicates across
//! contacts, or decides that two contacts are the same person. See
//! [`identity`] for the anchors this Field emits and
//! [`crate::record`]'s module documentation for why `occurred_at` is a
//! contact's own last-modified instant.
//!
//! # What this Field never does
//!
//! It never computes a record ID, a capture time, a content hash, a
//! canonical key order, a filename, or an artifact path -- core owns all of
//! those, and the record and artifact types this Field builds ([`record`])
//! structurally exclude them. It never writes to Microsoft Graph:
//! `fieldnotes_msgraph::GraphRequest` can only ever describe a `GET`.
//!
//! # Operation
//!
//! ```text
//! fieldnotes-field-outlook-contacts describe
//! fieldnotes-field-outlook-contacts collect
//! ```

mod collect;
mod config;
mod constants;
mod credential;
mod cursor;
mod describe;
mod graph;
mod identity;
mod manifest;
mod photo;
mod random;
mod record;
mod scope;

use std::io::{BufReader, stdin};
use std::process::ExitCode;

use fieldnotes_domain::Clock;

/// The one OS-backed [`Clock`] implementation this Field ever constructs,
/// for a tombstone's `observed_at`. Library code (everything under
/// [`collect`], [`record`], and the rest) never reads the wall clock
/// directly; it only ever sees this through the injected [`Clock`] trait,
/// which this binary's own composition root supplies.
struct SystemClock;

impl Clock for SystemClock {
    fn unix_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let operation = match fieldnotes_field_sdk::dispatch::parse_operation(
        &arguments,
        "fieldnotes-field-outlook-contacts",
    ) {
        Ok(operation) => operation,
        Err(code) => return ExitCode::from(code),
    };
    match operation {
        fieldnotes_field_protocol::host::Operation::Describe => {
            describe::run(BufReader::new(stdin()))
        }
        fieldnotes_field_protocol::host::Operation::Collect => collect::run(
            BufReader::new(stdin()),
            random::ProcessLocalRandom::new(),
            &SystemClock,
        ),
    }
}

/// Writes one line to standard error, which carries logs and nothing else:
/// standard output carries protocol frames only.
pub(crate) fn report(message: &str) {
    fieldnotes_field_sdk::dispatch::report(message);
}
