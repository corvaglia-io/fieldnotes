//! Shared harness for the `outlook_calendar` Field's executable conformance
//! cases.
//!
//! Every case here starts the real `fieldnotes-field-outlook-calendar`
//! binary as a **real child process** and talks to it over real pipes
//! through the reusable protocol conformance kit, exercising the same
//! process boundary a live sync run does. No tenant, no network, and no real
//! credential are ever involved:
//!
//! - Graph itself is faked via [`FIXTURE_SCRIPT_ENV`], read only by the
//!   Field's own `main`, which points its Graph transport at a sanitized
//!   recorded response script on disk instead of the real endpoint (see
//!   `src/fixture_transport.rs`);
//! - the protected credential channel is a real Unix domain socket this
//!   harness itself answers, handing back a fixture bearer token that is
//!   never a real secret.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use fieldnotes_field_protocol::conformance::{CollectPlan, CollectRun, FieldUnderTest};
use fieldnotes_field_protocol::grammar::{GrantId, OffsetDatetime, ProfileRef, PropertyNameToken};
use fieldnotes_field_protocol::message::{
    ChannelDescriptor, ChannelKind, CredentialGrant, FieldEvent, Manifest, RecordEvent, Window,
};
use fieldnotes_field_protocol::value::PropertyValue;
use fieldnotes_test_support::TempDir;

/// The `outlook_calendar` Field's pinned absolute path, resolved by Cargo.
pub const FIELD_EXECUTABLE: &str = env!("CARGO_BIN_EXE_fieldnotes-field-outlook-calendar");

/// The configured Field ID every case uses.
pub const FIELD_ID: &str = "outlook_calendar_work";

/// The non-secret tenant this harness configures every case against.
pub const TENANT_ID: &str = "8d820000-0000-7000-8000-000000000001";

/// The environment variable this Field's `main` reads to select the
/// sanitized fixture transport instead of the real Graph endpoint.
pub const FIXTURE_SCRIPT_ENV: &str = "FIELDNOTES_OUTLOOK_CALENDAR_FIXTURE_SCRIPT";

/// A run identifier for a describe run.
pub const DESCRIBE_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000001";

/// A run identifier for the first collect run in a case.
pub const COLLECT_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000002";

/// A run identifier for a second, resuming collect run.
pub const RESUME_RUN: &str = "1a4c9f2e-0000-4000-8000-000000000003";

fn config_key(name: &str) -> PropertyNameToken {
    PropertyNameToken::parse(name).unwrap_or_else(|error| panic!("{name} must be a key: {error}"))
}

/// One scripted Graph HTTP response, written into a fixture script file the
/// Field's own `main` loads in place of a real network call.
pub struct ScriptEntry {
    status: u16,
    headers: Vec<(String, String)>,
    body: serde_json::Value,
}

/// A successful `200` scripted response.
pub fn ok(body: serde_json::Value) -> ScriptEntry {
    ScriptEntry {
        status: 200,
        headers: Vec::new(),
        body,
    }
}

/// A scripted response carrying an HTTP status other than `200`, with a
/// `Retry-After` header when `retry_after_seconds` is given.
pub fn status(code: u16, retry_after_seconds: Option<u64>, body: serde_json::Value) -> ScriptEntry {
    let headers = retry_after_seconds
        .map(|seconds| vec![("Retry-After".to_owned(), seconds.to_string())])
        .unwrap_or_default();
    ScriptEntry {
        status: code,
        headers,
        body,
    }
}

fn write_script(path: &std::path::Path, entries: Vec<ScriptEntry>) {
    let json: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "status": entry.status,
                "headers": entry.headers,
                "body": entry.body,
            })
        })
        .collect();
    std::fs::write(
        path,
        serde_json::to_vec(&json).unwrap_or_else(|error| panic!("script must encode: {error}")),
    )
    .unwrap_or_else(|error| panic!("script must be writable: {error}"));
}

/// One Graph `calendarView` page envelope.
pub fn page(
    value: Vec<serde_json::Value>,
    next_link: Option<&str>,
    delta_link: Option<&str>,
) -> serde_json::Value {
    let mut object = serde_json::json!({ "value": value });
    if let Some(link) = next_link {
        object["@odata.nextLink"] = serde_json::json!(link);
    }
    if let Some(link) = delta_link {
        object["@odata.deltaLink"] = serde_json::json!(link);
    }
    object
}

