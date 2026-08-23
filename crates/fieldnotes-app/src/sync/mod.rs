//! `sync [field_id]`: the core-side orchestration that turns an approved
//! protocol conversation into durable notebook state.
//!
//! # How one run proceeds
//!
//! 1. Load the Field's durable configuration and remove any artifact staging
//!    left behind by an earlier crashed run.
//! 2. Run `describe` as a bounded child process and negotiate the protocol
//!    version, failing closed and actionably on a mismatch in either direction
//!    (A2 section 2). Negotiation happens before any staging directory or
//!    credential grant exists.
//! 3. Compare the reported manifest against the stored snapshot. A changed or
//!    removed declared-property type or cardinality, or a changed
//!    `cursor_format_version`, is a **migration, not a sync**, and core refuses
//!    (A2 section 4).
//! 4. Deliver a credential when the Field needs one: mint a short-lived access
//!    token from the stored refresh token, open the protected channel A2
//!    section 12 defines, and carry a **reference** to it in the collection
//!    request. See "Credentials fail before anything is spawned" below.
//! 5. Send one `collect_request` carrying the stored cursor when one is
//!    replayable, the requested mode and snapshot scope, the effective limits,
//!    the required media-type retention policy, the per-run staging directory,
//!    and the deadline.
//! 6. Consume the child's frames, turning accepted records into durable
//!    notebook state and committing cursors at eligible checkpoints.
//! 7. Remove Notes only when a completed authoritative snapshot proves their
//!    absence, record the run's outcome into
//!    `.fieldnotes/state/sync/<field_id>.status.json`, and remove the staging
//!    directory.
//!
//! # Durability ordering, and why the cursor cannot lead
//!
//! Per accepted record, in this exact order:
//!
//! 1. resolve every artifact reference — handle grammar, digest, declared
//!    length, media-type policy, and the run's staged-byte budget — touching no
//!    notebook file;
//! 2. install each original artifact durably (staged in `artifacts/`, fsynced,
//!    renamed, directory synced);
//! 3. locate the active Note by portable exact-source key and install the
//!    replacement atomically under the **existing** Note ID;
//! 4. remove a superseded event-time filename, only after the replacement
//!    exists durably;
//! 5. only then mark the record durable.
//!
//! A cursor is committed exclusively by
//! [`CollectSession::commit`], which refuses unless every accepted record at or below the checkpoint's
//! coverage has been marked durable — and step 5 is the only place that mark is
//! made. The durable cursor file is then written last. So a cursor may lag,
//! which costs repeated work that reconciliation makes a no-op, and it cannot
//! lead, which would lose an upstream object permanently.
//!
//! # Checkpoint eligibility when `seq` spans event kinds
//!
//! `seq` is shared by records, checkpoints, and diagnostics, so a checkpoint
//! declaring `covers_record_seq_through: N` may cover far fewer than `N`
//! records. Eligibility is tracked per **accepted record** by
//! [`CollectSession`], never as a contiguous watermark over raw `seq` values — a watermark would
//! silently never commit the moment a run emitted one diagnostic before the
//! record a later checkpoint covers, which the `local` Field does routinely
//! (it reports a skipped symlink as an `info` diagnostic before any record).
//!
//! # Reconciliation
//!
//! Every record is matched to at most one active Note by the portable
//! `(source_scope, source_identity)` pair. A match preserves that Note's ID and
//! atomically rewrites its frontmatter and body; no match mints a new Note ID.
//! Syncing a changed source object twice therefore leaves exactly one Note, and
//! no revision or history Note is ever written.
//!
//! # Credentials fail before anything is spawned
//!
//! A missing, expired, or revoked credential must fail a run **before** a child
//! process starts and before a staging directory exists, and it must never
//! advance a cursor. That ordering is why the credential is resolved from the
//! Field's **configuration** rather than from its manifest: the manifest is the
//! authority on what a Field needs, but reading it means running `describe`,
//! which is already a spawned process. Configuration is available with no
//! process at all, an authenticating Field cannot work without
//! [`credentials::PROFILE_KEY`] anyway, and
//! the manifest is still checked afterwards — so a Field that declares
//! authentication while configuring none is refused after `describe` and still
//! before any staging directory is created.
//!
//! Nothing about a credential failure can move a cursor: every credential check
//! happens before [`CollectSession`] exists, and a cursor is written in exactly
//! one place, from a checkpoint that session offered.
//!
//! A collection run never authorizes interactively. It refreshes silently or it
//! refuses with an instruction to run `fieldnotes fields auth`, so a scheduled
//! run cannot open a browser on an unattended machine.
//!
//! # Out of scope here
//!
//! No re-collection pass over `skipped_attachments` (the `local` Field declares
//! `refetch: unsupported`, so this release specifies nothing there), and no
//! conflict-bundle behavior (`0.1.2`): a portable source key that more than one
//! active Note claims is **reported** as a reached boundary rather than
//! resolved.

mod project;
mod report;

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use fieldnotes_domain::{Clock, Datetime, FieldId, FieldStemRegistry, RandomSource, RecordKind};
use fieldnotes_field_protocol::artifact::{ArtifactDigestIndex, ArtifactOutcome};
use fieldnotes_field_protocol::codes::RejectionCode;
use fieldnotes_field_protocol::declared::DeclaredPropertyIndex;
use fieldnotes_field_protocol::grammar::{
    CollectRequestTag, Cursor, DescribeRequestTag, FieldIdToken, GrantId, MediaTypeMatcher,
    OffsetDatetime, ProfileRef, ProtocolV1, RunId, SnapshotScope,
};
use fieldnotes_field_protocol::host::{FieldSpawn, Operation};
use fieldnotes_field_protocol::limits::{
    Deadline, Limits, MAX_ARTIFACT_MEDIA_TYPES, default_artifact_media_types, media_type_essence,
};
use fieldnotes_field_protocol::message::{
    ArtifactKind, ArtifactRef, CollectRequest, CollectionMode, CoreFrame, CredentialGrant,
    DescribeRequest, FieldEvent, Manifest, RecordEvent, Validate, VersionList,
};
use fieldnotes_field_protocol::redact::Redactor;
use fieldnotes_field_protocol::session::{
    AcceptedEvent, CheckpointOffer, CollectSession, ExitObservation, RecordDisposition, Rejection,
};
use fieldnotes_field_protocol::value::{ConfigMap, PropertyValue};
use fieldnotes_field_protocol::version::{PROTOCOL_REVISION, PROTOCOL_VERSION, negotiate};
use fieldnotes_format::{
    CanonicalRecord, ParsedRecord, detect_media_type, record_fingerprint, semantic_record_string,
};
use fieldnotes_store::{
    FieldConfig, IndexedNote, LastSyncOutcome, Notebook, SourceIndex, StoredCursor,
    build_source_index, clear_staging, create_staging_dir, find_artifact, list_field_configs,
    read_cursor, read_field_config, read_instance, remove_note, replace_note, store_artifact,
    write_cursor, write_last_sync_outcome, write_note,
};

use crate::credentials::channel::{GrantSpec, ProtectedChannel};
use crate::credentials::{self, CredentialFailure, CredentialSettings, CredentialSource};
use crate::error::AppError;
use crate::fields::{check_manifest_agreement, record_manifest};
use crate::kernel::Kernel;

use project::{ArtifactProjection, RetainedArtifact, SkippedArtifact};
pub use report::{
    CredentialReport, DeletionReport, FieldRunOutcome, FieldSyncReport, SyncCounts, SyncDiagnostic,
    SyncOutcome, SyncRejection, exit_label,
};

/// Which collection mode a run requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncMode {
    /// Move forward from the last committed cursor. Never authorizes deletion
    /// by absence.
    #[default]
    Incremental,
    /// Reconcile a whole declared scope. The only mode in which a Field's
    /// completeness claim can authorize removing Notes it did not report.
    Snapshot,
}

impl SyncMode {
    /// A stable lowercase label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SyncMode::Incremental => "incremental",
            SyncMode::Snapshot => "snapshot",
        }
    }

    fn protocol(self) -> CollectionMode {
        match self {
            SyncMode::Incremental => CollectionMode::Incremental,
            SyncMode::Snapshot => CollectionMode::Snapshot,
        }
    }
}

