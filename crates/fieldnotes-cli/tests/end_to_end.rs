//! End-to-end tests that run the real `fieldnotes` binary.

use std::path::Path;
use std::process::{Command, Output};

use fieldnotes_test_support::TempDir;

/// A minimal PNG signature plus payload.
const IMAGE_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n-pretend-pixels-";

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_fieldnotes")
}

fn run(notebook: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(binary())
        .arg("--notebook")
        .arg(notebook)
        .args(args)
        .output()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn the_binary_initializes_writes_and_validates_a_notebook() -> std::io::Result<()> {
    let temp = TempDir::new("cli")?;
    let root = temp.path().join("notebook");

    let init = run(&root, &["init", "--name", "cli-test", "--format", "json"])?;
    assert!(init.status.success(), "{}", stderr(&init));
    let init_json = stdout(&init);
    assert!(init_json.contains(r#""schema":"fieldnotes.init.v1""#));
    assert!(init_json.contains(r#""created":true"#));
    assert!(init_json.contains(r#""instance_id":"fn_"#));

    // A note body arrives on standard input rather than in argv.
    let mut child = Command::new(binary())
        .arg("--notebook")
        .arg(&root)
        .args([
            "note",
            "--stdin",
            "--title",
            "From stdin",
            "--format",
            "json",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        use std::io::Write;
        input.write_all(b"Typed on standard input.\n")?;
    }
    let note = child.wait_with_output()?;
    assert!(note.status.success(), "{}", stderr(&note));
    assert!(stdout(&note).contains(r#""type":"text""#));

    // A file import, in human form.
    let image = temp.path().join("photo.png");
    std::fs::write(&image, IMAGE_BYTES)?;
    let import = run(
        &root,
        &[
            "note",
            "--file",
            image.to_str().unwrap_or_default(),
            "--title",
            "Imported photo",
        ],
    )?;
    assert!(import.status.success(), "{}", stderr(&import));
    assert!(stdout(&import).contains("Wrote file Note note_"));
    assert!(stdout(&import).contains("(stored)"));

    // Re-importing the same bytes reuses the artifact.
    let again = run(
        &root,
        &[
            "note",
            "--file",
            image.to_str().unwrap_or_default(),
            "--title",
            "Again",
        ],
    )?;
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(stdout(&again).contains("(reused)"));

    let status = run(&root, &["status", "--format", "json"])?;
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json = stdout(&status);
    assert!(status_json.contains(r#""schema":"fieldnotes.status.v1""#));
    assert!(status_json.contains(r#""total":3"#));
    assert!(status_json.contains(r#""invalid":0"#));

    let inspect = run(&root, &["inspect", "--format", "json"])?;
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    assert!(stdout(&inspect).contains(r#""schema":"fieldnotes.inspect.v1""#));
    assert!(stdout(&inspect).contains(r#""ok":true"#));
    Ok(())
}

#[test]
fn failures_are_actionable_and_use_distinct_exit_codes() -> std::io::Result<()> {
    let temp = TempDir::new("cli-errors")?;
    let empty = temp.path().join("not-a-notebook");
    std::fs::create_dir_all(&empty)?;

    // No notebook: exit 4, with a hint, and nothing on standard output.
    let missing = run(&empty, &["status"])?;
    assert_eq!(missing.status.code(), Some(4));
    assert!(stdout(&missing).is_empty());
    assert!(stderr(&missing).contains("run `fieldnotes init`"));

    // The JSON error shape goes to standard error.
    let missing_json = run(&empty, &["status", "--format", "json"])?;
    assert_eq!(missing_json.status.code(), Some(4));
    assert!(stdout(&missing_json).is_empty());
    assert!(stderr(&missing_json).contains(r#""schema":"fieldnotes.error.v1""#));
    assert!(stderr(&missing_json).contains(r#""kind":"not_a_notebook""#));

    // An unusable offset is an input error: exit 2.
    let root = temp.path().join("notebook");
    let init = run(&root, &["init"])?;
    assert!(init.status.success(), "{}", stderr(&init));
    let bad_offset = run(&root, &["--offset", "0200", "note", "text"])?;
    assert_eq!(bad_offset.status.code(), Some(2));
    assert!(stderr(&bad_offset).contains("+HH:MM"));

    // A timezone-less --at value is refused with the contract's reason.
    let bad_at = run(&root, &["note", "--at", "2026-08-22T09:00:00", "text"])?;
    assert_eq!(bad_at.status.code(), Some(2));
    assert!(stderr(&bad_at).contains("explicit numeric UTC offset"));

    // An unhealthy notebook exits 3 while still reporting.
    let damaged = root
        .join("notes")
        .join("20260822T070000Z_self_text_note_01a02844-f150-7000-8000-0000000000ff.md");
    std::fs::write(&damaged, b"---\nid: broken\n")?;
    let inspect = run(&root, &["inspect"])?;
    assert_eq!(inspect.status.code(), Some(3));
    assert!(stdout(&inspect).contains("Notebook has problems"));
    Ok(())
}

#[test]
fn an_offset_can_come_from_the_environment() -> std::io::Result<()> {
    let temp = TempDir::new("cli-offset")?;
    let root = temp.path().join("notebook");
    let output = Command::new(binary())
        .arg("--notebook")
        .arg(&root)
        .args(["init", "--format", "json"])
        .env("FIELDNOTES_UTC_OFFSET", "+02:00")
        .output()?;
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("+02:00"),
        "created_at should carry the configured offset: {}",
        stdout(&output)
    );
    Ok(())
}
