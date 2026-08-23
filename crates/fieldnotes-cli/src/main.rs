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
//! - `fieldnotes.fields_add.v1`
//! - `fieldnotes.fields_list.v1`
//! - `fieldnotes.fields_status.v1`
//! - `fieldnotes.fields_remove.v1`
//! - `fieldnotes.sync.v1`
//! - `fieldnotes.config.v1`
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
//! | 3 | notebook contract violation (an unhealthy `inspect`, a required manifest migration, or a `sync` in which any Field did not complete) |
//! | 4 | notebook state error, such as no notebook found |
//! | 5 | filesystem failure |
//! | 70 | internal error |
//!
//! A `sync` that leaves any Field short of `complete` exits 3 while still
//! printing every Field's report, because A2 requires one Field's failure not to
//! abandon the others and durable work committed before a failure to stand.
//!
//! # Secret hygiene
//!
//! Nothing echoes or logs the process arguments. Note bodies are preferably
//! supplied through `--stdin` or `--body-file`, because a positional argument
//! can be captured by shell history; only the Note's own declared content is
//! ever persisted.
//!
//! # Persistent profile
//!
//! A user-level profile outside any notebook can record a default notebook
//! path and a default timezone; see [`config`] for its file location and
//! [`timezone`] for how a timezone setting becomes a numeric UTC offset. Every
//! setting resolves through the same order: an explicit flag, then an
//! environment variable, then the profile, then `0.1.0`'s existing behavior
//! (working-directory discovery for the notebook, UTC for the offset).

mod config;
mod environment;
mod json;
mod timezone;

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use fieldnotes_app::{
    AppError, FieldStatusReport, FieldSummary, FieldSyncReport, InitOutcome, InspectReport, Kernel,
    NoteOutcome, NoteRequest, NoteSource, StatusReport, SyncMode, SyncOptions, SyncOutcome,
    add_field, create_note, field_status, init, inspect, list_fields, remove_field, status,
    validate_artifact_max_bytes, validate_artifact_media_types, validate_field_id,
};
use fieldnotes_domain::{Clock, Datetime};
use fieldnotes_store::{FieldConfig, InitState, LastSyncOutcome, Notebook, Profile};

use crate::environment::{OFFSET_ENV, OsRandom, SystemClock};
use crate::json::Json;
use crate::timezone::TimeZoneSpec;

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
    /// Notebook to operate on. Highest-precedence source; then the
    /// FIELDNOTES_NOTEBOOK environment variable; then the profile's
    /// `notebook` setting (see `fieldnotes config`); then the notebook
    /// containing the working directory.
    #[arg(long, global = true, value_name = "PATH")]
    notebook: Option<PathBuf>,

    /// Output form.
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    format: Format,

    /// Timezone for generated datetimes: `system`, an IANA zone name such as
    /// `Europe/Zurich`, or a fixed +HH:MM/-HH:MM/utc offset. Highest-
    /// precedence source; then FIELDNOTES_TIMEZONE (or the legacy
    /// FIELDNOTES_UTC_OFFSET); then the profile's `timezone` setting (see
    /// `fieldnotes config`); then utc.
    #[arg(long, global = true, value_name = "OFFSET|ZONE")]
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
        /// Record this notebook as the profile's default, even if one is
        /// already recorded. Without this flag, `init` only records a
        /// default when the profile has none yet, so it never silently
        /// overwrites a default you or an earlier `init` already chose.
        #[arg(long)]
        set_default: bool,
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
    /// Show or change the persistent user profile (default notebook and
    /// timezone). Not part of any notebook: see the module documentation for
    /// its file location.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage Field configuration.
    ///
    /// A Field is a stable producer of Notes. `self` is the built-in Field
    /// and is not managed here. Configuring, enabling, disabling, or removing
    /// a Field is entirely local bookkeeping: no `fields` command starts a
    /// Field process, contacts a source, or writes to one.
    Fields {
        #[command(subcommand)]
        action: FieldsAction,
    },
    /// Collect current source state through the Field process contract.
    ///
    /// With a Field ID, runs that one Field. Without one, runs every enabled
    /// configured Field; one Field's failure never abandons the others.
    ///
    /// Collection is read-only with respect to every source. A Note is removed
    /// only under an authoritative tombstone or a completed authoritative
    /// snapshot; absence from a partial, windowed, or failed run is never
    /// deletion.
    Sync {
        /// The Field to sync. Omit to sync every enabled Field.
        field_id: Option<String>,
        /// Reconcile a whole scope instead of moving forward from the cursor.
        ///
        /// This is the only mode in which a Field's completeness claim can
        /// authorize removing Notes it did not report.
        #[arg(long)]
        snapshot: bool,
        /// The scope a snapshot run reconciles. Defaults to the single distinct
        /// `source_scope` the Field's existing Notes carry.
        #[arg(long, value_name = "SCOPE", requires = "snapshot")]
        scope: Option<String>,
        /// The single-artifact retention threshold in bytes, for this run only.
        ///
        /// Overrides the profile's `artifact_max_bytes` and the protocol
        /// default of 26214400 (25 MiB). A larger attachment stays at its
        /// source and its reference is recorded in `skipped_attachments`.
        #[arg(long, value_name = "BYTES")]
        max_artifact_bytes: Option<u64>,
        /// A media type or subtype wildcard to retain, repeatable, for this run
        /// only.
        ///
        /// Overrides the profile's `artifact_media_types` and the approved
        /// default include set. An attachment of an excluded type stays at its
        /// source exactly as an oversize one does.
        #[arg(long = "media-type", value_name = "TYPE/SUBTYPE")]
        media_types: Vec<String>,
        /// The run wall clock in seconds, up to the frozen 3600-second ceiling.
        #[arg(long, value_name = "SECONDS")]
        run_seconds: Option<u64>,
        /// Seconds without a frame before the run is considered idle.
        #[arg(long, value_name = "SECONDS")]
        idle_seconds: Option<u32>,
    },
}

