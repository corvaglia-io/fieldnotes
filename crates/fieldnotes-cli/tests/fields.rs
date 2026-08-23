//! End-to-end tests for `fieldnotes fields add/auth/list/status/remove`, run
//! through the real binary.
//!
//! Nothing here authenticates against anything. `fields auth` is exercised up
//! to, and never past, the point where it would contact a tenant: its command
//! surface, its refusal in a non-interactive invocation, and its refusals for a
//! Field that is unconfigured or misconfigured. What happens after that point is
//! covered in `fieldnotes-app`'s own tests, against injected collaborators.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fieldnotes_test_support::TempDir;

/// The environment variable overriding the profile file location, set on
/// every invocation so a test run never touches a developer's real profile.
const CONFIG_ENV: &str = "FIELDNOTES_CONFIG";

/// Set on every `fields auth` invocation in this file, so no test can open a
/// browser on the machine running it.
const NON_INTERACTIVE_ENV: &str = "FIELDNOTES_NON_INTERACTIVE";

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

/// Runs a command with the non-interactive marker set, so `fields auth` refuses
/// instead of opening a browser.
fn run_non_interactive(notebook: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(binary())
        .arg("--notebook")
        .arg(notebook)
        .args(args)
        .env(CONFIG_ENV, hermetic_config_path(notebook))
        .env(NON_INTERACTIVE_ENV, "1")
        .output()
}

#[test]
fn fields_auth_has_a_documented_surface_and_never_takes_a_secret() -> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-auth-help")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());

    let help = run(&root, &["fields", "auth", "--help"])?;
    assert!(help.status.success(), "{}", stderr(&help));
    let text = stdout(&help);
    assert!(text.contains("--no-browser"), "{text}");
    assert!(text.contains("credential_profile"), "{text}");
    // The two promises the command's documentation has to make.
    assert!(text.contains("short-lived access token"), "{text}");
    assert!(text.contains("protected channel"), "{text}");
    // No option anywhere takes credential material.
    for forbidden in ["--token", "--password", "--secret", "--client-secret"] {
        assert!(
            !text.contains(forbidden),
            "`fields auth` must not accept {forbidden}"
        );
    }
    Ok(())
}

#[test]
fn fields_auth_refuses_an_unconfigured_or_misconfigured_field() -> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-auth-refusals")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());

    // No such Field.
    let unknown = run_non_interactive(&root, &["fields", "auth", "outlook_mail_ghost"])?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("outlook_mail_ghost"));

    // `self` has nothing to authenticate.
    let built_in = run_non_interactive(&root, &["fields", "auth", "self"])?;
    assert_eq!(built_in.status.code(), Some(2));

    // Configured, but naming no credential profile: the message says which
    // key to set, and nothing was spawned to find that out.
    let add = run(
        &root,
        &[
            "fields",
            "add",
            "outlook_mail",
            "work",
            "--executable",
            "/usr/local/bin/fieldnotes-field-outlook-mail",
        ],
    )?;
    assert!(add.status.success(), "{}", stderr(&add));
    let unconfigured = run_non_interactive(&root, &["fields", "auth", "outlook_mail_work"])?;
    assert_eq!(unconfigured.status.code(), Some(2));
    assert!(
        stderr(&unconfigured).contains("credential_profile"),
        "{}",
        stderr(&unconfigured)
    );
    Ok(())
}

