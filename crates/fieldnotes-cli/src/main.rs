//! Thin command-line composition root for Fieldnotes.
//!
//! The binary parses arguments, injects the real clock and random source,
//! calls one application use case, and renders the result. It contains no
//! notebook rules of its own and never formats notebook bytes.
//!
//! # Output
//!
//! Human-readable text is the default. `--format json` prints one compact
//! object per invocation on standard output, each carrying a `schema` field so
//! automation can pin a shape:
//!
//! - `fieldnotes.init.v1`
//! - `fieldnotes.note.v1`
//! - `fieldnotes.status.v1`
//! - `fieldnotes.inspect.v1`
//! - `fieldnotes.error.v1`
//!
//! Failures print `fieldnotes.error.v1` on standard **error**, leaving standard
//! output empty, so a JSON consumer never has to distinguish a result from a
//! diagnostic.
//!
//! # Exit codes
//!
//! | Code | Meaning |
//! |---|---|
//! | 0 | success |
//! | 2 | usage or input error |
//! | 3 | notebook contract violation (including an unhealthy `inspect`) |
//! | 4 | notebook state error, such as no notebook found |
//! | 5 | filesystem failure |
//! | 70 | internal error |
//!
//! # Secret hygiene
//!
//! Nothing echoes or logs the process arguments. Note bodies are preferably
//! supplied through `--stdin` or `--body-file`, because a positional argument
//! can be captured by shell history; only the Note's own declared content is
//! ever persisted.

mod environment;
mod json;

use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use fieldnotes_app::{
    AppError, InitOutcome, InspectReport, Kernel, NoteOutcome, NoteRequest, NoteSource,
    StatusReport, create_note, init, inspect, status,
};
use fieldnotes_domain::Datetime;
use fieldnotes_store::{InitState, Notebook};

use crate::environment::{OsRandom, SystemClock, resolve_offset};
use crate::json::Json;

/// Success.
const EXIT_OK: i32 = 0;
/// Usage or input error.
const EXIT_USAGE: i32 = 2;
/// The notebook contract was violated.
const EXIT_CONTRACT: i32 = 3;
/// The notebook is missing or unusable.
const EXIT_NOTEBOOK: i32 = 4;
/// A filesystem operation failed.
const EXIT_IO: i32 = 5;
/// An internal error, including a panic.
const EXIT_INTERNAL: i32 = 70;

/// Create and inspect a portable Fieldnotes notebook.
#[derive(Debug, Parser)]
#[command(name = "fieldnotes", version, about, long_about = None)]
struct Cli {
    /// Notebook to operate on. Defaults to the notebook containing the working
    /// directory.
    #[arg(long, global = true, value_name = "PATH")]
    notebook: Option<PathBuf>,

    /// Output form.
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    format: Format,

    /// UTC offset for generated datetimes, as +HH:MM, -HH:MM, or utc.
    /// Defaults to the FIELDNOTES_UTC_OFFSET environment variable, then utc.
    #[arg(long, global = true, value_name = "OFFSET")]
    offset: Option<String>,

    #[command(subcommand)]
    command: Command,
}

/// The output form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    /// Human-readable text.
    Human,
    /// One compact, versioned JSON object.
    Json,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a notebook and its instance identity.
    Init {
        /// Directory to initialize. Defaults to --notebook, then the working
        /// directory.
        path: Option<PathBuf>,
        /// Optional display-only notebook name.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
    /// Create a `self` Note.
    Note {
        /// Note text. Prefer --stdin or --body-file: a positional argument can
        /// be captured by shell history.
        #[arg(value_name = "TEXT", conflicts_with_all = ["stdin", "body_file"])]
        text: Option<String>,
        /// Read the Note text from standard input.
        #[arg(long, conflicts_with = "body_file")]
        stdin: bool,
        /// Read the Note text from a file.
        #[arg(long, value_name = "PATH")]
        body_file: Option<PathBuf>,
        /// Note title, also used as the body's first heading.
        #[arg(long, value_name = "TITLE")]
        title: Option<String>,
        /// Event time as RFC 3339 with an explicit numeric offset.
        #[arg(long, value_name = "DATETIME")]
        at: Option<String>,
        /// Import a file as a content-addressed original artifact.
        #[arg(long, value_name = "PATH", conflicts_with = "voice")]
        file: Option<PathBuf>,
        /// Import an audio recording as a content-addressed original artifact.
        #[arg(long, value_name = "PATH")]
        voice: Option<PathBuf>,
    },
    /// Summarize the notebook.
    Status,
    /// Validate the notebook, or render one record by ID, filename, or path.
    Inspect {
        /// Record ID, filename, or path. Omit to validate every file.
        #[arg(value_name = "RECORD")]
        target: Option<String>,
    },
}