/// How core's own durable writes behave during a run.
///
/// The release gate requires proving that a cursor never advances past a failed
/// durable write, and no portable filesystem can be made to fail exactly one
/// write on demand. The failure is therefore injected here, exactly as the
/// protocol crate's own conformance kit injects it. The CLI never sets anything
/// but the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DurabilityPolicy {
    /// Every durable write is attempted for real.
    #[default]
    AllSucceed,
    /// The Note write for this record sequence number is not performed, and the
    /// record is never marked durable.
    FailAt(u64),
}

impl DurabilityPolicy {
    fn succeeds(self, seq: u64) -> bool {
        match self {
            DurabilityPolicy::AllSucceed => true,
            DurabilityPolicy::FailAt(failing) => seq != failing,
        }
    }
}

/// Everything one `sync` invocation may configure.
#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    /// Incremental or snapshot.
    pub mode: SyncMode,
    /// The scope a snapshot run claims to reconcile.
    ///
    /// A Field computes its own portable `source_scope` at run time and the
    /// manifest declares only its *shape*, so core cannot derive the value.
    /// When this is `None`, a snapshot run infers the scope from the single
    /// distinct `source_scope` the Field's existing Notes carry, and refuses
    /// when there is not exactly one.
    pub snapshot_scope: Option<String>,
    /// The effective single-artifact retention threshold in bytes. `None` uses
    /// the protocol default (25 MiB), which is well below the frozen 512 MiB
    /// ceiling.
    pub max_artifact_bytes: Option<u64>,
    /// The effective media-type retention include set, as text. `None` uses the
    /// protocol default include set approved by ADR 0007.
    pub artifact_media_types: Option<Vec<String>>,
    /// The run wall clock in seconds. `None` uses the protocol default (600).
    pub run_seconds: Option<u64>,
    /// Seconds without a frame before the run is idle. `None` uses the protocol
    /// default (120).
    pub idle_seconds: Option<u32>,
    /// Fault injection for the release gate's cursor-durability evidence.
    pub durability: DurabilityPolicy,
    /// Extra allowlisted environment entries for the child process.
    ///
    /// Core builds the child's environment rather than inheriting it, and this
    /// widens that allowlist by exactly these names. **Never a secret**:
    /// credential material crosses only on the protected channel, and an
    /// inherited environment leaks to grandchild processes, appears in crash
    /// dumps, and lives as long as the process. The CLI passes none; the
    /// conformance tests use it to select a fixture Field's scenario, which is
    /// the same purpose
    /// [`FieldSpawn::with_env`](fieldnotes_field_protocol::host::FieldSpawn::with_env)
    /// exists for.
    pub field_environment: std::collections::BTreeMap<String, String>,
    /// Where this run gets an access token for a Field that needs one.
    ///
    /// Defaults to [`CredentialSource::None`], which refuses an authenticating
    /// Field rather than reaching for the keychain and the network from inside
    /// a library; the composition root injects the real source. A Field that
    /// needs no credential never consults this at all.
    pub credentials: CredentialSource,
}

/// Syncs one named Field, or every enabled Field when `field_id` is `None`.
///
/// One Field's failure never abandons the others: every Field gets its own
/// report naming its outcome, its counts, and which cursor it committed.
pub fn sync<C: Clock, R: RandomSource>(
    kernel: &mut Kernel<C, R>,
    notebook: &Notebook,
    field_id: Option<&str>,
    options: &SyncOptions,
) -> Result<SyncOutcome, AppError> {
    let targets: Vec<FieldConfig> = match field_id {
        Some(id) => {
            if id == crate::kernel::SELF_FIELD {
                return Err(AppError::CannotConfigureSelf);
            }
            vec![
                read_field_config(notebook, id)?
                    .ok_or_else(|| AppError::FieldNotConfigured { id: id.to_owned() })?,
            ]
        }
        None => list_field_configs(notebook)?
            .into_iter()
            .filter(|config| config.enabled)
            .collect(),
    };

    let mut fields = Vec::with_capacity(targets.len());
    for config in targets {
        fields.push(run_field(kernel, notebook, &config, options));
    }
    Ok(SyncOutcome { fields })
}

/// Runs one Field, always producing a report.
fn run_field<C: Clock, R: RandomSource>(
    kernel: &mut Kernel<C, R>,
    notebook: &Notebook,
    config: &FieldConfig,
    options: &SyncOptions,
) -> FieldSyncReport {
    let mode = options.mode.as_str();
    if !config.enabled {
        return FieldSyncReport::not_run(
            &config.id,
            mode,
            FieldRunOutcome::Skipped,
            "the Field is configured disabled; enable it before syncing",
        );
    }
    match prepare_and_collect(kernel, notebook, config, options) {
        Ok(report) => report,
        Err(refusal) => {
            FieldSyncReport::not_run(&config.id, mode, FieldRunOutcome::Failed, refusal.message)
        }
    }
}

/// A refusal to start or to continue a run, already phrased for a user.
struct Refusal {
    message: String,
}

impl Refusal {
    fn new(message: impl Into<String>) -> Self {
        Refusal {
            message: message.into(),
        }
    }
}

impl From<AppError> for Refusal {
    fn from(error: AppError) -> Self {
        Refusal::new(error.to_string())
    }
}

impl From<fieldnotes_store::StoreError> for Refusal {
    fn from(error: fieldnotes_store::StoreError) -> Self {
        Refusal::new(error.to_string())
    }
}

impl From<CredentialFailure> for Refusal {
    /// A credential failure is already phrased for a user, including what to
    /// run to fix it, so it becomes the refusal text unchanged.
    fn from(failure: CredentialFailure) -> Self {
        Refusal::new(failure.to_string())
    }
}

/// A minted access token together with the non-secret settings it came from.
///
/// Deliberately has no `Debug` implementation: `AccessToken`'s own `Debug`
/// redacts, and not deriving one here means there is not even a redacted
/// formatting call to reach for.
struct Minted {
    settings: CredentialSettings,
    token: fieldnotes_credentials::oauth::AccessToken,
}

/// Resolves and mints this run's credential from the Field's configuration,
/// before anything else happens.
///
/// `Ok(None)` means the Field's configuration names no credential profile,
/// which is the ordinary case for a Field that needs none. An error means a
/// credential was called for and could not be produced: missing, expired,
/// revoked, or a store that could not be read. In every one of those cases the
/// run has not spawned a process, has not created a staging directory, and has
/// not touched the cursor.
fn mint_credential(config: &FieldConfig, options: &SyncOptions) -> Result<Option<Minted>, Refusal> {
    if !credentials::config_declares_credential(&config.config) {
        return Ok(None);
    }
    let settings = credentials::settings_from_config(&config.id, &config.config)?;
    let token = options.credentials.mint(&config.id, &settings)?;
    Ok(Some(Minted { settings, token }))
}

