//! End-to-end tests for `fieldnotes fields add/list/status/remove`, run
//! through the real binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fieldnotes_test_support::TempDir;

/// The environment variable overriding the profile file location, set on
/// every invocation so a test run never touches a developer's real profile.
const CONFIG_ENV: &str = "FIELDNOTES_CONFIG";

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_fieldnotes")
}

fn hermetic_config_path(notebook: &Path) -> PathBuf {
    notebook
        .parent()
        .unwrap_or(notebook)
        .join(".fieldnotes-test-profile")
}

fn run(notebook: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(binary())
        .arg("--notebook")
        .arg(notebook)
        .args(args)
        .env(CONFIG_ENV, hermetic_config_path(notebook))
        .output()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn add_list_status_remove_round_trip_through_the_binary() -> std::io::Result<()> {
    let temp = TempDir::new("cli-fields")?;
    let root = temp.path().join("notebook");
    let init = run(&root, &["init"])?;
    assert!(init.status.success(), "{}", stderr(&init));

    // `self` cannot be added: it is reserved, not a registered external stem.
    let add_self = run(
        &root,
        &["fields", "add", "self", "work", "--executable", "/bin/x"],
    )?;
    assert!(!add_self.status.success());
    assert_eq!(add_self.status.code(), Some(2));
    assert!(stderr(&add_self).contains("self"));

    // An unregistered stem is rejected the same way.
    let add_unknown = run(
        &root,
        &["fields", "add", "github", "sam", "--executable", "/bin/x"],
    )?;
    assert!(!add_unknown.status.success());
    assert_eq!(add_unknown.status.code(), Some(2));

    // A real, registered stem succeeds and reports its configuration.
    let add = run(
        &root,
        &[
            "fields",
            "add",
            "local",
            "work",
            "--executable",
            "/usr/local/bin/fieldnotes-field-local",
            "--config",
            "path=/tmp/reference",
            "--format",
            "json",
        ],
    )?;
    assert!(add.status.success(), "{}", stderr(&add));
    let add_json = stdout(&add);
    assert!(add_json.contains(r#""schema":"fieldnotes.fields_add.v1""#));
    assert!(add_json.contains(r#""id":"local_work""#));
    assert!(add_json.contains(r#""enabled":true"#));

    // Adding the same ID again is refused rather than silently reconfigured.
    let add_again = run(
        &root,
        &[
            "fields",
            "add",
            "local",
            "work",
            "--executable",
            "/bin/other",
        ],
    )?;
    assert!(!add_again.status.success());
    assert_eq!(add_again.status.code(), Some(2));

    // `list` shows `self` plus the configured Field.
    let list = run(&root, &["fields", "list", "--format", "json"])?;
    assert!(list.status.success(), "{}", stderr(&list));
    let list_json = stdout(&list);
    assert!(list_json.contains(r#""schema":"fieldnotes.fields_list.v1""#));
    assert!(list_json.contains(r#""id":"self""#));
    assert!(list_json.contains(r#""id":"local_work""#));

    // `status` reports no cursor and no manifest yet for a freshly configured
    // Field: `0.1.1`'s configuration phase never runs a Field.
    let status = run(
        &root,
        &["fields", "status", "local_work", "--format", "json"],
    )?;
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json = stdout(&status);
    assert!(status_json.contains(r#""schema":"fieldnotes.fields_status.v1""#));
    assert!(status_json.contains(r#""cursor_present":false"#));
    assert!(status_json.contains(r#""manifest_present":false"#));
    assert!(status_json.contains(r#""last_sync":null"#));

    // A self Note exists before removal...
    let note = run(&root, &["note", "evidence that must survive removal"])?;
    assert!(note.status.success(), "{}", stderr(&note));
    let before = run(&root, &["status", "--format", "json"])?;
    assert!(before.status.success());
    assert!(stdout(&before).contains(r#""total":1"#));

    // ...and remove never touches it.
    let remove = run(
        &root,
        &["fields", "remove", "local_work", "--format", "json"],
    )?;
    assert!(remove.status.success(), "{}", stderr(&remove));
    let remove_json = stdout(&remove);
    assert!(remove_json.contains(r#""schema":"fieldnotes.fields_remove.v1""#));
    assert!(remove_json.contains(r#""notes_preserved":true"#));

    let after = run(&root, &["status", "--format", "json"])?;
    assert!(after.status.success());
    assert!(stdout(&after).contains(r#""total":1"#));

    // The Field is gone from `list` and `status` reports it as unconfigured.
    let list_after = run(&root, &["fields", "list", "--format", "json"])?;
    assert!(!stdout(&list_after).contains("local_work"));
    let status_after = run(&root, &["fields", "status", "local_work"])?;
    assert!(!status_after.status.success());
    assert_eq!(status_after.status.code(), Some(2));

    // Removing an already-removed (or never-configured) Field is reported,
    // not silently accepted.
    let remove_again = run(&root, &["fields", "remove", "local_work"])?;
    assert!(!remove_again.status.success());
    assert_eq!(remove_again.status.code(), Some(2));

    // `self` cannot be removed.
    let remove_self = run(&root, &["fields", "remove", "self"])?;
    assert!(!remove_self.status.success());
    assert_eq!(remove_self.status.code(), Some(2));
    Ok(())
}

#[test]
fn removing_one_field_leaves_another_fields_configuration_and_notes_untouched()
-> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-isolation")?;
    let root = temp.path().join("notebook");
    let init = run(&root, &["init"])?;
    assert!(init.status.success(), "{}", stderr(&init));

    for (field_type, label) in [("local", "work"), ("teams", "acme")] {
        let add = run(
            &root,
            &["fields", "add", field_type, label, "--executable", "/bin/x"],
        )?;
        assert!(add.status.success(), "{}", stderr(&add));
    }
    let note = run(&root, &["note", "unrelated evidence"])?;
    assert!(note.status.success(), "{}", stderr(&note));

    let remove = run(&root, &["fields", "remove", "local_work"])?;
    assert!(remove.status.success(), "{}", stderr(&remove));

    let list = run(&root, &["fields", "list", "--format", "json"])?;
    let list_json = stdout(&list);
    assert!(!list_json.contains("local_work"));
    assert!(list_json.contains("teams_acme"));

    let status = run(&root, &["status", "--format", "json"])?;
    assert!(stdout(&status).contains(r#""total":1"#));
    Ok(())
}

#[test]
fn a_credential_shaped_config_key_is_refused() -> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-credential")?;
    let root = temp.path().join("notebook");
    let init = run(&root, &["init"])?;
    assert!(init.status.success(), "{}", stderr(&init));

    let add = run(
        &root,
        &[
            "fields",
            "add",
            "local",
            "work",
            "--executable",
            "/bin/x",
            "--config",
            "api_key=super-secret",
        ],
    )?;
    assert!(!add.status.success());
    assert!(stderr(&add).contains("credential"));
    // Nothing was written: the Field must not appear in `list`.
    let list = run(&root, &["fields", "list", "--format", "json"])?;
    assert!(!stdout(&list).contains("local_work"));
    Ok(())
}