/// A present Graph event, with the repository's fictional cast as
/// participants.
pub fn event_json(id: &str, subject: &str, start: &str, end: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "subject": subject,
        "start": {"dateTime": start, "timeZone": "UTC"},
        "end": {"dateTime": end, "timeZone": "UTC"},
        "isAllDay": false,
        "isCancelled": false,
        "organizer": {"emailAddress": {"address": "sam@example.net"}},
        "attendees": [
            {"emailAddress": {"address": "alice@example.com"}},
            {"emailAddress": {"address": "bob@example.net"}}
        ],
        "type": "singleInstance",
        "webLink": format!("https://outlook.office.com/calendar/item/{id}"),
        "changeKey": "calendar-version-1",
        "responseStatus": {"response": "accepted"}
    })
}

/// An all-day Graph event, spanning whole UTC-midnight-bounded days.
pub fn all_day_event_json(
    id: &str,
    subject: &str,
    start_date: &str,
    end_date: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "subject": subject,
        "start": {"dateTime": format!("{start_date}T00:00:00.0000000"), "timeZone": "UTC"},
        "end": {"dateTime": format!("{end_date}T00:00:00.0000000"), "timeZone": "UTC"},
        "isAllDay": true,
        "isCancelled": false,
        "organizer": {"emailAddress": {"address": "sam@example.net"}},
        "attendees": [],
        "type": "singleInstance",
        "webLink": format!("https://outlook.office.com/calendar/item/{id}"),
        "changeKey": "calendar-version-1",
        "responseStatus": {"response": "organizer"}
    })
}

/// One expanded occurrence of a recurring series.
pub fn occurrence_json(
    id: &str,
    series_master_id: &str,
    subject: &str,
    start: &str,
    end: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "subject": subject,
        "start": {"dateTime": start, "timeZone": "UTC"},
        "end": {"dateTime": end, "timeZone": "UTC"},
        "isAllDay": false,
        "isCancelled": false,
        "organizer": {"emailAddress": {"address": "sam@example.net"}},
        "attendees": [],
        "type": "occurrence",
        "seriesMasterId": series_master_id,
        "webLink": format!("https://outlook.office.com/calendar/item/{id}"),
        "changeKey": "calendar-version-1",
        "responseStatus": {"response": "accepted"}
    })
}

/// An authoritative Graph delta removal marker.
pub fn removed_json(id: &str) -> serde_json::Value {
    serde_json::json!({ "id": id, "@removed": {"reason": "deleted"} })
}

/// Runs a detached fake "core" credential server, answering every connection
/// with a granted fixture bearer token. Detached rather than joined: a
/// single [`Case`] may drive several collect runs, each opening its own
/// connection, and the test process ending reclaims the thread either way.
fn spawn_fake_credential_server() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let socket_path = PathBuf::from(format!(
        "/tmp/fn-cal-it-{}-{unique}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).unwrap_or_else(|error| panic!("bind: {error}"));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let mut reader = BufReader::new(
                stream
                    .try_clone()
                    .unwrap_or_else(|error| panic!("clone: {error}")),
            );
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let response = serde_json::json!({
                "v": 1,
                "type": "credential_response",
                "run_id": "1a4c9f2e-0000-4000-8000-000000000002",
                "grant_id": "abcdef0123456789",
                "outcome": "granted",
                "material": {
                    "kind": "bearer_token",
                    "value": "FIXTURE-NOT-A-REAL-TOKEN"
                }
            });
            let mut bytes = serde_json::to_vec(&response).unwrap_or_default();
            bytes.push(b'\n');
            let _ = stream.write_all(&bytes);
        }
    });
    socket_path
}

fn credential_grant(socket_path: &std::path::Path) -> CredentialGrant {
    CredentialGrant {
        profile_ref: ProfileRef::parse("outlook_calendar_work")
            .unwrap_or_else(|error| panic!("must parse: {error}")),
        grant_id: GrantId::parse("abcdef0123456789")
            .unwrap_or_else(|error| panic!("must parse: {error}")),
        channel: ChannelDescriptor {
            kind: ChannelKind::UnixSocketPath,
            path: Some(
                socket_path
                    .to_str()
                    .unwrap_or_else(|| panic!("utf8 path"))
                    .to_owned(),
            ),
        },
        expires_at: OffsetDatetime::parse("2099-01-01T00:00:00+00:00")
            .unwrap_or_else(|error| panic!("must parse: {error}")),
        scopes: Some(vec!["Calendars.Read".to_owned()]),
    }
}