/// Opens the protected channel for one run and returns it, already serving.
///
/// The grant expires at the earlier of the access token's own expiry and the
/// run's deadline, so material a Field holds cannot outlive either. Enforcement
/// inside the channel is monotonic; the instant in the frame is the wall-clock
/// rendering of the same bound.
#[allow(clippy::too_many_arguments)]
fn open_channel(
    field_id: &str,
    run_id: &RunId,
    grant_id: String,
    minted: Minted,
    scopes: &[String],
    started_at: Datetime,
    deadline: Deadline,
    limits: Limits,
    offset_minutes: i16,
) -> Result<ProtectedChannel, Refusal> {
    let grant_id = GrantId::parse(&grant_id).map_err(|error| {
        Refusal::new(format!(
            "the generated channel grant identifier is invalid: {error}"
        ))
    })?;
    let profile = ProfileRef::parse(minted.settings.profile.as_str()).map_err(|error| {
        Refusal::new(format!(
            "the configured credential profile is not a protocol profile reference: {error}"
        ))
    })?;
    let token_remaining_millis = minted
        .token
        .expires_at_unix_millis()
        .saturating_sub(started_at.unix_millis());
    if token_remaining_millis <= 0 {
        // The authorization server minted something already past its own
        // expiry, or the clock moved: either way, refusing here is the same
        // actionable outcome as an expired refresh token.
        return Err(Refusal::from(CredentialFailure::Expired {
            field_id: field_id.to_owned(),
            profile: minted.settings.profile.as_str().to_owned(),
        }));
    }
    // The run's own wall-clock bound, as `effective_deadline` computed it.
    let run_remaining_millis = deadline
        .not_after
        .datetime()
        .unix_millis()
        .saturating_sub(started_at.unix_millis())
        .max(1);
    let lifetime_millis = token_remaining_millis.min(run_remaining_millis);
    let expires_millis = started_at
        .unix_millis()
        .saturating_add(lifetime_millis.max(1));
    let expires_at = Datetime::from_unix_millis(expires_millis, offset_minutes)
        .map_err(|error| Refusal::new(format!("the grant expiry is not representable: {error}")))?;
    let expires_at = OffsetDatetime::parse(&expires_at.to_string())
        .map_err(|error| Refusal::new(format!("the grant expiry failed its own guard: {error}")))?;
    let spec = GrantSpec {
        run_id: run_id.clone(),
        profile,
        grant_id,
        scopes: scopes.to_vec(),
        expires_at,
        lifetime: Duration::from_millis(u64::try_from(lifetime_millis).unwrap_or(u64::MAX)),
        token: minted.token,
        max_frame_bytes: limits.max_frame_bytes,
    };
    ProtectedChannel::open(spec).map_err(Refusal::from)
}

/// Runs `describe` against one configured Field and returns its manifest.
///
/// Exposed because `fields auth` needs a Field's declared scopes before any
/// credential exists, and A2 guarantees a describe run carries no credential
/// grant, no cursor, and no staging directory. It shares the same bounded
/// spawn, negotiation, and failure handling one sync run uses.
pub fn describe_field<C: Clock, R: RandomSource>(
    kernel: &mut Kernel<C, R>,
    config: &FieldConfig,
) -> Result<Manifest, AppError> {
    let options = SyncOptions::default();
    let mut describe_run = || -> Result<Manifest, Refusal> {
        let field_token = FieldIdToken::parse(&config.id).map_err(|error| {
            Refusal::new(format!("`{}` is not a Field ID token: {error}", config.id))
        })?;
        let limits = effective_limits(&options)?;
        let (run_id_text, started_at) = kernel.new_run().map_err(Refusal::from)?;
        let run_id = RunId::parse(&run_id_text).map_err(|error| {
            Refusal::new(format!("the generated run identifier is invalid: {error}"))
        })?;
        let deadline = effective_deadline(&options, started_at, kernel.offset_minutes())?;
        let idle = Duration::from_secs(u64::from(deadline.idle_seconds));
        let wait = Duration::from_secs(u64::from(deadline.cancel_grace_seconds).saturating_add(
            remaining_seconds(options.run_seconds.unwrap_or(Deadline::DEFAULT_RUN_SECONDS)),
        ));
        describe(
            config,
            &run_id,
            &field_token,
            limits,
            deadline,
            idle,
            wait,
            &options,
        )
        .map(|described| described.manifest)
    };
    describe_run().map_err(|refusal| AppError::FieldDescribe {
        message: refusal.message,
    })
}

fn prepare_and_collect<C: Clock, R: RandomSource>(
    kernel: &mut Kernel<C, R>,
    notebook: &Notebook,
    config: &FieldConfig,
    options: &SyncOptions,
) -> Result<FieldSyncReport, Refusal> {
    let stems = FieldStemRegistry::v1();
    let field_id = FieldId::parse(&config.id, stems).map_err(|error| {
        Refusal::new(format!("`{}` is not a valid Field ID: {error}", config.id))
    })?;
    let field_token = FieldIdToken::parse(&config.id).map_err(|error| {
        Refusal::new(format!("`{}` is not a Field ID token: {error}", config.id))
    })?;
    let instance = read_instance(notebook)?;

    // The credential comes first, before recovery, before any process, and
    // before any staging directory: a run that cannot authenticate must fail
    // here, where nothing has happened yet and no cursor can move. See this
    // module's "Credentials fail before anything is spawned".
    let minted = mint_credential(config, options)?;
    // Captured before the token is moved into the channel, so the report can
    // name the provider without the settings outliving the run.
    let credential_provider = minted
        .as_ref()
        .map(|minted| minted.settings.provider.as_str());

    // Startup recovery: staged bytes from a crashed earlier run reference no
    // Note, because the record they belonged to was never accepted.
    clear_staging(notebook, &config.id)?;

    let limits = effective_limits(options)?;
    let media_types = effective_media_types(options)?;
    let (run_id_text, started_at) = kernel.new_run().map_err(Refusal::from)?;
    let run_id = RunId::parse(&run_id_text).map_err(|error| {
        Refusal::new(format!("the generated run identifier is invalid: {error}"))
    })?;
    let deadline = effective_deadline(options, started_at, kernel.offset_minutes())?;
    let idle = Duration::from_secs(u64::from(deadline.idle_seconds));
    // A hung or killed child must never wedge a run: the wait is the run's own
    // wall clock plus the cancellation grace period, and nothing longer.
    let wait = Duration::from_secs(u64::from(deadline.cancel_grace_seconds).saturating_add(
        remaining_seconds(options.run_seconds.unwrap_or(Deadline::DEFAULT_RUN_SECONDS)),
    ));

    let described = describe(
        config,
        &run_id,
        &field_token,
        limits,
        deadline,
        idle,
        wait,
        options,
    )?;
    let manifest = described.manifest;

    // A Field configured under one registered stem must not report a manifest
    // declaring another: its prefixed properties would then belong to a
    // different Field's stem, which A1 ruling 4 forbids on this Note. Failing
    // here says so once, instead of rejecting every record later.
    let declared_stem = manifest.field_stem.as_str();
    if config.id != declared_stem && !config.id.starts_with(&format!("{declared_stem}_")) {
        return Err(Refusal::new(format!(
            "Field `{}` is configured under the registered stem this ID implies, but its manifest \
             declares stem `{declared_stem}`; a Field's prefixed properties belong to its own stem, \
             so core refuses rather than collecting Notes under a mismatched producer",
            config.id
        )));
    }

    // A migration, not a sync: core refuses rather than retyping notebook data
    // in place or losing track of what a no-longer-declared name meant.
    let manifest_json = serde_json::to_value(&manifest).map_err(|error| {
        Refusal::new(format!(
            "the reported manifest could not be re-encoded: {error}"
        ))
    })?;
    if let Some(stored) = &config.manifest {
        check_manifest_agreement(stored, &manifest_json).map_err(Refusal::from)?;
    }
    record_manifest(notebook, &config.id, manifest_json).map_err(Refusal::from)?;

    // The manifest is the authority on what this Field needs. Configuration
    // already decided whether a credential was minted; this is where the two
    // are reconciled, still before any staging directory exists.
    let requirement = credentials::requirement_of_manifest(&config.id, &manifest)?;
    if requirement.required && minted.is_none() {
        return Err(Refusal::from(CredentialFailure::NotConfigured {
            field_id: config.id.clone(),
            detail: format!(
                "its manifest requires authentication, so set `--config {}=<name>` and run \
                 `fieldnotes fields auth {}`",
                credentials::PROFILE_KEY,
                config.id
            ),
        }));
    }

    if !manifest
        .collection
        .supported_modes
        .contains(&options.mode.protocol())
    {
        return Err(Refusal::new(format!(
            "Field `{}` does not support {} collection; its manifest declares {:?}",
            config.id,
            options.mode.as_str(),
            manifest.collection.supported_modes
        )));
    }

    let index = build_source_index(notebook)?;
    let snapshot_scope = match options.mode {
        SyncMode::Incremental => None,
        SyncMode::Snapshot => Some(resolve_snapshot_scope(options, &index, &config.id)?),
    };

    // A2 section 9: a cursor stored at a different format version is not
    // replayed. The run starts unbounded and reports a recovery gap rather than
    // handing a Field a token it may misread.
    let stored_cursor = read_cursor(notebook, &config.id)?;
    let mut cursor_recovery_gap = false;
    let replayable = match &stored_cursor {
        Some(stored)
            if stored.cursor_format_version == manifest.collection.cursor_format_version =>
        {
            match Cursor::parse(&stored.cursor) {
                Ok(cursor) => Some((cursor, stored.cursor_format_version)),
                Err(_) => {
                    cursor_recovery_gap = true;
                    None
                }
            }
        }
        Some(_) => {
            cursor_recovery_gap = true;
            None
        }
        None => None,
    };

    // The protected channel is opened before the staging directory and before
    // the child, so every credential-shaped failure still precedes both. It is
    // held for exactly the length of the run: dropping it stops serving,
    // unlinks the socket, and zeroizes the token.
    let channel = match (requirement.required, minted) {
        (true, Some(minted)) => Some(open_channel(
            &config.id,
            &run_id,
            kernel.new_grant_id().map_err(Refusal::from)?,
            minted,
            &requirement.scopes,
            started_at,
            deadline,
            limits,
            kernel.offset_minutes(),
        )?),
        // A Field that declares no authentication receives no grant, even when
        // a profile happens to be configured: A2 gives core nothing to deliver
        // to it.
        _ => None,
    };

    let staging = create_staging_dir(notebook, &config.id, run_id.as_str())?;
    let request = build_request(
        &run_id,
        &field_token,
        options.mode,
        replayable,
        snapshot_scope.clone(),
        config,
        &staging,
        limits,
        deadline,
        media_types,
        channel.as_ref().map(|channel| channel.grant().clone()),
    )?;

    let outcome = collect(CollectContext {
        kernel,
        notebook,
        config,
        field_id: &field_id,
        instance_id: &instance.instance_id,
        manifest: &manifest,
        stems,
        request: &request,
        staging: &staging,
        index,
        options,
        idle,
        wait,
        cursor_recovery_gap,
        started_at,
        described_stderr: described.stderr,
    });

    // The staging directory is operational sync state, and core removes it when
    // the run ends whether the run succeeded or not.
    let _ = clear_staging(notebook, &config.id);

    // The channel stops serving, unlinks its socket, and drops (zeroizing) the
    // token here, on every path out of this function including an error one.
    let credential = channel.as_ref().map(|channel| {
        let counts = channel.counts();
        CredentialReport {
            profile: channel.grant().profile_ref.as_str().to_owned(),
            provider: credential_provider.unwrap_or("keychain").to_owned(),
            scopes: channel.grant().scopes.clone().unwrap_or_default(),
            requests: counts.requests,
            granted: counts.granted,
            refused: counts.refused,
        }
    });
    drop(channel);

    let mut report = outcome?;
    report.credential = credential;
    write_last_sync_outcome(notebook, &config.id, &status_file(&report, started_at))?;
    Ok(report)
}