#[test]
fn a_non_interactive_invocation_is_told_to_run_interactively() -> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-auth-headless")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());
    let add = run(
        &root,
        &[
            "fields",
            "add",
            "outlook_mail",
            "work",
            "--executable",
            "/usr/local/bin/fieldnotes-field-outlook-mail",
            "--config",
            "credential_profile=work",
        ],
    )?;
    assert!(add.status.success(), "{}", stderr(&add));

    let refused = run_non_interactive(&root, &["fields", "auth", "outlook_mail_work"])?;
    assert_eq!(refused.status.code(), Some(2));
    let message = stderr(&refused);
    assert!(message.contains("non-interactive"), "{message}");
    assert!(
        message.contains("fieldnotes fields auth outlook_mail_work"),
        "{message}"
    );

    // The JSON form of the same refusal, on standard error, leaving standard
    // output empty.
    let json = Command::new(binary())
        .arg("--notebook")
        .arg(&root)
        .args(["--format", "json", "fields", "auth", "outlook_mail_work"])
        .env(CONFIG_ENV, hermetic_config_path(&root))
        .env(NON_INTERACTIVE_ENV, "1")
        .output()?;
    assert_eq!(json.status.code(), Some(2));
    assert!(stdout(&json).is_empty());
    let error = stderr(&json);
    assert!(
        error.contains(r#""schema":"fieldnotes.error.v1""#),
        "{error}"
    );
    assert!(
        error.contains(r#""kind":"credential_not_interactive""#),
        "{error}"
    );
    Ok(())
}

#[test]
fn fields_status_reports_credential_state_without_attempting_a_sync() -> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-auth-status")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());

    // A Field that needs no credential says so.
    let add_local = run(
        &root,
        &["fields", "add", "local", "work", "--executable", "/bin/x"],
    )?;
    assert!(add_local.status.success(), "{}", stderr(&add_local));
    let local = run(
        &root,
        &["fields", "status", "local_work", "--format", "json"],
    )?;
    assert!(local.status.success(), "{}", stderr(&local));
    let local_json = stdout(&local);
    assert!(
        local_json.contains(r#""credential_state":"not_required""#),
        "{local_json}"
    );
    assert!(
        local_json.contains(r#""credential_profile":null"#),
        "{local_json}"
    );

    // A Field configured with a profile reports the profile and the provider,
    // and its state comes from reading the credential store — not from a sync.
    let add_mail = run(
        &root,
        &[
            "fields",
            "add",
            "outlook_mail",
            "work",
            "--executable",
            "/usr/local/bin/fieldnotes-field-outlook-mail",
            "--config",
            "credential_profile=fieldnotes_cli_test_absent_profile",
            "--config",
            "credential_provider=environment",
            "--config",
            "credential_env_var=FIELDNOTES_CLI_TEST_DELIBERATELY_UNSET_4c1a9f",
        ],
    )?;
    assert!(add_mail.status.success(), "{}", stderr(&add_mail));
    let mail = run(
        &root,
        &["fields", "status", "outlook_mail_work", "--format", "json"],
    )?;
    assert!(mail.status.success(), "{}", stderr(&mail));
    let mail_json = stdout(&mail);
    assert!(
        mail_json.contains(r#""credential_profile":"fieldnotes_cli_test_absent_profile""#),
        "{mail_json}"
    );
    assert!(
        mail_json.contains(r#""credential_provider":"environment""#),
        "{mail_json}"
    );
    // The environment provider reads one deliberately unset variable, so the
    // answer is "absent" rather than "stored", with no keychain involved.
    assert!(
        mail_json.contains(r#""credential_state":"absent""#),
        "{mail_json}"
    );

    // Human output names the command that fixes it.
    let human = run(&root, &["fields", "status", "outlook_mail_work"])?;
    let text = stdout(&human);
    assert!(
        text.contains("fieldnotes fields auth outlook_mail_work"),
        "{text}"
    );
    Ok(())
}

/// Rewrites one Field's configuration file with a recorded credential account,
/// which is what a real `fields auth` writes after storing the refresh token.
///
/// Done by editing the file directly because reaching that write for real needs
/// a tenant and a browser, which these tests deliberately never touch. The write
/// itself is covered against the real code path in `fieldnotes-app`'s tests.
fn record_account(notebook: &Path, field_id: &str, account: Option<&str>) -> std::io::Result<()> {
    let path = notebook
        .join(".fieldnotes")
        .join("fields")
        .join(format!("{field_id}.json"));
    let text = std::fs::read_to_string(&path)?;
    let rewritten = match account {
        None => text,
        Some(account) => text.replacen(
            '{',
            &format!("{{\n  \"credential_account\": \"{account}\","),
            1,
        ),
    };
    std::fs::write(&path, rewritten)
}

/// Configures one Outlook Field whose credential lives in a deliberately unset
/// environment variable, so no keychain is ever consulted.
fn add_authenticating_field(
    notebook: &Path,
    stem: &str,
    label: &str,
    profile: &str,
) -> std::io::Result<Output> {
    run(
        notebook,
        &[
            "fields",
            "add",
            stem,
            label,
            "--executable",
            "/nonexistent/field-binary",
            "--config",
            &format!("credential_profile={profile}"),
            "--config",
            "credential_provider=environment",
            "--config",
            "credential_env_var=FIELDNOTES_CLI_TEST_DELIBERATELY_UNSET_8b3e1c",
        ],
    )
}

#[test]
fn fields_status_reports_which_account_a_credential_authenticates_as() -> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-account")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());
    let added = add_authenticating_field(&root, "outlook_mail", "work", "work")?;
    assert!(added.status.success(), "{}", stderr(&added));

    // Nothing recorded yet — the state a credential stored before Fieldnotes
    // learned to record accounts is in. Unknown, with the command that fixes it.
    let unknown = run(&root, &["fields", "status", "outlook_mail_work"])?;
    assert!(unknown.status.success(), "{}", stderr(&unknown));
    let text = stdout(&unknown);
    assert!(
        text.contains("account            unknown; run `fieldnotes fields auth outlook_mail_work`"),
        "{text}"
    );
    let unknown_json = run(
        &root,
        &["fields", "status", "outlook_mail_work", "--format", "json"],
    )?;
    assert!(
        stdout(&unknown_json).contains(r#""credential_account":null"#),
        "{}",
        stdout(&unknown_json)
    );
    assert!(
        stdout(&unknown_json).contains(r#""credential_account_mismatch":null"#),
        "{}",
        stdout(&unknown_json)
    );

    // Recorded: reported before any sync has run, in both forms.
    record_account(
        &root,
        "outlook_mail_work",
        Some("mailbox.owner@example.test"),
    )?;
    let known = run(&root, &["fields", "status", "outlook_mail_work"])?;
    assert!(known.status.success(), "{}", stderr(&known));
    assert!(
        stdout(&known).contains("account            mailbox.owner@example.test"),
        "{}",
        stdout(&known)
    );
    let known_json = run(
        &root,
        &["fields", "status", "outlook_mail_work", "--format", "json"],
    )?;
    assert!(
        stdout(&known_json).contains(r#""credential_account":"mailbox.owner@example.test""#),
        "{}",
        stdout(&known_json)
    );

    // A second Field signed in as the same person: still no warning.
    let contacts = add_authenticating_field(&root, "outlook_contacts", "work", "work")?;
    assert!(contacts.status.success(), "{}", stderr(&contacts));
    record_account(
        &root,
        "outlook_contacts_work",
        Some("mailbox.owner@example.test"),
    )?;
    let agreeing = run(&root, &["fields", "status"])?;
    assert!(
        !stdout(&agreeing).contains("WARNING"),
        "{}",
        stdout(&agreeing)
    );

    Ok(())
}

#[test]
fn fields_status_warns_prominently_when_fields_are_signed_in_as_different_accounts()
-> std::io::Result<()> {
    let temp = TempDir::new("cli-fields-account-mismatch")?;
    let root = temp.path().join("notebook");
    assert!(run(&root, &["init"])?.status.success());
    for (stem, label) in [("outlook_mail", "work"), ("outlook_contacts", "work")] {
        let added = add_authenticating_field(&root, stem, label, "work")?;
        assert!(added.status.success(), "{}", stderr(&added));
    }
    record_account(
        &root,
        "outlook_mail_work",
        Some("mailbox.owner@example.test"),
    )?;
    record_account(
        &root,
        "outlook_contacts_work",
        Some("tenant.admin@example.test"),
    )?;

    let human = run(&root, &["fields", "status"])?;
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(text.contains("WARNING"), "{text}");
    assert!(text.contains("different accounts"), "{text}");
    // Both accounts, and the Field each belongs to.
    assert!(text.contains("mailbox.owner@example.test"), "{text}");
    assert!(text.contains("tenant.admin@example.test"), "{text}");
    assert!(text.contains("outlook_mail_work"), "{text}");
    assert!(text.contains("outlook_contacts_work"), "{text}");
    assert!(text.contains("fieldnotes fields auth"), "{text}");
    // A warning, not a refusal.
    assert_eq!(human.status.code(), Some(0));

    // Naming one Field still reports the notebook's disagreement, because that
    // is true of the notebook whichever Field was asked about.
    let single = run(&root, &["fields", "status", "outlook_mail_work"])?;
    assert!(stdout(&single).contains("WARNING"), "{}", stdout(&single));

    let json = run(&root, &["fields", "status", "--format", "json"])?;
    let json_text = stdout(&json);
    assert!(
        json_text.contains(
            r#""credential_account_mismatch":{"accounts":[{"account":"mailbox.owner@example.test","field_ids":["outlook_mail_work"]},{"account":"tenant.admin@example.test","field_ids":["outlook_contacts_work"]}]"#
        ),
        "{json_text}"
    );
    // Still exactly one JSON object on standard output.
    assert_eq!(json_text.trim_end().lines().count(), 1, "{json_text}");
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