/// A `fieldnotes fields` action.
#[derive(Debug, Subcommand)]
enum FieldsAction {
    /// Configure a new external Field.
    ///
    /// `<type>` must be a registered Field stem (for example `local`); `self`
    /// is reserved and cannot be added this way. The resulting Field ID is
    /// `<type>_<label>` and is immutable once configured: to reconfigure,
    /// remove it first.
    ///
    /// Fieldnotes never searches `PATH` or otherwise discovers a Field
    /// executable implicitly, so `--executable` is required and always names
    /// an exact, pinned path.
    Add {
        /// Registered Field type (stem), e.g. `local`.
        r#type: String,
        /// User-chosen label. Combined with `<type>` as `<type>_<label>`.
        label: String,
        /// Pinned path to the Field's executable.
        #[arg(long, value_name = "PATH")]
        executable: PathBuf,
        /// Non-secret `key=value` connector configuration, repeatable.
        /// Credential material must never appear here: a handful of
        /// obviously credential-shaped key names (`password`, `token`,
        /// `api_key`, and similar) are refused outright, and a future
        /// release's credential-profile reference belongs here as a name,
        /// never as a secret value.
        #[arg(long = "config", value_name = "KEY=VALUE")]
        config: Vec<String>,
        /// Configure the Field disabled rather than enabled.
        #[arg(long)]
        disabled: bool,
    },
    /// List every Field: the built-in `self` Field plus every configured
    /// external Field.
    List,
    /// Show per-Field state useful before and after a sync: enabled or not,
    /// whether a durable cursor is recorded, whether a `describe` manifest
    /// snapshot is recorded, and the last recorded sync outcome, if any.
    ///
    /// `0.1.1` does not run Fields itself, so a freshly configured Field
    /// normally shows no cursor, no manifest, and no sync outcome yet; that
    /// state is populated once a later `sync` implementation runs it.
    Status {
        /// Show only this Field. Omit to show every Field.
        field_id: Option<String>,
    },
    /// Remove one external Field's configuration and operational sync state.
    ///
    /// This never deletes Notes or artifacts: Notes and retained artifacts
    /// are the notebook's canonical evidence and remain, attributable to
    /// their original producer, whether or not the Field that produced them
    /// is still configured. Removing a Field also never touches any other
    /// Field's configuration or Notes. `self` cannot be removed.
    Remove {
        /// The Field to remove.
        field_id: String,
    },
}

/// A `fieldnotes config` action.
#[derive(Debug, Subcommand)]
enum ConfigAction {
    /// Show every recorded profile setting and the profile's file location.
    Show,
    /// Print one setting's recorded value.
    Get {
        /// Which setting to print.
        key: ConfigKey,
    },
    /// Record one setting, after validating it.
    Set {
        /// Which setting to record.
        key: ConfigKey,
        /// The value to record: a notebook path, or a timezone (`system`, an
        /// IANA zone name, or a fixed +HH:MM/-HH:MM/utc offset).
        value: String,
    },
}