/// How many seconds of wall clock a run may take, bounded by the frozen
/// ceiling.
fn remaining_seconds(configured: u64) -> u64 {
    configured.min(Deadline::MAX_RUN_SECONDS)
}

/// Validates a configured single-artifact retention threshold.
///
/// A2 section 14 makes this a configurable *default*: a notebook may move it in
/// either direction between the product's minimum and the frozen 512 MiB
/// ceiling, and only crossing that ceiling requires a protocol revision. The
/// rule lives here rather than in a user interface, so every entry point checks
/// the same one.
pub fn validate_artifact_max_bytes(bytes: u64) -> Result<(), String> {
    let mut limits = Limits::defaults();
    limits.max_artifact_bytes = bytes;
    limits.validate().map_err(|error| error.to_string())
}

/// Validates a configured artifact media-type retention include set.
pub fn validate_artifact_media_types(entries: &[String]) -> Result<(), String> {
    if entries.is_empty() {
        return Err(
            "an empty media-type include set would retain nothing at all; omit the setting to use \
             the approved default include set"
                .to_owned(),
        );
    }
    if entries.len() > MAX_ARTIFACT_MEDIA_TYPES {
        return Err(format!(
            "an artifact media-type retention set carries at most {MAX_ARTIFACT_MEDIA_TYPES} \
             entries, not {}",
            entries.len()
        ));
    }
    for entry in entries {
        MediaTypeMatcher::parse(entry.trim()).map_err(|error| {
            format!("`{entry}` is not a media type or subtype wildcard: {error}")
        })?;
    }
    Ok(())
}

fn effective_limits(options: &SyncOptions) -> Result<Limits, Refusal> {
    let mut limits = Limits::defaults();
    if let Some(bytes) = options.max_artifact_bytes {
        limits.max_artifact_bytes = bytes;
    }
    limits.validate().map_err(|error| {
        Refusal::new(format!(
            "the configured artifact retention threshold is not usable: {error}"
        ))
    })?;
    Ok(limits)
}

fn effective_media_types(options: &SyncOptions) -> Result<Vec<MediaTypeMatcher>, Refusal> {
    let Some(configured) = &options.artifact_media_types else {
        return Ok(default_artifact_media_types());
    };
    validate_artifact_media_types(configured).map_err(Refusal::new)?;
    configured
        .iter()
        .map(|entry| {
            MediaTypeMatcher::parse(entry.trim()).map_err(|error| {
                Refusal::new(format!(
                    "`{entry}` is not a media type or subtype wildcard: {error}"
                ))
            })
        })
        .collect()
}

fn effective_deadline(
    options: &SyncOptions,
    started_at: Datetime,
    offset_minutes: i16,
) -> Result<Deadline, Refusal> {
    let run_seconds =
        remaining_seconds(options.run_seconds.unwrap_or(Deadline::DEFAULT_RUN_SECONDS));
    let millis = started_at
        .unix_millis()
        .saturating_add(i64::try_from(run_seconds.saturating_mul(1000)).unwrap_or(i64::MAX));
    let not_after = Datetime::from_unix_millis(millis, offset_minutes)
        .map_err(|error| Refusal::new(format!("the run deadline is not representable: {error}")))?;
    let deadline = Deadline {
        not_after: OffsetDatetime::parse(&not_after.to_string()).map_err(|error| {
            Refusal::new(format!("the run deadline failed its own guard: {error}"))
        })?,
        idle_seconds: options
            .idle_seconds
            .unwrap_or(Deadline::DEFAULT_IDLE_SECONDS),
        cancel_grace_seconds: Deadline::DEFAULT_CANCEL_GRACE_SECONDS,
    };
    deadline
        .validate()
        .map_err(|error| Refusal::new(format!("the run deadline is not usable: {error}")))?;
    Ok(deadline)
}

/// Resolves the scope a snapshot run claims.
///
/// **This is where A2 leaves core without an answer.** A snapshot run must send
/// an explicit `snapshot_scope`, deletion by absence is refused unless the
/// Field's completeness claim names exactly that scope, and yet a Field's
/// `source_scope` value is computed by the Field at run time — the manifest
/// declares its `scope_shape` for review, not its value. The notebook's own
/// Notes are therefore the only place core can learn it before a run, so the
/// scope is inferred from them when the caller did not name one, and refused
/// when the notebook holds no Note for the Field or more than one scope.
fn resolve_snapshot_scope(
    options: &SyncOptions,
    index: &SourceIndex,
    field_id: &str,
) -> Result<SnapshotScope, Refusal> {
    if let Some(explicit) = &options.snapshot_scope {
        return SnapshotScope::parse(explicit).map_err(|error| {
            Refusal::new(format!("`{explicit}` is not a snapshot scope: {error}"))
        });
    }
    let scopes = index.scopes_for(field_id);
    match scopes.as_slice() {
        [single] => SnapshotScope::parse(single).map_err(|error| {
            Refusal::new(format!(
                "the scope `{single}` this Field's Notes carry is not a usable snapshot scope: \
                 {error}"
            ))
        }),
        [] => Err(Refusal::new(format!(
            "a snapshot run must name the scope it reconciles, and Field `{field_id}` has no Note \
             in this notebook to infer one from. Run an incremental sync first, or name the scope \
             explicitly."
        ))),
        many => Err(Refusal::new(format!(
            "Field `{field_id}` has Notes in {} distinct source scopes ({}), so core cannot infer \
             which one a snapshot reconciles. Name the scope explicitly.",
            many.len(),
            many.join(", ")
        ))),
    }
}

