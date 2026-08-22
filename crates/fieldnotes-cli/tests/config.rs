//! End-to-end tests for the persistent user profile: its precedence chain,
//! the `fieldnotes config` command surface, and the malformed-file failure
//! mode — all run against the real `fieldnotes` binary.
//!
//! Every invocation sets `FIELDNOTES_CONFIG` to a path inside the test's own
//! [`TempDir`], so these tests never read or write a developer's actual
//! profile.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fieldnotes_store::{Profile, write_profile};
use fieldnotes_test_support::TempDir;

const CONFIG_ENV: &str = "FIELDNOTES_CONFIG";
const NOTEBOOK_ENV: &str = "FIELDNOTES_NOTEBOOK";
const TIMEZONE_ENV: &str = "FIELDNOTES_TIMEZONE";
const LEGACY_OFFSET_ENV: &str = "FIELDNOTES_UTC_OFFSET";

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_fieldnotes")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Pulls one field's raw JSON-encoded value out of a compact, one-line
/// object, e.g. `field_of(text, "root")` on `{"root":"/a"}` returns `/a`.
///
/// A tiny hand-rolled slice is enough here: the CLI's own JSON writer emits a
/// stable, quote-delimited `"key":"value"` shape, and pulling in a JSON
/// parser just for test assertions would be a second dependency for a job a
/// few lines of `str` methods already do.
fn field_of(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let end = json[start..].find('"')? + start;
    // Undo the two escapes a Windows path can trigger (a literal backslash
    // and, in principle, a quote), since the CLI's JSON writer escapes both.
    Some(json[start..end].replace("\\\\", "\\").replace("\\\"", "\""))
}

/// Creates an initialized notebook under `parent` at `name` and returns its
/// path, using a private, never-shared profile so `init`'s own default-
/// recording behavior cannot interfere with the test.
fn init_notebook(parent: &Path, name: &str) -> std::io::Result<PathBuf> {
    let root = parent.join(name);
    let output = Command::new(binary())
        .arg("init")
        .arg(&root)
        .env(CONFIG_ENV, parent.join(format!(".{name}-init-profile")))
        .output()?;
    assert!(output.status.success(), "{}", stderr(&output));
    Ok(root)
}

#[test]
fn notebook_precedence_is_flag_then_env_then_profile_then_discovery() -> std::io::Result<()> {
    let temp = TempDir::new("config-notebook-precedence")?;
    let flagged = init_notebook(temp.path(), "flagged")?;
    let enved = init_notebook(temp.path(), "enved")?;
    let profiled = init_notebook(temp.path(), "profiled")?;
    let unrelated = temp.path().join("unrelated-cwd");
    std::fs::create_dir_all(&unrelated)?;

    let profile_path = temp.path().join("profile");
    write_profile(
        &profile_path,
        &Profile {
            notebook: Some(profiled.clone()),
            timezone: None,
        },
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;

    // Tier 1: the flag wins over everything else.
    let output = Command::new(binary())
        .arg("--notebook")
        .arg(&flagged)
        .args(["status", "--format", "json"])
        .env(NOTEBOOK_ENV, &enved)
        .env(CONFIG_ENV, &profile_path)
        .current_dir(&unrelated)
        .output()?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "root").as_deref(),
        Some(flagged.to_string_lossy().as_ref())
    );

    // Tier 2: no flag, so the environment variable wins over the profile.
    let output = Command::new(binary())
        .args(["status", "--format", "json"])
        .env(NOTEBOOK_ENV, &enved)
        .env(CONFIG_ENV, &profile_path)
        .current_dir(&unrelated)
        .output()?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "root").as_deref(),
        Some(enved.to_string_lossy().as_ref())
    );

    // Tier 3: no flag, no environment variable, so the profile wins over
    // discovery from an unrelated working directory.
    let output = Command::new(binary())
        .args(["status", "--format", "json"])
        .env(CONFIG_ENV, &profile_path)
        .current_dir(&unrelated)
        .output()?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "root").as_deref(),
        Some(profiled.to_string_lossy().as_ref())
    );

    // Tier 4: nothing at all falls back to working-directory discovery, which
    // fails from a directory that is not inside any notebook.
    let output = Command::new(binary())
        .args(["status", "--format", "json"])
        .env(CONFIG_ENV, temp.path().join("no-such-profile"))
        .current_dir(&unrelated)
        .output()?;
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("run `fieldnotes init`"));
    Ok(())
}

