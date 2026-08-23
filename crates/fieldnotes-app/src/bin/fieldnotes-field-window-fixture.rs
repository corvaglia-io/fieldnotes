//! A minimal, real Field process used only to test core's collection-window
//! wiring end to end, with a real child process and no tenant, network, or
//! credential.
//!
//! `fields/fieldnotes-field-fixture` already plays this role for the rest of
//! the workspace, but it is driven by an environment variable
//! (`FIELDNOTES_FIXTURE_SCENARIO`) that core's own child-process spawn does
//! not forward — [`fieldnotes_field_protocol::host::FieldSpawn::spawn`]
//! builds the child's environment from an explicit allowlist rather than
//! inheriting or forwarding anything, and the real `fieldnotes` binary's
//! `sync` command widens that allowlist by nothing at all. So a scenario
//! selected only through that variable can be driven by `fieldnotes-app`'s
//! own tests (which set it through
//! [`fieldnotes_field_protocol::host::FieldSpawn::with_env`]) but not by a
//! plain invocation of the real binary, which is exactly what this
//! feature's manual verification needs. This binary needs no such
//! selection for its primary behavior: it always answers the same manifest
//! and the same one record, so a plain `fieldnotes fields add` plus
//! `fieldnotes sync` against it is enough to observe the window this crate's
//! `sync` decided to send.
//!
//! It declares `window_supported: true` and no authentication. On `collect`
//! it ignores the window and the cursor it was given and always reports the
//! same fixed record and a fixed cursor, which is enough to observe, from
//! core's own report: a first run sends a window, a run with a committed
//! cursor does not, and a windowed run never gains deletion authority even
//! when it otherwise claims a complete snapshot. Setting
//! `FIELDNOTES_WINDOW_FIXTURE_SUPPORTS_WINDOW=false` in the child's
//! environment (through `SyncOptions::field_environment`, never through a
//! real process's own environment, which core never forwards) makes it
//! declare no window support instead, for the complementary case.
//!
//! This is deliberately not built on the shared Field-authoring SDK
//! (`fieldnotes-field-sdk`): pulling that crate in only for a test-support
//! binary would make it a real dependency of `fieldnotes-app` rather than a
//! `dev-dependency`, since a `src/bin` target only sees `[dependencies]`.
//! Frames are therefore built as raw JSON and written by hand, exactly as
//! `fields/fieldnotes-field-fixture` builds them, but without that crate's
//! extra `checked_json` round trip through the protocol's own encoder before
//! writing.

use std::io::{BufReader, Write};

use fieldnotes_field_protocol::codes::ExitCode as ProtocolExit;
use fieldnotes_field_protocol::host::read_core_frame;
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{CollectionMode, CoreFrame};
use serde_json::{Value, json};

/// The environment variable that, set to exactly `false`, makes the manifest
/// declare no window support instead of the default `true`.
const SUPPORTS_WINDOW_VAR: &str = "FIELDNOTES_WINDOW_FIXTURE_SUPPORTS_WINDOW";

/// The fixed source scope this Field reports, when a run names none of its
/// own (an incremental run, or a snapshot run that left `--scope` to
/// inference).
const DEFAULT_SCOPE: &str = "window-fixture:root/demo";

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let code = match arguments.as_slice() {
        [operation] if operation == "describe" => run_describe(),
        [operation] if operation == "collect" => run_collect(),
        [operation] => {
            eprintln!("fieldnotes-field-window-fixture: unknown operation {operation:?}");
            ProtocolExit::Usage.as_raw()
        }
        _ => {
            eprintln!(
                "fieldnotes-field-window-fixture: exactly one operation token is expected, \
                 either 'describe' or 'collect'"
            );
            ProtocolExit::Usage.as_raw()
        }
    };
    std::process::ExitCode::from(code)
}

/// Whether the manifest declares `window_supported: true`, from
/// [`SUPPORTS_WINDOW_VAR`]. Defaults to `true` when unset, so the ordinary
/// window-sending case needs no environment at all.
fn declares_window_support() -> bool {
    std::env::var(SUPPORTS_WINDOW_VAR)
        .map(|value| value != "false")
        .unwrap_or(true)
}

fn run_describe() -> u8 {
    let mut input = BufReader::new(std::io::stdin());
    let request = match read_core_frame(&mut input, Limits::ceilings().max_frame_bytes) {
        Ok(Some(CoreFrame::Describe(request))) => *request,
        Ok(Some(_)) => {
            eprintln!("fieldnotes-field-window-fixture: a describe run expects a describe_request");
            return ProtocolExit::Usage.as_raw();
        }
        Ok(None) => {
            eprintln!(
                "fieldnotes-field-window-fixture: standard input closed before a request \
                 arrived"
            );
            return ProtocolExit::Usage.as_raw();
        }
        Err(error) => {
            eprintln!("fieldnotes-field-window-fixture: {error}");
            return ProtocolExit::Usage.as_raw();
        }
    };
    if !request.supported_protocol_versions.as_slice().contains(&1) {
        eprintln!(
            "fieldnotes-field-window-fixture: protocol version mismatch: core offered {:?}, \
             this build supports [1]",
            request.supported_protocol_versions.as_slice()
        );
        return ProtocolExit::Negotiation.as_raw();
    }
    write_frame(&manifest_json(
        request.run_id.as_str(),
        declares_window_support(),
    ));
    ProtocolExit::Completed.as_raw()
}