/// What one describe run produced.
struct Described {
    manifest: Manifest,
    stderr: String,
}

/// Builds the spawn recipe for one operation, widening the child's sanitized
/// environment by exactly the configured allowlist entries.
fn spawn_recipe(
    config: &FieldConfig,
    operation: Operation,
    options: &SyncOptions,
) -> Result<FieldSpawn, Refusal> {
    let mut spawn = FieldSpawn::new(config.executable.clone(), operation).map_err(|error| {
        Refusal::new(format!(
            "cannot run Field `{}` at `{}`: {error}",
            config.id,
            config.executable.display()
        ))
    })?;
    for (name, value) in &options.field_environment {
        spawn = spawn.with_env(name, value);
    }
    Ok(spawn)
}

/// Runs `describe` and settles version negotiation, failing closed.
#[allow(clippy::too_many_arguments)]
fn describe(
    config: &FieldConfig,
    run_id: &RunId,
    field_token: &FieldIdToken,
    limits: Limits,
    deadline: Deadline,
    idle: Duration,
    wait: Duration,
    options: &SyncOptions,
) -> Result<Described, Refusal> {
    let spawn = spawn_recipe(config, Operation::Describe, options)?;
    let mut process = spawn.spawn(limits).map_err(|error| {
        Refusal::new(format!(
            "cannot start Field `{}` at `{}`: {error}",
            config.id,
            config.executable.display()
        ))
    })?;
    let offered = VersionList::new([PROTOCOL_VERSION])
        .map_err(|error| Refusal::new(format!("core's own version list is invalid: {error}")))?;
    let request = DescribeRequest {
        v: ProtocolV1,
        frame_type: DescribeRequestTag,
        run_id: run_id.clone(),
        supported_protocol_versions: offered.clone(),
        max_protocol_revision: PROTOCOL_REVISION,
        field_id: Some(field_token.clone()),
        limits: Some(limits),
        deadline,
    };
    let mut failure: Option<String> = None;
    if let Err(error) = process.send(&CoreFrame::Describe(Box::new(request))) {
        failure = Some(format!("could not send the describe request: {error}"));
    }
    process.close_stdin();

    let mut manifest: Option<Manifest> = None;
    if failure.is_none() {
        match process.next_event(idle) {
            Ok(Some(FieldEvent::Manifest(frame))) => {
                match negotiate(
                    offered.as_slice(),
                    PROTOCOL_REVISION,
                    protocol_major(&frame),
                    frame.protocol_revision,
                    frame.supported_protocol_versions.as_slice(),
                ) {
                    Ok(_) => match frame.validate() {
                        Ok(()) => manifest = Some(*frame),
                        Err(error) => {
                            failure = Some(format!("the reported manifest is invalid: {error}"));
                        }
                    },
                    Err(error) => failure = Some(error.to_string()),
                }
            }
            Ok(Some(_)) => {
                failure = Some("a describe run answers with exactly one manifest".to_owned());
            }
            Ok(None) => {
                failure = Some(format!(
                    "Field `{}` emitted no manifest. A Field that supports no protocol version \
                     core offered emits none deliberately: core offers version {PROTOCOL_VERSION}. \
                     Upgrade Fieldnotes or install a matching Field build.",
                    config.id
                ));
            }
            Err(error) => failure = Some(format!("{}: {}", error.code, error.detail)),
        }
    }

    // A describe run answers with exactly one manifest, so once it has been
    // read (or refused) there is nothing left to consume and the child is given
    // only the cancellation grace period to end on its own.
    let grace = Duration::from_secs(u64::from(deadline.cancel_grace_seconds));
    let exit = match process.wait(if failure.is_some() { grace } else { wait }) {
        Ok(ExitObservation::Timeout) if failure.is_some() => ExitObservation::TerminatedByCore,
        Ok(observed) => observed,
        Err(error) => {
            return Err(Refusal::new(format!(
                "could not wait for the describe run: {error}"
            )));
        }
    };
    process.join_stderr();
    let stderr = Redactor::new().redact_log(&process.captured_stderr());

    match (manifest, failure) {
        (Some(manifest), None) => Ok(Described { manifest, stderr }),
        (_, Some(message)) => Err(Refusal::new(format!(
            "{message}{}{}",
            exit_suffix(exit),
            stderr_suffix(&stderr)
        ))),
        (None, None) => Err(Refusal::new(format!(
            "Field `{}` produced no manifest{}{}",
            config.id,
            exit_suffix(exit),
            stderr_suffix(&stderr)
        ))),
    }
}

fn protocol_major(manifest: &Manifest) -> u16 {
    // `protocol_version` is a schema constant of 1 for protocol v1; the value
    // core negotiates against is that constant, and a manifest declaring
    // anything else fails to decode at all.
    let _ = manifest;
    PROTOCOL_VERSION
}

fn exit_suffix(exit: ExitObservation) -> String {
    format!(" (child {})", exit_label(exit))
}

fn stderr_suffix(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("; the Field reported: {trimmed}")
    }
}

#[allow(clippy::too_many_arguments)]
fn build_request(
    run_id: &RunId,
    field_token: &FieldIdToken,
    mode: SyncMode,
    cursor: Option<(Cursor, u16)>,
    snapshot_scope: Option<SnapshotScope>,
    config: &FieldConfig,
    staging: &Path,
    limits: Limits,
    deadline: Deadline,
    artifact_media_types: Vec<MediaTypeMatcher>,
    credential: Option<CredentialGrant>,
) -> Result<CollectRequest, Refusal> {
    let staging_text = staging
        .to_str()
        .ok_or_else(|| {
            Refusal::new("the artifact staging directory path is not valid UTF-8".to_owned())
        })?
        .to_owned();
    let mut connector_config = ConfigMap::new();
    for (key, value) in &config.config {
        let name =
            fieldnotes_field_protocol::grammar::PropertyNameToken::parse(key).map_err(|error| {
                Refusal::new(format!("configuration key `{key}` is invalid: {error}"))
            })?;
        connector_config.insert(name, PropertyValue::Text(value.clone()));
    }
    let request = CollectRequest {
        v: ProtocolV1,
        frame_type: CollectRequestTag,
        run_id: run_id.clone(),
        protocol_version: ProtocolV1,
        protocol_revision: PROTOCOL_REVISION,
        field_id: field_token.clone(),
        mode: mode.protocol(),
        cursor: cursor.as_ref().map(|(cursor, _)| cursor.clone()),
        cursor_format_version: cursor.as_ref().map(|(_, version)| *version),
        window: None,
        snapshot_scope,
        config: connector_config,
        // A reference, never a value: the profile name, this run's single-use
        // grant, where to reach core, when the grant stops being honored, and
        // the granted scopes. Material crosses only on the channel it names.
        credential,
        artifact_staging_dir: staging_text,
        limits,
        deadline,
        artifact_media_types,
        // `recollect_targets` is deliberately never set: the re-collection
        // operation is separate work, and the `local` Field declares
        // `refetch: unsupported`.
        recollect_targets: None,
    };
    request.validate().map_err(|error| {
        Refusal::new(format!("core's own collection request is invalid: {error}"))
    })?;
    Ok(request)
}

/// Digests the notebook already stores, for a `digest_only` reference.
struct StoredDigests(BTreeSet<String>);

impl StoredDigests {
    fn of(notebook: &Notebook) -> Self {
        let mut digests = BTreeSet::new();
        if let Ok(entries) = std::fs::read_dir(notebook.artifacts_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(stem) = path.file_stem().and_then(|value| value.to_str())
                    && let Some(hex) = stem.strip_prefix(fieldnotes_domain::ArtifactId::PREFIX)
                {
                    digests.insert(hex.to_owned());
                }
            }
        }
        StoredDigests(digests)
    }
}

impl ArtifactDigestIndex for StoredDigests {
    fn contains_digest(&self, digest: &str) -> bool {
        self.0.contains(digest)
    }
}

