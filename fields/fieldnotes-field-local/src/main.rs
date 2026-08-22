//! The `local` Field: a bounded, read-only connector that collects Notes
//! from a configured local directory, speaking Fieldnotes' Field process
//! protocol (A2) over standard input and output as a child process.
//!
//! # What this Field does
//!
//! It walks a configured root directory, maps every regular file it finds
//! onto A1's `file` or `document` Note types, stages the file's original
//! bytes as an artifact for core to hash, and offers a durable resume
//! cursor so a later run can collect only what changed. Because a complete
//! directory walk genuinely is an authoritative snapshot of what exists
//! under the root, this Field also supports authoritative deletion by
//! absence in `snapshot` mode (A2 section 10).
//!
//! # What this Field never does
//!
//! It never reads outside the configured root ([`walk`]), never follows a
//! symlink out of it, never writes to the source, and never touches the
//! network. It never computes a record ID, a capture time, a content hash,
//! a canonical key order, a filename, or an artifact path -- core owns all
//! of those, and the record and artifact types this Field builds
//! ([`record`]) structurally exclude them.
//!
//! # Operation
//!
//! ```text
//! fieldnotes-field-local describe
//! fieldnotes-field-local collect
//! ```
//!
//! Protocol data is read from and written to standard input and output;
//! diagnostics are the only thing this Field ever writes to standard error.

mod classify;
mod collect;
mod config;
mod constants;
mod cursor;
mod describe;
mod hexutil;
mod manifest;
mod record;
mod scope;
mod walk;

use std::io::{BufReader, stdin};
use std::process::ExitCode;

use fieldnotes_field_protocol::codes::ExitCode as ProtocolExit;
use fieldnotes_field_protocol::host::Operation;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [token] if token == Operation::Describe.as_str() => describe::run(BufReader::new(stdin())),
        [token] if token == Operation::Collect.as_str() => collect::run(BufReader::new(stdin())),
        [token] => {
            report(&format!(
                "fieldnotes-field-local: unknown operation {token:?}; protocol v1 has exactly \
                 two, 'describe' and 'collect'"
            ));
            ExitCode::from(ProtocolExit::Usage.as_raw())
        }
        _ => {
            report(
                "fieldnotes-field-local: exactly one operation token is expected, either \
                 'describe' or 'collect'",
            );
            ExitCode::from(ProtocolExit::Usage.as_raw())
        }
    }
}

/// Writes one line to standard error, which carries logs and nothing else:
/// standard output carries protocol frames only.
pub(crate) fn report(message: &str) {
    use std::io::Write;
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "{message}");
    let _ = stderr.flush();
}