/// A failure, ready to render and exit with.
#[derive(Debug)]
struct Failure {
    kind: String,
    message: String,
    code: i32,
}

impl Failure {
    fn new(kind: &str, message: impl Into<String>, code: i32) -> Self {
        Failure {
            kind: kind.to_owned(),
            message: message.into(),
            code,
        }
    }
}

impl From<AppError> for Failure {
    fn from(error: AppError) -> Self {
        let code = match error.kind() {
            "io" => EXIT_IO,
            "not_a_notebook" | "not_a_directory" | "unexpected_tree" => EXIT_NOTEBOOK,
            "invalid_record" | "invalid_file" | "artifact_corrupt" => EXIT_CONTRACT,
            "empty_note" | "not_audio" | "invalid_offset" | "unknown_target" => EXIT_USAGE,
            _ => EXIT_INTERNAL,
        };
        Failure::new(error.kind(), error.to_string(), code)
    }
}

fn main() {
    install_panic_hook();
    let cli = Cli::parse();
    let format = cli.format;
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(failure) => {
            report_failure(&failure, format);
            std::process::exit(failure.code);
        }
    }
}

/// Replaces the default panic output with one short line.
///
/// A user must never see a panic message or a backtrace: it is noise at best
/// and could echo file content at worst.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map_or_else(|| "unknown location".to_owned(), ToString::to_string);
        let mut stderr = std::io::stderr();
        let _ = writeln!(
            stderr,
            "fieldnotes: internal error at {location}; no further changes were made"
        );
        std::process::exit(EXIT_INTERNAL);
    }));
}

fn report_failure(failure: &Failure, format: Format) {
    let mut stderr = std::io::stderr();
    let rendered = match format {
        Format::Human => format!("error: {}\n", failure.message),
        Format::Json => format!(
            "{}\n",
            Json::Obj(vec![
                ("schema", Json::text("fieldnotes.error.v1")),
                ("ok", Json::Bool(false)),
                ("kind", Json::text(failure.kind.clone())),
                ("message", Json::text(failure.message.clone())),
            ])
            .render()
        ),
    };
    let _ = stderr.write_all(rendered.as_bytes());
}