/// One conformance case: a pinned executable, a fixture Graph script, and a
/// real fake credential channel.
pub struct Case {
    field: FieldUnderTest,
    socket_path: PathBuf,
    _script_dir: TempDir,
}

impl Case {
    /// Builds a case whose Graph transport answers `entries` in order.
    pub fn new(label: &str, entries: Vec<ScriptEntry>) -> Self {
        let script_dir = TempDir::new(&format!("outlook-calendar-{label}"))
            .unwrap_or_else(|error| panic!("a script directory is required: {error}"));
        let script_path = script_dir.path().join("script.json");
        write_script(&script_path, entries);
        let socket_path = spawn_fake_credential_server();
        let field = FieldUnderTest::new(FIELD_EXECUTABLE)
            .with_idle(Duration::from_secs(10))
            .with_wait(Duration::from_secs(15))
            .with_env(
                FIXTURE_SCRIPT_ENV,
                script_path.to_str().unwrap_or_else(|| panic!("utf8 path")),
            );
        Case {
            field,
            socket_path,
            _script_dir: script_dir,
        }
    }

    /// Runs a describe run and requires a manifest.
    pub fn manifest(&self) -> Manifest {
        match self.field.describe(DESCRIBE_RUN, Some(FIELD_ID)) {
            Ok(run) => run.manifest.unwrap_or_else(|| {
                panic!(
                    "the describe run produced no manifest: {:?} {:?}",
                    run.rejection, run.detail
                )
            }),
            Err(error) => panic!("the describe run could not start: {error}"),
        }
    }

    fn with_tenant_and_credential(&self, mut plan: CollectPlan) -> CollectPlan {
        plan.config.insert(
            config_key("tenant_id"),
            PropertyValue::Text(TENANT_ID.to_owned()),
        );
        plan.credential = Some(credential_grant(&self.socket_path));
        plan
    }

    /// A first, windowed incremental plan.
    pub fn windowed_plan(&self, run_id: &str, from: &str, to: &str) -> CollectPlan {
        let plan = CollectPlan::incremental(run_id, FIELD_ID, self.staging_dir())
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"));
        let mut plan = self.with_tenant_and_credential(plan);
        plan.window = Some(Window {
            from: OffsetDatetime::parse(from).unwrap_or_else(|error| panic!("must parse: {error}")),
            to: OffsetDatetime::parse(to).unwrap_or_else(|error| panic!("must parse: {error}")),
        });
        plan
    }

    /// A plan carrying no credential, for a config-refusal case that never
    /// needs to reach the credential channel or Graph at all.
    pub fn windowed_plan_without_tenant(&self, run_id: &str, from: &str, to: &str) -> CollectPlan {
        let mut plan = CollectPlan::incremental(run_id, FIELD_ID, self.staging_dir())
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"));
        plan.window = Some(Window {
            from: OffsetDatetime::parse(from).unwrap_or_else(|error| panic!("must parse: {error}")),
            to: OffsetDatetime::parse(to).unwrap_or_else(|error| panic!("must parse: {error}")),
        });
        plan
    }

    /// A resuming incremental plan carrying a previously committed cursor.
    pub fn resume_plan(&self, run_id: &str, cursor: &str) -> CollectPlan {
        let plan = CollectPlan::incremental(run_id, FIELD_ID, self.staging_dir())
            .unwrap_or_else(|error| panic!("the plan is invalid: {error}"));
        let plan = self.with_tenant_and_credential(plan);
        plan.with_cursor(cursor, 1)
            .unwrap_or_else(|error| panic!("the cursor is invalid: {error}"))
    }

    fn staging_dir(&self) -> PathBuf {
        std::env::temp_dir()
    }

    /// Runs a collect run against a manifest already obtained from
    /// [`Case::manifest`].
    pub fn collect(&self, manifest: &Manifest, plan: &CollectPlan) -> CollectRun {
        match self.field.collect(manifest, plan) {
            Ok(run) => run,
            Err(error) => panic!("the collect run could not start: {error}"),
        }
    }
}

/// Every record event a run accepted, in order.
pub fn record_events(run: &CollectRun) -> Vec<&RecordEvent> {
    run.events
        .iter()
        .filter_map(|event| match event {
            FieldEvent::Record(record) => Some(record.as_ref()),
            _ => None,
        })
        .collect()
}