/// A profile setting `fieldnotes config` can read or write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ConfigKey {
    /// The default notebook path.
    Notebook,
    /// The default timezone.
    Timezone,
    /// The default single-artifact retention threshold, in bytes.
    ArtifactMaxBytes,
    /// The default artifact media-type retention include set, comma-separated.
    ArtifactMediaTypes,
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
            "empty_note"
            | "not_audio"
            | "invalid_offset"
            | "unknown_target"
            | "invalid_profile"
            | "invalid_field_config"
            | "credential_shaped_config_key"
            | "invalid_field_id"
            | "cannot_configure_self"
            | "field_already_configured"
            | "field_not_configured"
            | "invalid_manifest" => EXIT_USAGE,
            // A migration is required before this Field may sync again: the
            // notebook's own configuration state disagrees with a freshly
            // reported manifest, which is the same class of problem as an
            // unhealthy notebook (`inspect`'s EXIT_CONTRACT), not a usage
            // mistake or an internal bug.
            "manifest_migration_required" => EXIT_CONTRACT,
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
    // The profile is loaded once, in this one place, and everything below
    // resolves through the same flag-then-environment-then-profile order
    // implemented in `config::resolve_notebook_start` and
    // `config::resolve_timezone_text`. A malformed profile fails here for
    // every command rather than being silently skipped.
    let profile_path = config::resolve_profile_path();
    let profile = match &profile_path {
        Some(path) => fieldnotes_store::read_profile(path)
            .map_err(|error| Failure::from(AppError::from(error)))?,
        None => Profile::default(),
    };

    let notebook_start = config::resolve_notebook_start(
        cli.notebook.clone(),
        non_empty_env(config::NOTEBOOK_ENV),
        profile.notebook.clone(),
    );
    let timezone_text = config::resolve_timezone_text(
        cli.offset.clone(),
        non_empty_env(config::TIMEZONE_ENV),
        non_empty_env(OFFSET_ENV),
        profile.timezone.clone(),
    );
    let timezone_spec = match &timezone_text {
        Some(text) => TimeZoneSpec::parse(text)
            .map_err(|error| Failure::new("invalid_offset", error.to_string(), EXIT_USAGE))?,
        // Documented final fallback: UTC. A1 requires an explicit numeric
        // offset on every datetime and forbids a timezone-less value, and
        // guessing a local zone with no configuration at all would be a
        // silent, unreviewable choice.
        None => TimeZoneSpec::Fixed(0),
    };
    let now_millis = i64::try_from(SystemClock.unix_millis()).unwrap_or(i64::MAX);
    let offset = timezone_spec
        .resolve_minutes(now_millis)
        .map_err(|error| Failure::new("invalid_offset", error.to_string(), EXIT_USAGE))?;

    match cli.command {
        Command::Init {
            path,
            name,
            set_default,
        } => {
            let root = path
                .or_else(|| cli.notebook.clone())
                .unwrap_or_else(|| PathBuf::from("."));
            let mut kernel = Kernel::new(SystemClock, OsRandom, offset)?;
            let outcome = init(&mut kernel, &root, name.as_deref())?;
            let recorded_default = match &profile_path {
                Some(path) => config::record_default_notebook_if_absent(
                    path,
                    &profile,
                    &outcome.root,
                    set_default,
                )
                .map_err(|error| Failure::from(AppError::from(error)))?,
                None => false,
            };
            print(&render_init(&outcome, recorded_default, cli.format));
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
            let notebook = open_notebook(notebook_start)?;
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
            let notebook = open_notebook(notebook_start)?;
            let report = status(&notebook)?;
            print(&render_status(&report, cli.format));
            Ok(EXIT_OK)
        }
        Command::Inspect { target } => {
            let notebook = open_notebook(notebook_start)?;
            let report = inspect(&notebook, target.as_deref())?;
            print(&render_inspect(&report, cli.format));
            Ok(if report.healthy {
                EXIT_OK
            } else {
                EXIT_CONTRACT
            })
        }
        Command::Config { action } => {
            let text = run_config(action, profile_path.as_deref(), &profile, cli.format)?;
            print(&text);
            Ok(EXIT_OK)
        }
        Command::Fields { action } => run_fields(action, notebook_start, cli.format),
        Command::Sync {
            field_id,
            snapshot,
            scope,
            max_artifact_bytes,
            media_types,
            run_seconds,
            idle_seconds,
        } => {
            let notebook = open_notebook(notebook_start)?;
            let options = SyncOptions {
                mode: if snapshot {
                    SyncMode::Snapshot
                } else {
                    SyncMode::Incremental
                },
                snapshot_scope: scope,
                // Flag, then profile, then the protocol's own approved default.
                max_artifact_bytes: max_artifact_bytes.or(profile.artifact_max_bytes),
                artifact_media_types: resolve_media_types(
                    &media_types,
                    profile.artifact_media_types.as_deref(),
                ),
                run_seconds,
                idle_seconds,
                durability: fieldnotes_app::DurabilityPolicy::AllSucceed,
                // Core builds the child's environment rather than inheriting
                // it, and the CLI widens that allowlist by nothing at all.
                field_environment: std::collections::BTreeMap::new(),
            };
            let mut kernel = Kernel::new(SystemClock, OsRandom, offset)?;
            let outcome =
                fieldnotes_app::sync(&mut kernel, &notebook, field_id.as_deref(), &options)?;
            print(&render_sync(&outcome, cli.format));
            Ok(if outcome.ok() { EXIT_OK } else { EXIT_CONTRACT })
        }
    }
}