/// Everything one collect run needs.
struct CollectContext<'a, C, R> {
    kernel: &'a mut Kernel<C, R>,
    notebook: &'a Notebook,
    config: &'a FieldConfig,
    field_id: &'a FieldId,
    instance_id: &'a fieldnotes_domain::RecordId,
    manifest: &'a Manifest,
    stems: &'a FieldStemRegistry,
    request: &'a CollectRequest,
    staging: &'a Path,
    index: SourceIndex,
    options: &'a SyncOptions,
    idle: Duration,
    wait: Duration,
    cursor_recovery_gap: bool,
    started_at: Datetime,
    described_stderr: String,
}

/// Consumes the child's frames and turns accepted records into durable
/// notebook state.
fn collect<C: Clock, R: RandomSource>(
    mut context: CollectContext<'_, C, R>,
) -> Result<FieldSyncReport, Refusal> {
    let declared = DeclaredPropertyIndex::new(context.manifest, context.stems)
        .map_err(|error| Refusal::new(format!("the reported manifest is unusable: {error}")))?;
    let mut session = CollectSession::new(context.request, context.manifest, context.stems)
        .map_err(|error| Refusal::new(format!("the run could not start: {error}")))?;

    let spawn = spawn_recipe(context.config, Operation::Collect, context.options)?;
    let mut process = spawn
        .spawn(context.request.limits)
        .map_err(|error| Refusal::new(format!("cannot start the Field: {error}")))?;

    let mut counts = SyncCounts::default();
    let mut diagnostics: Vec<SyncDiagnostic> = Vec::new();
    let mut withheld: Vec<String> = Vec::new();
    let mut committed: Option<StoredCursor> = None;
    let mut rejection: Option<Rejection> = None;
    let mut failure: Option<String> = None;
    let mut reported: BTreeSet<(String, String)> = BTreeSet::new();
    let mut digests = StoredDigests::of(context.notebook);
    let redactor = Redactor::new();

    if let Err(error) = process.send(&CoreFrame::Collect(Box::new(context.request.clone()))) {
        session.note_rejection();
        rejection = Some(Rejection::new(error.code, error.detail));
    }

    while rejection.is_none() && failure.is_none() {
        let frame = match process.next_frame(context.idle) {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(error) => {
                session.note_rejection();
                rejection = Some(Rejection::new(error.code, error.detail));
                break;
            }
        };
        let event = match FieldEvent::decode(frame.value) {
            Ok(event) => event,
            Err(error) => {
                session.note_rejection();
                rejection = Some(Rejection::new(error.code, error.message));
                break;
            }
        };
        let record = match &event {
            FieldEvent::Record(frame) => Some(frame.as_ref().clone()),
            _ => None,
        };
        let diagnostic = match &event {
            FieldEvent::Diagnostic(frame) => Some(frame.as_ref().clone()),
            _ => None,
        };
        match session.accept(event) {
            Err(error) => {
                rejection = Some(error);
                break;
            }
            Ok(AcceptedEvent::Record(accepted)) => {
                let Some(record) = record else {
                    rejection = Some(Rejection::new(
                        RejectionCode::ProtocolSchemaInvalid,
                        "an accepted record must be a record frame",
                    ));
                    break;
                };
                reported.insert((
                    record.source.scope.as_str().to_owned(),
                    record.source.identity.as_str().to_owned(),
                ));
                match apply_record(
                    &mut context,
                    &mut session,
                    &declared,
                    &record,
                    accepted.seq,
                    accepted.disposition,
                    &mut counts,
                    &mut digests,
                ) {
                    Ok(()) => {}
                    Err(ApplyError::Rejected(error)) => {
                        session.note_rejection();
                        rejection = Some(error);
                        break;
                    }
                    Err(ApplyError::Durability(message)) => {
                        // A durable write that did not complete leaves the
                        // cursor where it was; the next run replays from there
                        // and reconciliation makes the replay a no-op.
                        counts.durable_write_failures += 1;
                        failure = Some(message);
                        break;
                    }
                }
            }
            Ok(AcceptedEvent::Checkpoint(offer)) => {
                match commit_checkpoint(
                    context.notebook,
                    &context.config.id,
                    &mut session,
                    &offer,
                    context.started_at,
                ) {
                    Ok(Some(stored)) => committed = Some(stored),
                    Ok(None) => {}
                    Err(reason) => withheld.push(reason),
                }
            }
            Ok(AcceptedEvent::Diagnostic { .. }) => {
                // Two-layer redaction: the Field sanitized before emission and
                // named what it removed; core redacts again before display or
                // persistence.
                if let Some(diagnostic) = diagnostic {
                    let redacted = redactor.redact_diagnostic(&diagnostic);
                    diagnostics.push(SyncDiagnostic {
                        severity: redacted.severity.as_str().to_owned(),
                        code: redacted.code.as_str().to_owned(),
                        message: redacted.message.as_str().to_owned(),
                        source_identity: redacted
                            .source
                            .as_ref()
                            .map(|source| source.identity.as_str().to_owned()),
                    });
                }
            }
        }
    }

    // On a rejection or a durable-write failure core stops consuming output, so
    // the child may be blocked writing to a pipe nobody is reading. A2 section
    // 14 says core terminates it after the grace period rather than waiting out
    // the run's whole wall clock, which is also what keeps a hung or killed
    // child from wedging a test run.
    let exit = if rejection.is_some() || failure.is_some() {
        let grace = Duration::from_secs(u64::from(context.request.deadline.cancel_grace_seconds));
        match process.wait(grace) {
            Ok(ExitObservation::Timeout) => ExitObservation::TerminatedByCore,
            Ok(observed) => observed,
            Err(error) => {
                return Err(Refusal::new(format!(
                    "could not terminate the collect run: {error}"
                )));
            }
        }
    } else {
        process
            .wait(context.wait)
            .map_err(|error| Refusal::new(format!("could not wait for the collect run: {error}")))?
    };
    process.join_stderr();
    let mut stderr = redactor.redact_log(&process.captured_stderr());
    if process.stderr_truncated() {
        stderr.push_str(&format!(
            "\n[core] standard error was truncated; {} bytes were dropped\n",
            process.stderr_dropped_bytes()
        ));
    }
    let report = session.finish(exit);

    let mut outcome = FieldRunOutcome::from_run(report.outcome);
    if outcome == FieldRunOutcome::Complete
        && (counts.durable_write_failures > 0 || !withheld.is_empty())
    {
        // Durable work happened and the run did not complete: a withheld
        // checkpoint means the cursor did not reach where the Field offered.
        outcome = FieldRunOutcome::Partial;
    }

    let mut deletion = DeletionReport::from_authorization(&report.deletion);
    if deletion.authorized_scope.is_some()
        && (counts.durable_write_failures > 0 || !withheld.is_empty())
    {
        deletion = DeletionReport {
            authorized_scope: None,
            refusals: vec!["a durable write in this run did not complete".to_owned()],
        };
    }
    if let Some(scope) = deletion.authorized_scope.clone()
        && outcome == FieldRunOutcome::Complete
    {
        match remove_absent_notes(&mut context, &scope, &reported) {
            Ok(removed) => counts.removed_by_snapshot = removed,
            Err(error) => failure = Some(error.message),
        }
    }

    let conflicts = context
        .index
        .duplicate_keys()
        .iter()
        .map(|(_, scope, identity)| format!("{scope}\t{identity}"))
        .collect();

    Ok(FieldSyncReport {
        field_id: context.config.id.clone(),
        mode: context.options.mode.as_str().to_owned(),
        outcome,
        counts,
        diagnostics,
        cursor_committed: committed.is_some(),
        cursor_coverage: committed
            .as_ref()
            .map(|stored| stored.covers_record_seq_through),
        withheld_checkpoints: withheld,
        cursor_recovery_gap: context.cursor_recovery_gap,
        deletion,
        rejection: rejection
            .as_ref()
            .map(|error| SyncRejection::new(error.code, error.detail.clone())),
        failure,
        exit: exit_label(exit),
        stderr: combined_stderr(&context.described_stderr, &stderr),
        conflicts,
        // Filled in by `prepare_and_collect`, which owns the channel: the
        // counts are only final once it has stopped serving.
        credential: None,
    })
}