#[test]
fn timezone_precedence_is_flag_then_env_then_profile_then_utc() -> std::io::Result<()> {
    let temp = TempDir::new("config-timezone-precedence")?;
    let notebook = init_notebook(temp.path(), "notebook")?;
    let profile_path = temp.path().join("profile");
    write_profile(
        &profile_path,
        &Profile {
            notebook: None,
            timezone: Some("+02:00".to_owned()),
        },
    )
    .map_err(|error| std::io::Error::other(error.to_string()))?;

    let run_note = |envs: &[(&str, &str)]| -> std::io::Result<Output> {
        let mut command = Command::new(binary());
        command
            .arg("--notebook")
            .arg(&notebook)
            .args(["note", "text for offset precedence", "--format", "json"])
            .env(CONFIG_ENV, &profile_path);
        for (key, value) in envs {
            command.env(key, value);
        }
        command.output()
    };

    // Tier 1: the flag beats every environment variable and the profile.
    let mut command = Command::new(binary());
    let output = command
        .arg("--notebook")
        .arg(&notebook)
        .args([
            "--offset",
            "+05:00",
            "note",
            "flag wins",
            "--format",
            "json",
        ])
        .env(TIMEZONE_ENV, "+01:00")
        .env(LEGACY_OFFSET_ENV, "+03:00")
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "occurred_at").map(|value| value.ends_with("+05:00")),
        Some(true)
    );

    // Tier 2a: no flag, so FIELDNOTES_TIMEZONE beats the legacy variable and
    // the profile.
    let output = run_note(&[(TIMEZONE_ENV, "+01:00"), (LEGACY_OFFSET_ENV, "+03:00")])?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "occurred_at").map(|value| value.ends_with("+01:00")),
        Some(true)
    );

    // Tier 2b: only the legacy variable is set, so it still works and beats
    // the profile, keeping existing scripts working.
    let output = run_note(&[(LEGACY_OFFSET_ENV, "+03:00")])?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "occurred_at").map(|value| value.ends_with("+03:00")),
        Some(true)
    );

    // Tier 3: no flag, no environment variable, so the profile's +02:00 wins
    // over the utc default.
    let output = run_note(&[])?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "occurred_at").map(|value| value.ends_with("+02:00")),
        Some(true)
    );

    // Tier 4: an empty profile falls back to the documented utc default.
    let empty_profile = temp.path().join("empty-profile");
    std::fs::write(&empty_profile, b"")?;
    let output = Command::new(binary())
        .arg("--notebook")
        .arg(&notebook)
        .args(["note", "utc default", "--format", "json"])
        .env(CONFIG_ENV, &empty_profile)
        .output()?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        field_of(&stdout(&output), "occurred_at").map(|value| value.ends_with("+00:00")),
        Some(true)
    );
    Ok(())
}

#[test]
fn config_set_then_show_round_trips_and_leaves_no_staging_litter() -> std::io::Result<()> {
    let temp = TempDir::new("config-roundtrip")?;
    let notebook = init_notebook(temp.path(), "notebook")?;
    let profile_path = temp.path().join("profile");

    let set_notebook = Command::new(binary())
        .args(["config", "set", "notebook"])
        .arg(&notebook)
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert!(set_notebook.status.success(), "{}", stderr(&set_notebook));

    let set_timezone = Command::new(binary())
        .args(["config", "set", "timezone", "Europe/Zurich"])
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert!(set_timezone.status.success(), "{}", stderr(&set_timezone));

    let show = Command::new(binary())
        .args(["config", "show", "--format", "json"])
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json = stdout(&show);
    assert_eq!(
        field_of(&show_json, "notebook").as_deref(),
        Some(notebook.to_string_lossy().as_ref())
    );
    assert_eq!(
        field_of(&show_json, "timezone").as_deref(),
        Some("Europe/Zurich")
    );

    // Setting the notebook to something that is not a notebook must fail and
    // must not touch the already-recorded setting.
    let not_a_notebook = temp.path().join("not-a-notebook");
    std::fs::create_dir_all(&not_a_notebook)?;
    let rejected = Command::new(binary())
        .args(["config", "set", "notebook"])
        .arg(&not_a_notebook)
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert_eq!(rejected.status.code(), Some(4));
    assert!(stderr(&rejected).contains("run `fieldnotes init`"));
    let show_again = Command::new(binary())
        .args(["config", "get", "notebook"])
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert_eq!(
        stdout(&show_again).trim(),
        notebook.to_string_lossy(),
        "a rejected `config set` must not overwrite the recorded value"
    );

    // No staged-file litter survives any of the writes above.
    let entries = std::fs::read_dir(temp.path())?;
    for entry in entries {
        let name = entry?.file_name();
        assert!(!name.to_string_lossy().contains("fieldnotes-staged"));
    }
    Ok(())
}