/// Resolves the media-type retention include set from the flag, then the
/// profile, then `None` for the protocol's approved default set.
fn resolve_media_types(flag: &[String], profile: Option<&str>) -> Option<Vec<String>> {
    if !flag.is_empty() {
        return Some(flag.to_vec());
    }
    profile.map(config::split_media_types)
}

/// Splits one `KEY=VALUE` argument, rejecting a missing `=` or an empty key.
fn parse_config_pair(pair: &str) -> Result<(String, String), Failure> {
    let (key, value) = pair.trim().split_once('=').ok_or_else(|| {
        Failure::new(
            "invalid_config_pair",
            format!("`--config {pair}` is not `KEY=VALUE`"),
            EXIT_USAGE,
        )
    })?;
    let key = key.trim();
    if key.is_empty() {
        return Err(Failure::new(
            "invalid_config_pair",
            format!("`--config {pair}` has an empty key"),
            EXIT_USAGE,
        ));
    }
    Ok((key.to_owned(), value.trim().to_owned()))
}

fn parse_config_pairs(
    pairs: &[String],
) -> Result<std::collections::BTreeMap<String, String>, Failure> {
    let mut config = std::collections::BTreeMap::new();
    for pair in pairs {
        let (key, value) = parse_config_pair(pair)?;
        config.insert(key, value);
    }
    Ok(config)
}