fn combined_stderr(described: &str, collected: &str) -> Option<String> {
    let mut text = String::new();
    for part in [described, collected] {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(trimmed);
        }
    }
    if text.is_empty() { None } else { Some(text) }
}

/// Why applying one record to the notebook failed.
enum ApplyError {
    /// The record itself is unusable: the run fails with a protocol code.
    Rejected(Rejection),
    /// A durable write did not complete. Not a protocol violation: the cursor
    /// simply never advances past it.
    Durability(String),
}

impl From<Rejection> for ApplyError {
    fn from(error: Rejection) -> Self {
        ApplyError::Rejected(error)
    }
}

/// Applies one accepted record, in the fixed persistence order.
#[allow(clippy::too_many_arguments)]
fn apply_record<C: Clock, R: RandomSource>(
    context: &mut CollectContext<'_, C, R>,
    session: &mut CollectSession<'_>,
    declared: &DeclaredPropertyIndex<'_>,
    record: &RecordEvent,
    seq: u64,
    disposition: RecordDisposition,
    counts: &mut SyncCounts,
    digests: &mut StoredDigests,
) -> Result<(), ApplyError> {
    counts.records_accepted += 1;
    if let Some(integrity) = &record.integrity {
        if integrity.damaged {
            counts.damaged += 1;
        }
        if integrity.truncated {
            counts.truncated += 1;
        }
    }
    let scope = record.source.scope.as_str().to_owned();
    let identity = record.source.identity.as_str().to_owned();

    if disposition == RecordDisposition::NoChange {
        // The same source key was already asserted with an identical payload in
        // this run: core rewrites nothing.
        counts.unchanged += 1;
        session.record_durable(seq);
        return Ok(());
    }

    if disposition == RecordDisposition::Delete {
        // The session already refused a delete record from a Field whose
        // manifest does not declare authoritative tombstones, so reaching here
        // means the authority was declared.
        if let Some(existing) = context.index.get(&context.config.id, &scope, &identity) {
            let filename = existing.filename.clone();
            remove_note(context.notebook, &filename)
                .map_err(|error| ApplyError::Durability(error.to_string()))?;
            context.index.remove(&context.config.id, &scope, &identity);
            counts.removed_by_tombstone += 1;
        }
        session.record_durable(seq);
        return Ok(());
    }

    // 1. Resolve every artifact reference. Nothing is written yet.
    let outcomes = session
        .resolve_artifacts(record, context.staging, digests)
        .map_err(ApplyError::Rejected)?;
    // 2. Make the originals durable, before the Note that references them.
    let projection = install_artifacts(context, record, &outcomes, counts, digests)?;
    counts.attachments_skipped += u64::try_from(projection.skipped.len()).unwrap_or(0);

    let existing = context
        .index
        .get(&context.config.id, &scope, &identity)
        .cloned();
    let (note_id, captured_at) = match &existing {
        Some(found) => {
            let (_, captured_at) = context
                .kernel
                .new_record(RecordKind::Note)
                .map_err(|error| ApplyError::Durability(error.to_string()))?;
            (found.note_id, captured_at)
        }
        None => context
            .kernel
            .new_record(RecordKind::Note)
            .map_err(|error| ApplyError::Durability(error.to_string()))?,
    };

    let candidate = project::build_note(
        &note_id,
        context.instance_id,
        context.field_id,
        record,
        &projection,
        captured_at,
        declared,
    )
    .map_err(ApplyError::Rejected)?;

    if let Some(found) = &existing
        && unchanged(found, &candidate).map_err(ApplyError::Rejected)?
    {
        counts.unchanged += 1;
        session.record_durable(seq);
        return Ok(());
    }

    if !context.options.durability.succeeds(seq) {
        return Err(ApplyError::Durability(format!(
            "the durable write for record {seq} did not complete, so the cursor stays where it was"
        )));
    }

    // 3. Install the replacement atomically under the existing Note ID, and
    // 4. remove a superseded filename only after the replacement exists.
    let write = match &existing {
        Some(found) => replace_note(context.notebook, &candidate, &found.filename),
        None => write_note(context.notebook, &candidate),
    }
    .map_err(|error| ApplyError::Durability(error.to_string()))?;
    if write.removed_previous.is_some() {
        counts.renamed += 1;
    }
    if existing.is_some() {
        counts.updated += 1;
    } else {
        counts.created += 1;
    }
    context.index.insert(
        &context.config.id,
        &scope,
        &identity,
        IndexedNote {
            path: write.path.clone(),
            filename: write.filename.clone(),
            note_id,
            record: candidate.record().clone(),
        },
    );

    // 5. Only now is the record durable, which is what makes a covering
    // checkpoint eligible.
    session.record_durable(seq);
    Ok(())
}

/// Whether the notebook already holds this record's current state.
///
/// Compared through A1 section 8's semantic fingerprint, which excludes the
/// bookkeeping properties (`captured_at`, `content_hash`, `id`, `instance_id`,
/// `field_id`, `collected_by`, `source_version`, and the rebuildable
/// projections). The source version is compared separately, and so is the
/// filename, so a replay of the same object rewrites nothing at all while a real
/// change — including a source version that moved on its own — does.
fn unchanged(existing: &IndexedNote, candidate: &CanonicalRecord) -> Result<bool, Rejection> {
    let fingerprint = |record: &ParsedRecord| -> Result<String, Rejection> {
        semantic_record_string(record)
            .map(|text| record_fingerprint(&text))
            .map_err(|error| {
                Rejection::new(
                    RejectionCode::ProtocolSchemaInvalid,
                    format!("a Note could not be fingerprinted for comparison: {error}"),
                )
            })
    };
    if fingerprint(&existing.record)? != fingerprint(candidate.record())? {
        return Ok(false);
    }
    if text_of(&existing.record, "source_version") != text_of(candidate.record(), "source_version")
    {
        return Ok(false);
    }
    let expected = candidate.note_filename().map_err(|error| {
        Rejection::new(
            RejectionCode::ProtocolSchemaInvalid,
            format!("the projected Note has no canonical filename: {error}"),
        )
    })?;
    Ok(expected == existing.filename)
}

fn text_of(record: &ParsedRecord, key: &str) -> Option<String> {
    match record.get(key) {
        Some(fieldnotes_domain::Value::Scalar(fieldnotes_domain::Scalar::Text(text))) => {
            Some(text.clone())
        }
        _ => None,
    }
}

/// Installs every resolved artifact durably and projects the references.
fn install_artifacts<C: Clock, R: RandomSource>(
    context: &mut CollectContext<'_, C, R>,
    record: &RecordEvent,
    outcomes: &[ArtifactOutcome],
    counts: &mut SyncCounts,
    digests: &mut StoredDigests,
) -> Result<ArtifactProjection, ApplyError> {
    let mut projection = ArtifactProjection::default();
    let references: &[ArtifactRef] = match &record.artifacts {
        Some(references) => references,
        None => &[],
    };
    for (reference, outcome) in references.iter().zip(outcomes.iter()) {
        match outcome {
            ArtifactOutcome::Declined(declined) => {
                projection.skipped.push(SkippedArtifact {
                    attachment_ref: declined.attachment_ref.clone(),
                    source_filename: declined.source_filename.clone(),
                });
            }
            ArtifactOutcome::Resolved(resolved) => {
                let media_type = reference
                    .media_type
                    .as_ref()
                    .map(|declared| media_type_essence(declared.as_str()));
                let stored = if resolved.reused && reference.kind == ArtifactKind::DigestOnly {
                    reuse_stored(context.notebook, &resolved.digest, media_type.as_deref())?
                } else {
                    install_staged(
                        context.notebook,
                        context.staging,
                        reference,
                        &resolved.digest,
                        media_type.as_deref(),
                        counts,
                    )?
                };
                digests.0.insert(resolved.digest.clone());
                projection.retained.push(RetainedArtifact {
                    artifact_id: stored.0,
                    relative_path: stored.1,
                    role: reference.role,
                    source_filename: reference.source_filename.clone(),
                });
            }
        }
    }
    Ok(projection)
}