fn print(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

fn run(cli: Cli) -> Result<i32, Failure> {
    let offset = resolve_offset(cli.offset.as_deref())
        .map_err(|message| Failure::new("invalid_offset", message, EXIT_USAGE))?;
    match cli.command {
        Command::Init { path, name } => {
            let root = path
                .or_else(|| cli.notebook.clone())
                .unwrap_or_else(|| PathBuf::from("."));
            let mut kernel = Kernel::new(SystemClock, OsRandom, offset)?;
            let outcome = init(&mut kernel, &root, name.as_deref())?;
            print(&render_init(&outcome, cli.format));
            Ok(EXIT_OK)
        }
        Command::Note {
            text,
            stdin,
            body_file,
            title,
            at,
            file,
            voice,
        } => {
            let notebook = open_notebook(cli.notebook.as_deref())?;
            let source = match (file, voice) {
                (Some(path), _) => NoteSource::File(path),
                (None, Some(path)) => NoteSource::Voice(path),
                (None, None) => NoteSource::Text,
            };
            let text = read_note_text(text, stdin, body_file.as_deref())?;
            let occurred_at = match at {
                Some(value) => Some(Datetime::parse(&value).map_err(|error| {
                    Failure::new(
                        "datetime",
                        format!("--at `{value}` is invalid: {error}"),
                        EXIT_USAGE,
                    )
                })?),
                None => None,
            };
            let request = NoteRequest {
                source,
                text,
                title,
                occurred_at,
            };
            let mut kernel = Kernel::new(SystemClock, OsRandom, offset)?;
            let outcome = create_note(&mut kernel, &notebook, &request)?;
            print(&render_note(&outcome, cli.format));
            Ok(EXIT_OK)
        }
        Command::Status => {
            let notebook = open_notebook(cli.notebook.as_deref())?;
            let report = status(&notebook)?;
            print(&render_status(&report, cli.format));
            Ok(EXIT_OK)
        }
        Command::Inspect { target } => {
            let notebook = open_notebook(cli.notebook.as_deref())?;
            let report = inspect(&notebook, target.as_deref())?;
            print(&render_inspect(&report, cli.format));
            Ok(if report.healthy {
                EXIT_OK
            } else {
                EXIT_CONTRACT
            })
        }
    }
}

/// Locates the notebook to operate on.
fn open_notebook(explicit: Option<&std::path::Path>) -> Result<Notebook, Failure> {
    let start = match explicit {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().map_err(|error| {
            Failure::new(
                "io",
                format!("could not read the working directory: {error}"),
                EXIT_IO,
            )
        })?,
    };
    Notebook::discover(&start).map_err(|error| Failure::from(AppError::Store(error)))
}

/// Collects the Note text from the chosen input.
fn read_note_text(
    positional: Option<String>,
    stdin: bool,
    body_file: Option<&std::path::Path>,
) -> Result<Option<String>, Failure> {
    if let Some(text) = positional {
        return Ok(Some(text));
    }
    if let Some(path) = body_file {
        let text = std::fs::read_to_string(path).map_err(|error| {
            Failure::new(
                "io",
                format!("could not read `{}`: {error}", path.display()),
                EXIT_IO,
            )
        })?;
        return Ok(Some(text));
    }
    if stdin {
        let text = std::io::read_to_string(std::io::stdin()).map_err(|error| {
            Failure::new(
                "io",
                format!("could not read standard input: {error}"),
                EXIT_IO,
            )
        })?;
        return Ok(Some(text));
    }
    Ok(None)
}

fn render_init(outcome: &InitOutcome, format: Format) -> String {
    let created = outcome.state == InitState::Created;
    match format {
        Format::Human => {
            let headline = if created {
                "Initialized notebook"
            } else {
                "Notebook already initialized"
            };
            let mut out = format!("{headline} at {}\n", outcome.root.display());
            out.push_str(&format!("  instance  {}\n", outcome.instance.instance_id));
            out.push_str(&format!("  created   {}\n", outcome.instance.created_at));
            if let Some(name) = &outcome.instance.name {
                out.push_str(&format!("  name      {name}\n"));
            }
            out
        }
        Format::Json => {
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.init.v1")),
                ("ok", Json::Bool(true)),
                ("created", Json::Bool(created)),
                ("root", Json::text(outcome.root.display().to_string())),
                (
                    "instance_id",
                    Json::text(outcome.instance.instance_id.to_string()),
                ),
                (
                    "created_at",
                    Json::text(outcome.instance.created_at.to_string()),
                ),
                ("name", Json::maybe_text(outcome.instance.name.clone())),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn render_note(outcome: &NoteOutcome, format: Format) -> String {
    match format {
        Format::Human => {
            let mut out = format!("Wrote {} Note {}\n", outcome.note_type, outcome.note_id);
            out.push_str(&format!("  file       {}\n", outcome.relative_path));
            out.push_str(&format!("  occurred   {}\n", outcome.occurred_at));
            out.push_str(&format!("  captured   {}\n", outcome.captured_at));
            if let Some(artifact) = &outcome.artifact {
                let state = if artifact.reused { "reused" } else { "stored" };
                out.push_str(&format!(
                    "  artifact   {} ({state})\n",
                    artifact.relative_path
                ));
                out.push_str(&format!("             {}\n", artifact.id));
            }
            out
        }
        Format::Json => {
            let artifact = match &outcome.artifact {
                Some(artifact) => Json::Obj(vec![
                    ("id", Json::text(artifact.id.to_string())),
                    ("path", Json::text(artifact.relative_path.clone())),
                    ("reused", Json::Bool(artifact.reused)),
                ]),
                None => Json::Null,
            };
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.note.v1")),
                ("ok", Json::Bool(true)),
                ("note_id", Json::text(outcome.note_id.to_string())),
                ("type", Json::text(outcome.note_type.to_string())),
                ("path", Json::text(outcome.relative_path.clone())),
                ("occurred_at", Json::text(outcome.occurred_at.to_string())),
                ("captured_at", Json::text(outcome.captured_at.to_string())),
                ("content_hash", Json::text(outcome.content_hash.clone())),
                ("artifact", artifact),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn render_status(report: &StatusReport, format: Format) -> String {
    match format {
        Format::Human => {
            let mut out = format!("Notebook {}\n", report.root.display());
            out.push_str(&format!("  instance   {}\n", report.instance.instance_id));
            out.push_str(&format!("  created    {}\n", report.instance.created_at));
            if let Some(name) = &report.instance.name {
                out.push_str(&format!("  name       {name}\n"));
            }
            out.push_str(&format!("  fields     {}\n", report.fields.join(", ")));
            out.push_str(&format!(
                "  notes      {} ({} valid, {} with problems)\n",
                report.notes_total, report.notes_valid, report.notes_invalid
            ));
            for (note_type, count) in &report.notes_by_type {
                out.push_str(&format!("               {note_type}: {count}\n"));
            }
            out.push_str(&format!(
                "  artifacts  {} ({} bytes, {} unreferenced)\n",
                report.artifacts_total, report.artifact_bytes, report.artifacts_unreferenced
            ));
            if let Some((first, last)) = &report.occurred_range {
                out.push_str(&format!("  occurred   {first} .. {last}\n"));
            }
            if report.missing_artifact_references > 0 {
                out.push_str(&format!(
                    "  warning    {} artifact reference(s) have no stored bytes\n",
                    report.missing_artifact_references
                ));
            }
            if report.interrupted_writes > 0 {
                out.push_str(&format!(
                    "  warning    {} interrupted write(s) left staging files; run `fieldnotes inspect`\n",
                    report.interrupted_writes
                ));
            }
            out
        }
        Format::Json => {
            let by_type: Vec<Json> = report
                .notes_by_type
                .iter()
                .map(|(note_type, count)| {
                    Json::Obj(vec![
                        ("type", Json::text(note_type.clone())),
                        ("count", Json::count(*count)),
                    ])
                })
                .collect();
            let (first, last) = match &report.occurred_range {
                Some((first, last)) => (Json::text(first.clone()), Json::text(last.clone())),
                None => (Json::Null, Json::Null),
            };
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.status.v1")),
                ("ok", Json::Bool(true)),
                ("root", Json::text(report.root.display().to_string())),
                (
                    "instance_id",
                    Json::text(report.instance.instance_id.to_string()),
                ),
                (
                    "created_at",
                    Json::text(report.instance.created_at.to_string()),
                ),
                ("name", Json::maybe_text(report.instance.name.clone())),
                (
                    "fields",
                    Json::Arr(report.fields.iter().cloned().map(Json::Str).collect()),
                ),
                (
                    "notes",
                    Json::Obj(vec![
                        ("total", Json::count(report.notes_total)),
                        ("valid", Json::count(report.notes_valid)),
                        ("invalid", Json::count(report.notes_invalid)),
                        ("by_type", Json::Arr(by_type)),
                        ("earliest_occurred_at", first),
                        ("latest_occurred_at", last),
                    ]),
                ),
                (
                    "artifacts",
                    Json::Obj(vec![
                        ("total", Json::count(report.artifacts_total)),
                        ("bytes", Json::Int(report.artifact_bytes)),
                        ("unreferenced", Json::count(report.artifacts_unreferenced)),
                        (
                            "missing_references",
                            Json::count(report.missing_artifact_references),
                        ),
                    ]),
                ),
                ("interrupted_writes", Json::count(report.interrupted_writes)),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn render_inspect(report: &InspectReport, format: Format) -> String {
    match format {
        Format::Human => {
            let mut out = format!("Notebook {}\n", report.root.display());
            for record in &report.records {
                let mark = if record.valid { "ok" } else { "PROBLEM" };
                out.push_str(&format!("  [{mark}] {}\n", record.path));
                if let Some(id) = &record.id {
                    out.push_str(&format!("         id         {id}\n"));
                }
                if let Some(field_id) = &record.field_id {
                    out.push_str(&format!("         field      {field_id}\n"));
                }
                if let Some(record_type) = &record.record_type {
                    out.push_str(&format!("         type       {record_type}\n"));
                }
                if let Some(occurred_at) = &record.occurred_at {
                    out.push_str(&format!("         occurred   {occurred_at}\n"));
                }
                for artifact in &record.artifacts {
                    out.push_str(&format!("         artifact   {artifact}\n"));
                }
                for problem in &record.problems {
                    out.push_str(&format!("         {} {}\n", problem.kind, problem.message));
                }
                if let Some(body) = &record.body {
                    out.push('\n');
                    for line in body.lines() {
                        // Indent only content, so no blank line carries
                        // trailing whitespace.
                        if line.is_empty() {
                            out.push('\n');
                        } else {
                            out.push_str(&format!("         {line}\n"));
                        }
                    }
                }
            }
            for artifact in &report.artifacts {
                let mark = if artifact.valid { "ok" } else { "PROBLEM" };
                let reference = if artifact.referenced {
                    "referenced"
                } else {
                    "unreferenced"
                };
                out.push_str(&format!(
                    "  [{mark}] {} ({} bytes, {reference})\n",
                    artifact.path, artifact.bytes
                ));
                for problem in &artifact.problems {
                    out.push_str(&format!("         {} {}\n", problem.kind, problem.message));
                }
            }
            for staged in &report.interrupted_writes {
                out.push_str(&format!("  [PROBLEM] {staged} (interrupted write)\n"));
            }
            out.push_str(if report.healthy {
                "Notebook is valid\n"
            } else {
                "Notebook has problems\n"
            });
            out
        }
        Format::Json => {
            let records: Vec<Json> = report
                .records
                .iter()
                .map(|record| {
                    Json::Obj(vec![
                        ("path", Json::text(record.path.clone())),
                        ("id", Json::maybe_text(record.id.clone())),
                        ("field_id", Json::maybe_text(record.field_id.clone())),
                        ("type", Json::maybe_text(record.record_type.clone())),
                        ("occurred_at", Json::maybe_text(record.occurred_at.clone())),
                        (
                            "artifacts",
                            Json::Arr(record.artifacts.iter().cloned().map(Json::Str).collect()),
                        ),
                        ("valid", Json::Bool(record.valid)),
                        ("problems", problems_json(&record.problems)),
                        ("body", Json::maybe_text(record.body.clone())),
                    ])
                })
                .collect();
            let artifacts: Vec<Json> = report
                .artifacts
                .iter()
                .map(|artifact| {
                    Json::Obj(vec![
                        ("path", Json::text(artifact.path.clone())),
                        ("bytes", Json::Int(artifact.bytes)),
                        ("referenced", Json::Bool(artifact.referenced)),
                        ("valid", Json::Bool(artifact.valid)),
                        ("problems", problems_json(&artifact.problems)),
                    ])
                })
                .collect();
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.inspect.v1")),
                ("ok", Json::Bool(report.healthy)),
                ("root", Json::text(report.root.display().to_string())),
                (
                    "instance_id",
                    Json::text(report.instance.instance_id.to_string()),
                ),
                ("records", Json::Arr(records)),
                ("artifacts", Json::Arr(artifacts)),
                (
                    "interrupted_writes",
                    Json::Arr(
                        report
                            .interrupted_writes
                            .iter()
                            .cloned()
                            .map(Json::Str)
                            .collect(),
                    ),
                ),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn problems_json(problems: &[fieldnotes_app::ReportedProblem]) -> Json {
    Json::Arr(
        problems
            .iter()
            .map(|problem| {
                Json::Obj(vec![
                    ("kind", Json::text(problem.kind.clone())),
                    ("message", Json::text(problem.message.clone())),
                ])
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_surface_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_positional_body_conflicts_with_the_preferred_inputs() {
        // Explicitness prevents an ambiguous mixture of body sources.
        assert!(Cli::try_parse_from(["fieldnotes", "note", "text", "--stdin"]).is_err());
        assert!(
            Cli::try_parse_from(["fieldnotes", "note", "--file", "a", "--voice", "b"]).is_err()
        );
        assert!(Cli::try_parse_from(["fieldnotes", "note", "--stdin"]).is_ok());
    }
}
