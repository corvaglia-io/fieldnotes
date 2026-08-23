//! The fixture Field: a deterministic child process that speaks the approved
//! Field protocol on demand, including on purpose badly.
//!
//! Protocol v1 was frozen by A2 on 2026-08-23. The capability slices, declared
//! property names, and source scopes this executable emits remain
//! **illustrative** rather than normative: each becomes normative only when its
//! own Field's release gate approves that Field's manifest and fixtures.
//!
//! # What it is
//!
//! A conformance counterparty. It has no network, no account, no credential, no
//! wall clock, and no randomness: every instant, identifier, cursor, and byte it
//! emits is a fixture constant, so two runs of one scenario are byte-identical.
//! It is **not** the `local` Field, which is release `0.1.1` work and requires
//! an approved A2.
//!
//! # How it is driven
//!
//! Argv carries exactly the protocol's one operation token, so the process looks
//! to core exactly like a real Field:
//!
//! ```text
//! fieldnotes-field-fixture describe
//! fieldnotes-field-fixture collect
//! ```
//!
//! The scenario is selected by the environment instead, because adding a second
//! argv token would make the fixture stop looking like a conforming Field:
//!
//! | Variable | Meaning |
//! |---|---|
//! | `FIELDNOTES_FIXTURE_SCENARIO` | required; the scenario name, see [`Scenario`] |
//! | `FIELDNOTES_FIXTURE_EXIT_CODE` | overrides the scenario's exit code |
//! | `FIELDNOTES_FIXTURE_LEAK_VALUE` | a value the fixture deliberately leaks, for the negative control that proves the secret-canary scan is not vacuous |
//!
//! No variable ever carries a real secret, and the fixture never reads any
//! variable it was not given.

mod manifests;
mod records;
mod scenarios;

use std::io::{BufReader, Write};
use std::process::ExitCode;

use fieldnotes_field_protocol::codes::ExitCode as ProtocolExit;
use fieldnotes_field_protocol::host::{Operation, read_core_frame};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::CoreFrame;

use scenarios::{Scenario, ScenarioOutcome};

/// The environment variable that selects a scenario.
pub const SCENARIO_VARIABLE: &str = "FIELDNOTES_FIXTURE_SCENARIO";

/// The environment variable that overrides the scenario's exit code.
pub const EXIT_CODE_VARIABLE: &str = "FIELDNOTES_FIXTURE_EXIT_CODE";

/// The environment variable carrying a value the fixture deliberately leaks.
pub const LEAK_VARIABLE: &str = "FIELDNOTES_FIXTURE_LEAK_VALUE";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let operation = match arguments.as_slice() {
        [token] if token == Operation::Describe.as_str() => Operation::Describe,
        [token] if token == Operation::Collect.as_str() => Operation::Collect,
        [token] => {
            report(&format!(
                "fieldnotes-field-fixture: unknown operation {token:?}; protocol v1 has exactly \
                 two, 'describe' and 'collect'"
            ));
            return ExitCode::from(ProtocolExit::Usage.as_raw());
        }
        _ => {
            report(
                "fieldnotes-field-fixture: exactly one operation token is expected, either \
                 'describe' or 'collect'",
            );
            return ExitCode::from(ProtocolExit::Usage.as_raw());
        }
    };

    let Some(name) = std::env::var(SCENARIO_VARIABLE).ok() else {
        report(&format!(
            "fieldnotes-field-fixture: {SCENARIO_VARIABLE} is not set, so there is no scenario to \
             run; this executable is fixture-driven by design"
        ));
        return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
    };
    let Some(scenario) = Scenario::parse(&name) else {
        report(&format!(
            "fieldnotes-field-fixture: {name:?} is not a known scenario. Known scenarios: {}",
            Scenario::names().join(", ")
        ));
        return ExitCode::from(ProtocolExit::ConfigInvalid.as_raw());
    };

    let outcome = match operation {
        Operation::Describe => run_describe(scenario),
        Operation::Collect => run_collect(scenario),
    };
    let code = match std::env::var(EXIT_CODE_VARIABLE)
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
    {
        Some(override_code) => override_code,
        None => outcome.exit_code,
    };
    ExitCode::from(code)
}

/// Writes one line to standard error, which is the only thing that ever goes
/// there: standard output carries protocol frames and nothing else.
fn report(message: &str) {
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "{message}");
    let _ = stderr.flush();
}

fn run_describe(scenario: Scenario) -> ScenarioOutcome {
    // Reading the request first is what makes negotiation a negotiation: the
    // Field selects from what core offered rather than announcing.
    match read_core_frame(
        BufReader::new(std::io::stdin()),
        Limits::ceilings().max_frame_bytes,
    ) {
        Ok(Some(CoreFrame::Describe(request))) => scenarios::describe(scenario, &request),
        Ok(Some(_)) => {
            report(
                "fieldnotes-field-fixture: a describe run expects exactly one describe_request on \
                 standard input",
            );
            ScenarioOutcome::failed(ProtocolExit::Usage)
        }
        Ok(None) => {
            report("fieldnotes-field-fixture: standard input closed before any request arrived");
            ScenarioOutcome::failed(ProtocolExit::Usage)
        }
        Err(error) => {
            report(&format!(
                "fieldnotes-field-fixture: the describe request did not validate: {error}"
            ));
            ScenarioOutcome::failed(ProtocolExit::Usage)
        }
    }
}

fn run_collect(scenario: Scenario) -> ScenarioOutcome {
    let mut input = BufReader::new(std::io::stdin());
    match read_core_frame(&mut input, Limits::ceilings().max_frame_bytes) {
        Ok(Some(CoreFrame::Collect(request))) => scenarios::collect(scenario, &request, &mut input),
        Ok(Some(_)) => {
            report(
                "fieldnotes-field-fixture: a collect run expects a collect_request before any \
                 other frame",
            );
            ScenarioOutcome::failed(ProtocolExit::Usage)
        }
        Ok(None) => {
            report("fieldnotes-field-fixture: standard input closed before any request arrived");
            ScenarioOutcome::failed(ProtocolExit::Usage)
        }
        Err(error) => {
            report(&format!(
                "fieldnotes-field-fixture: the collection request did not validate: {error}"
            ));
            ScenarioOutcome::failed(ProtocolExit::Usage)
        }
    }
}