/// Locates an artifact core already stores, for a `digest_only` reference.
fn reuse_stored(
    notebook: &Notebook,
    digest: &str,
    media_type: Option<&str>,
) -> Result<(String, String), ApplyError> {
    let id = fieldnotes_domain::ArtifactId::parse(&format!(
        "{}{digest}",
        fieldnotes_domain::ArtifactId::PREFIX
    ))
    .map_err(|error| {
        ApplyError::Rejected(Rejection::new(
            RejectionCode::ArtifactUnknownDigest,
            format!("`{digest}` is not an artifact digest: {error}"),
        ))
    })?;
    match find_artifact(notebook, &id, media_type)
        .map_err(|error| ApplyError::Durability(error.to_string()))?
    {
        Some(path) => Ok((id.to_string(), notebook.relative_display(&path))),
        None => Err(ApplyError::Rejected(Rejection::new(
            RejectionCode::ArtifactUnknownDigest,
            format!(
                "no artifact with digest {digest} is stored, and core will not create a Note \
                 referencing bytes it does not hold"
            ),
        ))),
    }
}

/// Installs staged bytes as a content-addressed original.
fn install_staged(
    notebook: &Notebook,
    staging: &Path,
    reference: &ArtifactRef,
    digest: &str,
    media_type: Option<&str>,
    counts: &mut SyncCounts,
) -> Result<(String, String), ApplyError> {
    let handle = reference
        .parsed_handle()
        .map_err(|error| ApplyError::Rejected(Rejection::new(error.code, error.message)))?;
    let path = handle.resolve_in(staging);
    let bytes = std::fs::read(&path).map_err(|error| {
        ApplyError::Rejected(Rejection::new(
            RejectionCode::ArtifactMissingStagedFile,
            format!("staged bytes for handle {handle} could not be read: {error}"),
        ))
    })?;
    // The canonical extension comes from A1's media-type registry, never from
    // `source_filename`. A Field's declared type is upstream metadata and is
    // preferred; content sniffing is the fallback for a Field that declared
    // none.
    let detected = media_type.map_or_else(|| detect_media_type(&bytes), Some);
    let stored = store_artifact(notebook, &bytes, detected)
        .map_err(|error| ApplyError::Durability(error.to_string()))?;
    // The notebook path is derived from core's own digest. If the staged file
    // changed between the protocol crate's verified read and this one, the two
    // digests disagree and the record is rejected rather than stored under an
    // identity that does not describe its bytes.
    if stored.id.to_string() != format!("{}{digest}", fieldnotes_domain::ArtifactId::PREFIX) {
        return Err(ApplyError::Rejected(Rejection::new(
            RejectionCode::ArtifactDigestMismatch,
            format!(
                "the staged file for handle {handle} hashed to {} on the installing read but \
                 {digest} on the verifying read",
                stored.id
            ),
        )));
    }
    if stored.reused {
        counts.artifacts_reused += 1;
    } else {
        counts.artifacts_stored += 1;
    }
    Ok((stored.id.to_string(), stored.relative_path))
}

/// Commits one checkpoint offer, or reports why it was withheld.
fn commit_checkpoint(
    notebook: &Notebook,
    field_id: &str,
    session: &mut CollectSession<'_>,
    offer: &CheckpointOffer,
    started_at: Datetime,
) -> Result<Option<StoredCursor>, String> {
    match session.commit(offer) {
        Ok(cursor) => {
            let stored = StoredCursor {
                cursor: cursor.cursor.as_str().to_owned(),
                cursor_format_version: cursor.cursor_format_version,
                covers_record_seq_through: cursor.covers_record_seq_through,
                committed_at: started_at.to_string(),
            };
            // The cursor is the last durable write of a checkpoint, and the
            // only one whose failure is safe.
            match write_cursor(notebook, field_id, &stored) {
                Ok(()) => Ok(Some(stored)),
                Err(error) => Err(format!(
                    "the cursor could not be recorded, so it stays where it was: {error}"
                )),
            }
        }
        Err(refusal) => {
            let reason = refusal.to_string();
            let _ = session.withhold(offer);
            Err(reason)
        }
    }
}

/// Removes Notes a completed authoritative snapshot proved absent.
fn remove_absent_notes<C: Clock, R: RandomSource>(
    context: &mut CollectContext<'_, C, R>,
    scope: &str,
    reported: &BTreeSet<(String, String)>,
) -> Result<u64, Refusal> {
    let absent: Vec<(String, String)> = context
        .index
        .keys_in_scope(&context.config.id, scope)
        .into_iter()
        .filter(|((_, note_scope, identity), _)| {
            !reported.contains(&(note_scope.clone(), identity.clone()))
        })
        .map(|((_, note_scope, identity), _)| (note_scope.clone(), identity.clone()))
        .collect();
    let mut removed = 0;
    for (note_scope, identity) in absent {
        let Some(note) = context
            .index
            .get(&context.config.id, &note_scope, &identity)
            .cloned()
        else {
            continue;
        };
        // No tombstone Note and no revision entry: A1 section 7 keeps no
        // deletion ledger, so a later refetch recreates the Note under a new ID.
        remove_note(context.notebook, &note.filename)?;
        context
            .index
            .remove(&context.config.id, &note_scope, &identity);
        removed += 1;
    }
    Ok(removed)
}

/// Builds the reserved status file's contents.
fn status_file(report: &FieldSyncReport, finished_at: Datetime) -> LastSyncOutcome {
    let mut extra = serde_json::Map::new();
    extra.insert("mode".to_owned(), serde_json::json!(report.mode));
    extra.insert(
        "records_accepted".to_owned(),
        serde_json::json!(report.counts.records_accepted),
    );
    extra.insert(
        "created".to_owned(),
        serde_json::json!(report.counts.created),
    );
    extra.insert(
        "updated".to_owned(),
        serde_json::json!(report.counts.updated),
    );
    extra.insert(
        "unchanged".to_owned(),
        serde_json::json!(report.counts.unchanged),
    );
    extra.insert(
        "removed_by_tombstone".to_owned(),
        serde_json::json!(report.counts.removed_by_tombstone),
    );
    extra.insert(
        "removed_by_snapshot".to_owned(),
        serde_json::json!(report.counts.removed_by_snapshot),
    );
    extra.insert(
        "artifacts_stored".to_owned(),
        serde_json::json!(report.counts.artifacts_stored),
    );
    extra.insert(
        "artifacts_reused".to_owned(),
        serde_json::json!(report.counts.artifacts_reused),
    );
    extra.insert(
        "attachments_skipped".to_owned(),
        serde_json::json!(report.counts.attachments_skipped),
    );
    extra.insert(
        "durable_write_failures".to_owned(),
        serde_json::json!(report.counts.durable_write_failures),
    );
    extra.insert(
        "cursor_committed".to_owned(),
        serde_json::json!(report.cursor_committed),
    );
    extra.insert(
        "cursor_coverage".to_owned(),
        serde_json::json!(report.cursor_coverage),
    );
    extra.insert(
        "cursor_recovery_gap".to_owned(),
        serde_json::json!(report.cursor_recovery_gap),
    );
    extra.insert("exit".to_owned(), serde_json::json!(report.exit));
    extra.insert(
        "deletion_authorized_scope".to_owned(),
        serde_json::json!(report.deletion.authorized_scope),
    );
    extra.insert(
        "deletion_refusals".to_owned(),
        serde_json::json!(report.deletion.refusals),
    );
    if let Some(rejection) = &report.rejection {
        extra.insert(
            "rejection".to_owned(),
            serde_json::json!({ "code": rejection.code, "detail": rejection.detail }),
        );
    }
    if let Some(failure) = &report.failure {
        extra.insert("failure".to_owned(), serde_json::json!(failure));
    }
    if let Some(credential) = &report.credential {
        // The profile name and the counts, and nothing else: this file is
        // operational state a user can read, so it holds no material and
        // nothing that could become material.
        extra.insert(
            "credential".to_owned(),
            serde_json::json!({
                "profile": credential.profile,
                "provider": credential.provider,
                "scopes": credential.scopes,
                "requests": credential.requests,
                "granted": credential.granted,
                "refused": credential.refused,
            }),
        );
    }
    LastSyncOutcome {
        outcome: report.outcome.as_str().to_owned(),
        at: finished_at.to_string(),
        extra,
    }
}
