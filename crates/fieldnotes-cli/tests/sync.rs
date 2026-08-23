//! End-to-end tests for `fieldnotes sync`'s command surface, run through the
//! real binary.
//!
//! What a real Field process actually collects is proved where the Field
//! binaries live (`fields/fieldnotes-field-local/tests/sync_local.rs` and
//! `fields/fieldnotes-field-fixture/tests/sync_durability.rs`), because
//! `CARGO_BIN_EXE_<name>` resolves only inside the package that owns the
//! binary. What is proved here is the CLI contract: the stable schema-tagged
//! JSON shape, the exit code, and the retention settings' precedence.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fieldnotes_test_support::TempDir;

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

/// An absolute path that certainly holds no executable, so the run fails at the
/// one place a pinned-path contract can fail: starting the process.
fn missing_executable(temp: &TempDir) -> PathBuf {
    temp.path().join("no-such-field-binary")
}

#[test]
fn a_sync_with_no_configured_field_reports_an_empty_result() -> std::io::Result<()> {
    let temp = TempDir::new("cli-sync-empty")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());

    let human = run(&root, &["sync"])?;
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("no enabled Field is configured"));

    let json = run(&root, &["--format", "json", "sync"])?;
    assert!(json.status.success());
    let text = stdout(&json);
    assert!(text.contains(r#""schema":"fieldnotes.sync.v1""#));
    assert!(text.contains(r#""ok":true"#));
    assert!(text.contains(r#""fields":[]"#));
    Ok(())
}

#[test]
fn a_field_that_cannot_start_is_reported_per_field_and_exits_three() -> std::io::Result<()> {
    let temp = TempDir::new("cli-sync-unstartable")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());
    let executable = missing_executable(&temp);
    let add = run(
        &root,
        &[
            "fields",
            "add",
            "local",
            "work",
            "--executable",
            &executable.display().to_string(),
        ],
    )?;
    assert!(add.status.success(), "{}", stderr(&add));

    let json = run(&root, &["--format", "json", "sync"])?;
    // Durable work committed before a failure would stand; here there is none,
    // and the run is reported rather than abandoning the invocation.
    assert_eq!(json.status.code(), Some(3));
    let text = stdout(&json);
    assert!(text.contains(r#""schema":"fieldnotes.sync.v1""#));
    assert!(text.contains(r#""ok":false"#));
    assert!(text.contains(r#""id":"local_work""#));
    assert!(text.contains(r#""outcome":"failed""#));
    assert!(text.contains(r#""cursor_committed":false"#));
    assert!(text.contains("cannot start Field"), "{text}");

    // The human form names the Field and the reason.
    let human = run(&root, &["sync", "local_work"])?;
    assert_eq!(human.status.code(), Some(3));
    let rendered = stdout(&human);
    assert!(
        rendered.contains("local_work (incremental) failed"),
        "{rendered}"
    );
    Ok(())
}

/// The release gate's "revoked and expired credentials fail actionably"
/// requirement, at the command level: a Field configured to authenticate, with
/// no credential stored, fails **early and cleanly**.
///
/// "Early" is observable rather than asserted by inspection: the Field's
/// executable path holds no executable, so a run that got as far as starting it
/// would say `cannot start Field`. This run must not, because the credential is
/// resolved first.
#[test]
fn a_field_needing_credentials_with_none_stored_fails_early_and_cleanly() -> std::io::Result<()> {
    let temp = TempDir::new("cli-sync-credential")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());
    let executable = missing_executable(&temp);
    let add = run(
        &root,
        &[
            "fields",
            "add",
            "outlook_mail",
            "work",
            "--executable",
            &executable.display().to_string(),
            "--config",
            "credential_profile=fieldnotes_cli_test_absent_profile",
            // The explicit environment provider, reading a variable that is
            // deliberately never set: no keychain is touched by this test.
            "--config",
            "credential_provider=environment",
            "--config",
            "credential_env_var=FIELDNOTES_CLI_TEST_DELIBERATELY_UNSET_7b2e",
        ],
    )?;
    assert!(add.status.success(), "{}", stderr(&add));

    let json = run(&root, &["--format", "json", "sync", "outlook_mail_work"])?;
    assert_eq!(json.status.code(), Some(3));
    let text = stdout(&json);
    assert!(text.contains(r#""outcome":"failed""#), "{text}");
    assert!(text.contains(r#""cursor_committed":false"#), "{text}");
    assert!(text.contains(r#""credential":null"#), "{text}");
    assert!(
        text.contains("fieldnotes fields auth outlook_mail_work"),
        "the failure must say what to run: {text}"
    );
    assert!(
        !text.contains("cannot start Field"),
        "the run must refuse before the spawn: {text}"
    );

    // No staging directory and no cursor were created for this Field.
    let sync_state = root.join(".fieldnotes").join("state").join("sync");
    assert!(
        !sync_state.join("staging").exists(),
        "a credential failure must create no staging directory"
    );
    assert!(
        !sync_state.join("outlook_mail_work.cursor").exists(),
        "a credential failure must commit no cursor"
    );

    let human = run(&root, &["sync", "outlook_mail_work"])?;
    assert_eq!(human.status.code(), Some(3));
    let rendered = stdout(&human);
    assert!(
        rendered.contains("outlook_mail_work (incremental) failed"),
        "{rendered}"
    );
    assert!(
        rendered.contains("cursor             not advanced"),
        "{rendered}"
    );
    Ok(())
}

#[test]
fn syncing_an_unconfigured_field_is_a_usage_error() -> std::io::Result<()> {
    let temp = TempDir::new("cli-sync-unknown")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());

    let unknown = run(&root, &["sync", "local_ghost"])?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("local_ghost"));

    // `self` has no process and cannot be synced.
    let built_in = run(&root, &["sync", "self"])?;
    assert_eq!(built_in.status.code(), Some(2));
    assert!(stderr(&built_in).contains("built-in Field"));
    Ok(())
}

#[test]
fn a_snapshot_scope_requires_snapshot_mode() -> std::io::Result<()> {
    let temp = TempDir::new("cli-sync-scope")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());

    // `--scope` without `--snapshot` is a usage error clap refuses outright,
    // because a scope means nothing to an incremental run.
    let output = run(&root, &["sync", "--scope", "local-root:one"])?;
    assert!(!output.status.success());
    Ok(())
}

#[test]
fn the_retention_settings_are_recorded_validated_and_reported() -> std::io::Result<()> {
    let temp = TempDir::new("cli-sync-retention")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());

    // Unset, both settings report the approved defaults rather than a value.
    let unset = run(&root, &["config", "show"])?;
    assert!(unset.status.success(), "{}", stderr(&unset));
    assert!(stdout(&unset).contains("26214400-byte default"));
    assert!(stdout(&unset).contains("default include set"));

    // Configuring up toward the frozen ceiling is legal.
    let raised = run(&root, &["config", "set", "artifact-max-bytes", "104857600"])?;
    assert!(raised.status.success(), "{}", stderr(&raised));
    let get = run(&root, &["config", "get", "artifact-max-bytes"])?;
    assert_eq!(stdout(&get).trim(), "104857600");

    // Crossing it is not: only a protocol revision may raise the ceiling.
    let over = run(
        &root,
        &["config", "set", "artifact-max-bytes", "1073741824"],
    )?;
    assert_eq!(over.status.code(), Some(2));
    assert!(stderr(&over).contains("ceiling"), "{}", stderr(&over));

    // A media-type include set is validated entry by entry.
    let good = run(
        &root,
        &[
            "config",
            "set",
            "artifact-media-types",
            "application/pdf, image/*",
        ],
    )?;
    assert!(good.status.success(), "{}", stderr(&good));
    let listed = run(&root, &["config", "get", "artifact-media-types"])?;
    assert!(stdout(&listed).contains("application/pdf,image/*"));

    let bad = run(&root, &["config", "set", "artifact-media-types", "pdf"])?;
    assert_eq!(bad.status.code(), Some(2));
    assert!(stderr(&bad).contains("media type"), "{}", stderr(&bad));

    // And both appear in the stable JSON form.
    let json = run(&root, &["--format", "json", "config", "show"])?;
    let text = stdout(&json);
    assert!(text.contains(r#""artifact_max_bytes":104857600"#), "{text}");
    assert!(
        text.contains(r#""artifact_media_types":"application/pdf,image/*""#),
        "{text}"
    );
    Ok(())
}

#[test]
fn fields_status_reports_no_cursor_before_any_sync() -> std::io::Result<()> {
    let temp = TempDir::new("cli-sync-status")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());
    let executable = missing_executable(&temp);
    assert!(
        run(
            &root,
            &[
                "fields",
                "add",
                "local",
                "work",
                "--executable",
                &executable.display().to_string(),
            ],
        )?
        .status
        .success()
    );

    let json = run(&root, &["--format", "json", "fields", "status"])?;
    assert!(json.status.success(), "{}", stderr(&json));
    let text = stdout(&json);
    assert!(text.contains(r#""cursor_present":false"#));
    assert!(text.contains(r#""cursor_format_version":null"#));
    assert!(text.contains(r#""cursor_coverage":null"#));
    assert!(text.contains(r#""manifest_cursor_format_version":null"#));
    Ok(())
}