fn run_fields(
    action: FieldsAction,
    notebook_start: Option<PathBuf>,
    format: Format,
) -> Result<i32, Failure> {
    let notebook = open_notebook(notebook_start)?;
    match action {
        FieldsAction::Add {
            r#type,
            label,
            executable,
            config,
            disabled,
        } => {
            let field_id = validate_field_id(&r#type, &label)?;
            let config = parse_config_pairs(&config)?;
            let added = add_field(&notebook, &field_id, executable, config, !disabled)?;
            print(&render_fields_add(&added, format));
            Ok(EXIT_OK)
        }
        FieldsAction::List => {
            let fields = list_fields(&notebook)?;
            print(&render_fields_list(&fields, format));
            Ok(EXIT_OK)
        }
        FieldsAction::Status { field_id } => {
            let reports = field_status(&notebook, field_id.as_deref())?;
            print(&render_fields_status(&reports, format));
            Ok(EXIT_OK)
        }
        FieldsAction::Remove { field_id } => {
            remove_field(&notebook, &field_id)?;
            print(&render_fields_remove(&field_id, format));
            Ok(EXIT_OK)
        }
    }
}

/// Reads an environment variable, treating an unset or blank value as absent.
fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Locates the notebook to operate on, falling back to the working directory
/// when no flag, environment variable, or profile setting names a start path.
fn open_notebook(start: Option<PathBuf>) -> Result<Notebook, Failure> {
    let start = match start {
        Some(path) => path,
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

fn render_init(outcome: &InitOutcome, recorded_default: bool, format: Format) -> String {
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
            if recorded_default {
                out.push_str("  default   recorded in the Fieldnotes profile (see `fieldnotes config show`)\n");
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
                ("recorded_default", Json::Bool(recorded_default)),
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

fn render_fields_add(config: &FieldConfig, format: Format) -> String {
    match format {
        Format::Human => {
            let mut out = format!("Configured Field {}\n", config.id);
            out.push_str(&format!(
                "  enabled     {}\n",
                if config.enabled { "yes" } else { "no" }
            ));
            out.push_str(&format!("  executable  {}\n", config.executable.display()));
            for (key, value) in &config.config {
                out.push_str(&format!("  config      {key} = {value}\n"));
            }
            out
        }
        Format::Json => {
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.fields_add.v1")),
                ("ok", Json::Bool(true)),
                ("id", Json::text(config.id.clone())),
                ("enabled", Json::Bool(config.enabled)),
                (
                    "executable",
                    Json::text(config.executable.display().to_string()),
                ),
                ("config", config_map_json(&config.config)),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn config_map_json(config: &std::collections::BTreeMap<String, String>) -> Json {
    Json::Arr(
        config
            .iter()
            .map(|(key, value)| {
                Json::Obj(vec![
                    ("key", Json::text(key.clone())),
                    ("value", Json::text(value.clone())),
                ])
            })
            .collect(),
    )
}

fn render_fields_list(fields: &[FieldSummary], format: Format) -> String {
    match format {
        Format::Human => {
            let mut out = String::from("Fields\n");
            for field in fields {
                let origin = if field.built_in {
                    "built-in".to_owned()
                } else {
                    field.executable.as_ref().map_or_else(
                        || "configured".to_owned(),
                        |path| path.display().to_string(),
                    )
                };
                let state = if field.enabled { "enabled" } else { "disabled" };
                out.push_str(&format!("  {:<20} {state:<9} {origin}\n", field.id));
            }
            out
        }
        Format::Json => {
            let items: Vec<Json> = fields
                .iter()
                .map(|field| {
                    Json::Obj(vec![
                        ("id", Json::text(field.id.clone())),
                        ("built_in", Json::Bool(field.built_in)),
                        ("enabled", Json::Bool(field.enabled)),
                        (
                            "executable",
                            Json::maybe_text(
                                field
                                    .executable
                                    .as_ref()
                                    .map(|path| path.display().to_string()),
                            ),
                        ),
                    ])
                })
                .collect();
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.fields_list.v1")),
                ("ok", Json::Bool(true)),
                ("fields", Json::Arr(items)),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn last_sync_json(last_sync: Option<&LastSyncOutcome>) -> Json {
    match last_sync {
        Some(outcome) => Json::Obj(vec![
            ("outcome", Json::text(outcome.outcome.clone())),
            ("at", Json::text(outcome.at.clone())),
        ]),
        None => Json::Null,
    }
}

fn render_fields_status(reports: &[FieldStatusReport], format: Format) -> String {
    match format {
        Format::Human => {
            let mut out = String::from("Field status\n");
            for report in reports {
                out.push_str(&format!("  {}\n", report.id));
                out.push_str(&format!(
                    "    enabled            {}\n",
                    if report.enabled { "yes" } else { "no" }
                ));
                match (
                    report.cursor_present,
                    report.cursor_format_version,
                    report.cursor_coverage,
                ) {
                    (true, Some(version), Some(coverage)) => out.push_str(&format!(
                        "    cursor recorded    yes (format v{version}, covering records through \
                         seq {coverage})\n"
                    )),
                    (true, _, _) => out.push_str("    cursor recorded    yes\n"),
                    (false, _, _) => out.push_str("    cursor recorded    no\n"),
                }
                if let Some(at) = &report.cursor_committed_at {
                    out.push_str(&format!("    cursor committed   {at}\n"));
                }
                match (
                    report.cursor_format_version,
                    report.manifest_cursor_format_version,
                ) {
                    (Some(stored), Some(declared)) if stored != declared => out.push_str(&format!(
                        "    recovery gap       the recorded cursor is format v{stored} but the \
                         Field now declares v{declared}; the next run starts unbounded\n"
                    )),
                    _ => {}
                }
                out.push_str(&format!(
                    "    manifest recorded  {}\n",
                    match (
                        report.manifest_present,
                        report.manifest_cursor_format_version
                    ) {
                        (true, Some(version)) => format!("yes (cursor format v{version})"),
                        (true, None) => "yes".to_owned(),
                        (false, _) => "no".to_owned(),
                    }
                ));
                match &report.last_sync {
                    Some(outcome) => out.push_str(&format!(
                        "    last sync          {} at {}\n",
                        outcome.outcome, outcome.at
                    )),
                    None => out.push_str("    last sync          never\n"),
                }
            }
            out
        }
        Format::Json => {
            let items: Vec<Json> = reports
                .iter()
                .map(|report| {
                    Json::Obj(vec![
                        ("id", Json::text(report.id.clone())),
                        ("built_in", Json::Bool(report.built_in)),
                        ("enabled", Json::Bool(report.enabled)),
                        ("cursor_present", Json::Bool(report.cursor_present)),
                        (
                            "cursor_format_version",
                            report
                                .cursor_format_version
                                .map_or(Json::Null, |version| Json::Int(u64::from(version))),
                        ),
                        (
                            "cursor_coverage",
                            report.cursor_coverage.map_or(Json::Null, Json::Int),
                        ),
                        (
                            "cursor_committed_at",
                            Json::maybe_text(report.cursor_committed_at.clone()),
                        ),
                        ("manifest_present", Json::Bool(report.manifest_present)),
                        (
                            "manifest_cursor_format_version",
                            report
                                .manifest_cursor_format_version
                                .map_or(Json::Null, |version| Json::Int(u64::from(version))),
                        ),
                        ("last_sync", last_sync_json(report.last_sync.as_ref())),
                    ])
                })
                .collect();
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.fields_status.v1")),
                ("ok", Json::Bool(true)),
                ("fields", Json::Arr(items)),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn render_sync(outcome: &SyncOutcome, format: Format) -> String {
    match format {
        Format::Human => {
            if outcome.fields.is_empty() {
                return "Sync\n  no enabled Field is configured in this notebook\n".to_owned();
            }
            let mut out = String::from("Sync\n");
            for report in &outcome.fields {
                out.push_str(&format!(
                    "  {} ({}) {}\n",
                    report.field_id, report.mode, report.outcome
                ));
                let counts = &report.counts;
                out.push_str(&format!(
                    "    notes              {} new, {} updated, {} unchanged, {} removed\n",
                    counts.created,
                    counts.updated,
                    counts.unchanged,
                    counts.removed_by_tombstone + counts.removed_by_snapshot
                ));
                out.push_str(&format!(
                    "    artifacts          {} stored, {} reused, {} attachments skipped\n",
                    counts.artifacts_stored, counts.artifacts_reused, counts.attachments_skipped
                ));
                if counts.damaged > 0 || counts.truncated > 0 {
                    out.push_str(&format!(
                        "    integrity          {} damaged, {} truncated\n",
                        counts.damaged, counts.truncated
                    ));
                }
                out.push_str(&format!(
                    "    cursor             {}\n",
                    match report.cursor_coverage {
                        Some(coverage) if report.cursor_committed =>
                            format!("committed, covering records through seq {coverage}"),
                        _ => "not advanced".to_owned(),
                    }
                ));
                if report.cursor_recovery_gap {
                    out.push_str(
                        "    recovery gap       the stored cursor was written at a different \
                         format version and was not replayed\n",
                    );
                }
                for reason in &report.withheld_checkpoints {
                    out.push_str(&format!("    withheld           {reason}\n"));
                }
                match &report.deletion.authorized_scope {
                    Some(scope) => out.push_str(&format!(
                        "    deletion           authorized inside {scope}\n"
                    )),
                    None => {
                        if !report.deletion.refusals.is_empty() {
                            out.push_str(&format!(
                                "    deletion           refused: {}\n",
                                report.deletion.refusals.join("; ")
                            ));
                        }
                    }
                }
                for conflict in &report.conflicts {
                    let rendered = conflict.replace('\t', " ");
                    out.push_str(&format!(
                        "    conflict           more than one active Note claims {rendered}\n"
                    ));
                }
                for diagnostic in &report.diagnostics {
                    out.push_str(&format!(
                        "    {} {} {}\n",
                        diagnostic.severity, diagnostic.code, diagnostic.message
                    ));
                }
                if let Some(rejection) = &report.rejection {
                    out.push_str(&format!(
                        "    rejected           {} {}\n",
                        rejection.code, rejection.detail
                    ));
                }
                if let Some(failure) = &report.failure {
                    out.push_str(&format!("    failed             {failure}\n"));
                }
            }
            out
        }
        Format::Json => {
            let fields: Vec<Json> = outcome.fields.iter().map(field_sync_json).collect();
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.sync.v1")),
                ("ok", Json::Bool(outcome.ok())),
                ("fields", Json::Arr(fields)),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn field_sync_json(report: &FieldSyncReport) -> Json {
    let counts = &report.counts;
    Json::Obj(vec![
        ("id", Json::text(report.field_id.clone())),
        ("mode", Json::text(report.mode.clone())),
        ("outcome", Json::text(report.outcome.as_str())),
        (
            "counts",
            Json::Obj(vec![
                ("records_accepted", Json::Int(counts.records_accepted)),
                ("created", Json::Int(counts.created)),
                ("updated", Json::Int(counts.updated)),
                ("unchanged", Json::Int(counts.unchanged)),
                (
                    "removed_by_tombstone",
                    Json::Int(counts.removed_by_tombstone),
                ),
                ("removed_by_snapshot", Json::Int(counts.removed_by_snapshot)),
                ("renamed", Json::Int(counts.renamed)),
                ("artifacts_stored", Json::Int(counts.artifacts_stored)),
                ("artifacts_reused", Json::Int(counts.artifacts_reused)),
                ("attachments_skipped", Json::Int(counts.attachments_skipped)),
                ("damaged", Json::Int(counts.damaged)),
                ("truncated", Json::Int(counts.truncated)),
                (
                    "durable_write_failures",
                    Json::Int(counts.durable_write_failures),
                ),
            ]),
        ),
        ("cursor_committed", Json::Bool(report.cursor_committed)),
        (
            "cursor_coverage",
            report.cursor_coverage.map_or(Json::Null, Json::Int),
        ),
        (
            "cursor_recovery_gap",
            Json::Bool(report.cursor_recovery_gap),
        ),
        (
            "withheld_checkpoints",
            Json::Arr(
                report
                    .withheld_checkpoints
                    .iter()
                    .map(|reason| Json::text(reason.clone()))
                    .collect(),
            ),
        ),
        (
            "deletion",
            Json::Obj(vec![
                (
                    "authorized_scope",
                    Json::maybe_text(report.deletion.authorized_scope.clone()),
                ),
                (
                    "refusals",
                    Json::Arr(
                        report
                            .deletion
                            .refusals
                            .iter()
                            .map(|reason| Json::text(reason.clone()))
                            .collect(),
                    ),
                ),
            ]),
        ),
        (
            "diagnostics",
            Json::Arr(
                report
                    .diagnostics
                    .iter()
                    .map(|diagnostic| {
                        Json::Obj(vec![
                            ("severity", Json::text(diagnostic.severity.clone())),
                            ("code", Json::text(diagnostic.code.clone())),
                            ("message", Json::text(diagnostic.message.clone())),
                            (
                                "source_identity",
                                Json::maybe_text(diagnostic.source_identity.clone()),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "conflicts",
            Json::Arr(
                report
                    .conflicts
                    .iter()
                    .map(|conflict| Json::text(conflict.clone()))
                    .collect(),
            ),
        ),
        (
            "rejection",
            report.rejection.as_ref().map_or(Json::Null, |rejection| {
                Json::Obj(vec![
                    ("code", Json::text(rejection.code.clone())),
                    ("detail", Json::text(rejection.detail.clone())),
                ])
            }),
        ),
        ("failure", Json::maybe_text(report.failure.clone())),
        ("exit", Json::text(report.exit.clone())),
    ])
}

fn render_fields_remove(field_id: &str, format: Format) -> String {
    match format {
        Format::Human => format!(
            "Removed Field {field_id}\n  Notes and artifacts it produced remain in the \
             notebook.\n"
        ),
        Format::Json => {
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.fields_remove.v1")),
                ("ok", Json::Bool(true)),
                ("id", Json::text(field_id.to_owned())),
                ("notes_preserved", Json::Bool(true)),
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

/// A failure produced when no profile location could be determined at all
/// (neither `FIELDNOTES_CONFIG` nor the platform's home/config variable is
/// set), for a `config` action that must write somewhere.
fn no_profile_location_failure() -> Failure {
    Failure::new(
        "io",
        "cannot determine the Fieldnotes profile location; set HOME (or APPDATA on \
         Windows), or set FIELDNOTES_CONFIG explicitly",
        EXIT_IO,
    )
}

/// The offset a timezone setting resolves to right now, rendered as
/// `+HH:MM`, or `None` if the setting no longer parses (for display only;
/// `config set` already validates before writing).
fn current_offset_display(timezone: &str) -> Option<String> {
    let spec = TimeZoneSpec::parse(timezone).ok()?;
    let now_millis = i64::try_from(SystemClock.unix_millis()).unwrap_or(i64::MAX);
    let minutes = spec.resolve_minutes(now_millis).ok()?;
    Some(TimeZoneSpec::Fixed(minutes).to_string())
}

/// Runs one `fieldnotes config` action and renders its result.
fn run_config(
    action: ConfigAction,
    profile_path: Option<&Path>,
    profile: &Profile,
    format: Format,
) -> Result<String, Failure> {
    match action {
        ConfigAction::Show => Ok(render_config_show(profile_path, profile, format)),
        ConfigAction::Get { key } => Ok(render_config_get(key, profile, format)),
        ConfigAction::Set { key, value } => {
            let path = profile_path.ok_or_else(no_profile_location_failure)?;
            let updated = match key {
                ConfigKey::Notebook => config::set_notebook(path, profile, Path::new(&value))
                    .map_err(|error| Failure::from(AppError::from(error)))?,
                ConfigKey::Timezone => {
                    let spec = TimeZoneSpec::parse(&value).map_err(|error| {
                        Failure::new("invalid_offset", error.to_string(), EXIT_USAGE)
                    })?;
                    config::set_timezone(path, profile, &spec.to_string())
                        .map_err(|error| Failure::from(AppError::from(error)))?
                }
                ConfigKey::ArtifactMaxBytes => {
                    let bytes: u64 = value.trim().parse().map_err(|_| {
                        Failure::new(
                            "invalid_setting",
                            format!("`{value}` is not a byte count"),
                            EXIT_USAGE,
                        )
                    })?;
                    // Configuring up toward the ceiling is exactly as legal as
                    // configuring down from the default; only crossing the
                    // frozen ceiling needs a protocol revision. The rule lives
                    // in the application layer, not here.
                    validate_artifact_max_bytes(bytes)
                        .map_err(|message| Failure::new("invalid_setting", message, EXIT_USAGE))?;
                    config::set_artifact_max_bytes(path, profile, bytes)
                        .map_err(|error| Failure::from(AppError::from(error)))?
                }
                ConfigKey::ArtifactMediaTypes => {
                    let entries = config::split_media_types(&value);
                    validate_artifact_media_types(&entries)
                        .map_err(|message| Failure::new("invalid_setting", message, EXIT_USAGE))?;
                    config::set_artifact_media_types(path, profile, &entries.join(","))
                        .map_err(|error| Failure::from(AppError::from(error)))?
                }
            };
            Ok(render_config_show(Some(path), &updated, format))
        }
    }
}

fn render_config_show(profile_path: Option<&Path>, profile: &Profile, format: Format) -> String {
    let notebook = profile
        .notebook
        .as_ref()
        .map(|path| path.display().to_string());
    let timezone = profile.timezone.clone();
    let resolved_offset = timezone.as_deref().and_then(current_offset_display);
    match format {
        Format::Human => {
            let mut out = String::from("Fieldnotes profile\n");
            out.push_str(&format!(
                "  file       {}\n",
                profile_path.map_or_else(
                    || "(not determined)".to_owned(),
                    |path| path.display().to_string()
                )
            ));
            match &notebook {
                Some(value) => out.push_str(&format!("  notebook   {value}\n")),
                None => out.push_str("  notebook   (not set; using working-directory discovery)\n"),
            }
            match (&timezone, &resolved_offset) {
                (Some(value), Some(offset)) => {
                    out.push_str(&format!("  timezone   {value} (currently {offset})\n"));
                }
                (Some(value), None) => out.push_str(&format!("  timezone   {value}\n")),
                (None, _) => out.push_str("  timezone   (not set; using utc)\n"),
            }
            match profile.artifact_max_bytes {
                Some(bytes) => {
                    out.push_str(&format!("  artifact_max_bytes    {bytes}\n"));
                }
                None => out.push_str(
                    "  artifact_max_bytes    (not set; using the approved 26214400-byte default)\n",
                ),
            }
            match &profile.artifact_media_types {
                Some(value) => {
                    out.push_str(&format!("  artifact_media_types  {value}\n"));
                }
                None => out.push_str(
                    "  artifact_media_types  (not set; using the approved default include set)\n",
                ),
            }
            out
        }
        Format::Json => {
            let value = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.config.v1")),
                ("ok", Json::Bool(true)),
                (
                    "file",
                    Json::maybe_text(profile_path.map(|path| path.display().to_string())),
                ),
                ("notebook", Json::maybe_text(notebook)),
                ("timezone", Json::maybe_text(timezone)),
                ("resolved_offset", Json::maybe_text(resolved_offset)),
                (
                    "artifact_max_bytes",
                    profile.artifact_max_bytes.map_or(Json::Null, Json::Int),
                ),
                (
                    "artifact_media_types",
                    Json::maybe_text(profile.artifact_media_types.clone()),
                ),
            ]);
            format!("{}\n", value.render())
        }
    }
}

fn render_config_get(key: ConfigKey, profile: &Profile, format: Format) -> String {
    let (label, value) = match key {
        ConfigKey::Notebook => (
            "notebook",
            profile
                .notebook
                .as_ref()
                .map(|path| path.display().to_string()),
        ),
        ConfigKey::Timezone => ("timezone", profile.timezone.clone()),
        ConfigKey::ArtifactMaxBytes => (
            "artifact_max_bytes",
            profile.artifact_max_bytes.map(|bytes| bytes.to_string()),
        ),
        ConfigKey::ArtifactMediaTypes => {
            ("artifact_media_types", profile.artifact_media_types.clone())
        }
    };
    match format {
        Format::Human => match &value {
            Some(value) => format!("{value}\n"),
            None => "(not set)\n".to_owned(),
        },
        Format::Json => {
            let json = Json::Obj(vec![
                ("schema", Json::text("fieldnotes.config.v1")),
                ("ok", Json::Bool(true)),
                ("key", Json::text(label)),
                ("value", Json::maybe_text(value)),
            ]);
            format!("{}\n", json.render())
        }
    }
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