fn run_collect() -> u8 {
    let mut input = BufReader::new(std::io::stdin());
    let request = match read_core_frame(&mut input, Limits::ceilings().max_frame_bytes) {
        Ok(Some(CoreFrame::Collect(request))) => *request,
        Ok(Some(_)) => {
            eprintln!(
                "fieldnotes-field-window-fixture: a collect run expects a collect_request first"
            );
            return ProtocolExit::Usage.as_raw();
        }
        Ok(None) => {
            eprintln!(
                "fieldnotes-field-window-fixture: standard input closed before a request \
                 arrived"
            );
            return ProtocolExit::Usage.as_raw();
        }
        Err(error) => {
            eprintln!("fieldnotes-field-window-fixture: {error}");
            return ProtocolExit::Usage.as_raw();
        }
    };
    let run_id = request.run_id.as_str();
    let scope = request.snapshot_scope.as_ref().map_or_else(
        || DEFAULT_SCOPE.to_owned(),
        |scope| scope.as_str().to_owned(),
    );
    write_frame(&record_json(run_id, &scope));
    write_frame(&checkpoint_json(
        run_id,
        request.mode == CollectionMode::Snapshot,
        &scope,
    ));
    ProtocolExit::Completed.as_raw()
}

/// Writes one newline-delimited JSON frame to standard output and flushes it,
/// so a piped core never waits on a buffer for a line already written.
fn write_frame(value: &Value) {
    let mut stdout = std::io::stdout();
    if serde_json::to_writer(&mut stdout, value).is_ok() {
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
}

fn manifest_json(run_id: &str, window_supported: bool) -> Value {
    json!({
        "v": 1,
        "type": "manifest",
        "run_id": run_id,
        "protocol_version": 1,
        "protocol_revision": 0,
        "supported_protocol_versions": [1],
        "driver": "window-fixture",
        "driver_version": "0.1.0",
        "field_stem": "local",
        "declared_properties": [],
        "capabilities": [
            {
                "object_kind": "file",
                "note_type": "file",
                "emits_artifacts": false,
                "emits_identity_anchors": false,
                "description": "A fixed test note used only to prove core's collection-window wiring."
            }
        ],
        "source_key": {
            "scope_rule": "window_fixture_root_id",
            "scope_rule_version": 1,
            "scope_shape": "window-fixture:<root-id>",
            "scope_depends_on_field_label": false,
            "identity_shape": "<object-kind>/<path>",
            "identity_includes_object_kind": true,
            "source_version_ordering": "unsupported",
            "stable_across_instances": true
        },
        "auth": {
            "kind": "none",
            "credential_profile_required": false,
            "protected_channel_required": false,
            "refresh_owner": "not_applicable",
            "writes_to_source": false
        },
        "collection": {
            "incremental": true,
            "cursor_format_version": 1,
            "supported_modes": ["incremental", "snapshot"],
            "window_supported": window_supported,
            "refetch": "unsupported",
            "deletion": {
                "tombstones": "unsupported",
                "snapshot": "authoritative",
                "note": "Test-only stub Field for fieldnotes-app's window feature tests; not a real source."
            }
        }
    })
}

fn record_json(run_id: &str, scope: &str) -> Value {
    json!({
        "v": 1,
        "type": "record",
        "run_id": run_id,
        "seq": 1,
        "change": "upsert",
        "source": {
            "scope": scope,
            "identity": "file/window-fixture-note.md"
        },
        "object_kind": "file",
        "note_type": "file",
        "occurred_at": "2026-08-22T09:00:00+00:00",
        "properties": { "title": "Window fixture note" },
        "body": {
            "format": "markdown",
            "text": "# Window fixture note\n\nA fixed test note used only to prove core's collection-window wiring.\n"
        }
    })
}

fn checkpoint_json(run_id: &str, snapshot: bool, scope: &str) -> Value {
    let mut frame = json!({
        "v": 1,
        "type": "checkpoint",
        "run_id": run_id,
        "seq": 2,
        "cursor": "window-fixture:v1:generation=1",
        "cursor_format_version": 1,
        "covers_record_seq_through": 1,
        "records_covered": 1,
        "final": true
    });
    if snapshot && let Some(object) = frame.as_object_mut() {
        object.insert(
            "snapshot".to_owned(),
            json!({ "scope": scope, "state": "complete", "objects_enumerated": 1 }),
        );
    }
    frame
}