#[test]
fn a_malformed_profile_fails_actionably_instead_of_falling_back_silently() -> std::io::Result<()> {
    let temp = TempDir::new("config-malformed")?;
    let notebook = init_notebook(temp.path(), "notebook")?;
    let profile_path = temp.path().join("profile");
    std::fs::write(&profile_path, b"not-a-recognized-setting = value\n")?;

    let output = Command::new(binary())
        .arg("--notebook")
        .arg(&notebook)
        .args(["status"])
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    let message = stderr(&output);
    assert!(message.contains("malformed"));
    assert!(message.contains("unknown setting"));
    Ok(())
}

/// The end-to-end scenario: a default notebook and a DST-observing timezone
/// are recorded in a temporary profile, then `note` is run with no
/// `--notebook` from a working directory unrelated to the notebook, and the
/// produced Note both lands in the configured notebook and carries the
/// offset that is actually correct for the current moment in that zone.
#[test]
fn note_from_an_unrelated_directory_uses_the_configured_notebook_and_timezone()
-> std::io::Result<()> {
    let temp = TempDir::new("config-end-to-end")?;
    let notebook = init_notebook(temp.path(), "configured-notebook")?;
    let unrelated = temp.path().join("somewhere-else-entirely");
    std::fs::create_dir_all(&unrelated)?;
    let profile_path = temp.path().join("profile");

    let set_notebook = Command::new(binary())
        .args(["config", "set", "notebook"])
        .arg(&notebook)
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert!(set_notebook.status.success(), "{}", stderr(&set_notebook));
    let set_timezone = Command::new(binary())
        .args(["config", "set", "timezone", "Europe/Zurich"])
        .env(CONFIG_ENV, &profile_path)
        .output()?;
    assert!(set_timezone.status.success(), "{}", stderr(&set_timezone));

    let note = Command::new(binary())
        .args([
            "note",
            "written from an unrelated directory",
            "--format",
            "json",
        ])
        .env(CONFIG_ENV, &profile_path)
        .current_dir(&unrelated)
        .output()?;
    assert!(note.status.success(), "{}", stderr(&note));
    let note_json = stdout(&note);

    // It landed in the configured notebook, not the unrelated cwd.
    let relative_path = field_of(&note_json, "path").unwrap_or_default();
    let written_path = notebook.join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    assert!(
        written_path.is_file(),
        "expected {} to exist under the configured notebook",
        written_path.display()
    );

    // Its offset is the one actually valid for Europe/Zurich right now,
    // proving the whole chain (profile -> timezone spec -> per-instant
    // resolution) is wired together, not just individually correct.
    let occurred_at = field_of(&note_json, "occurred_at").unwrap_or_default();
    let expected_offset = zurich_offset_right_now();
    assert!(
        occurred_at.ends_with(&expected_offset),
        "occurred_at `{occurred_at}` should carry Zurich's current offset {expected_offset}"
    );
    Ok(())
}

/// Computes Europe/Zurich's offset for the current instant independently of
/// the CLI's own resolver, using `jiff` directly, so the assertion above is a
/// genuine cross-check rather than restating the production code.
fn zurich_offset_right_now() -> String {
    let zone = jiff::tz::TimeZone::get("Europe/Zurich").unwrap_or(jiff::tz::TimeZone::UTC);
    let offset = zone.to_offset(jiff::Timestamp::now());
    let total_minutes = offset.seconds() / 60;
    let sign = if total_minutes < 0 { '-' } else { '+' };
    let magnitude = total_minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
}
