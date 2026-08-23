//! The `0.1.3` credential integration, end to end, with no tenant, no network,
//! no browser, and no keychain.
//!
//! Four things are proved here, because they are the four things the
//! authentication gate turns on:
//!
//! 1. **A missing credential fails a run before anything happens.** No child
//!    process, no staging directory, no cursor.
//! 2. **An expired or revoked credential says what to run**, rather than
//!    producing a stack trace, and equally advances no cursor.
//! 3. **The protected channel delivers the access token and only the access
//!    token.** A unique canary is granted over a real per-platform channel and
//!    is then asserted absent from everything else a run produces — A2 section
//!    12 assigns this end-to-end canary to this gate by name.
//! 4. **`fields status` answers "is this Field authenticated?"** without
//!    attempting a sync.
//!
//! # How this stays hermetic
//!
//! The seams are the ones `fieldnotes-credentials` already provides. An access
//! token is minted through [`exchange_code`] against a **fake
//! [`TokenTransport`]**, which is how a canary value gets into a real
//! [`AccessToken`] without a tenant. A run's token source is an injected
//! [`AccessTokenSource`] that returns that token or a chosen failure, so the
//! keychain is never touched. The one test that would need a real keychain
//! (proving `KeyringCredentialProvider` round-trips) already lives in
//! `fieldnotes-credentials`, gated behind an explicit opt-in variable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fieldnotes_app::credentials::channel::{GrantSpec, ProtectedChannel};
use fieldnotes_app::credentials::{
    AccessTokenSource, CredentialFailure, CredentialInspector, CredentialSettings,
    CredentialSource, CredentialState, PROFILE_KEY, PROVIDER_KEY, settings_from_config,
};
use fieldnotes_app::{
    AppError, FieldRunOutcome, Kernel, SyncOptions, add_field, field_status, field_status_with,
    init, sync, validate_field_id,
};
use fieldnotes_credentials::oauth::token::exchange_code;
use fieldnotes_credentials::oauth::{
    AccessToken, TokenHttpResponse, TokenTransport, TransportError,
};
use fieldnotes_field_protocol::grammar::{
    CredentialRequestTag, GrantId, OffsetDatetime, ProfileRef, ProtocolV1, RunId,
};
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{
    CredentialFrame, CredentialOutcome, CredentialPurpose, CredentialRequest,
};
use fieldnotes_store::{Notebook, cursor_exists, staging_dir};
use fieldnotes_test_support::{CountingRandom, FixedClock, TempDir};

/// 2026-08-22T08:45:00+02:00 in Unix milliseconds.
const FIXED_MILLIS: u64 = 1_787_381_100_000;

/// The one secret value in this file. Unique enough to scan for.
const CANARY: &str = "FIELDNOTES-CANARY-ACCESS-TOKEN-93b1f0a7c2d4";

/// A temporary directory, or a panic naming why not: a test harness failure is
/// not one of the outcomes these tests are about.
fn temp(label: &str) -> TempDir {
    match TempDir::new(label) {
        Ok(temp) => temp,
        Err(error) => panic!("could not create a temporary directory: {error}"),
    }
}

fn kernel() -> Result<Kernel<FixedClock, CountingRandom>, AppError> {
    Kernel::new(FixedClock(FIXED_MILLIS), CountingRandom::new(1), 120)
}

/// A transport that answers every token request with the same scripted body,
/// so no network call is ever made.
struct ScriptedTransport {
    body: String,
}

impl TokenTransport for ScriptedTransport {
    fn post_form(
        &self,
        _endpoint: &str,
        _form: &[(&str, &str)],
    ) -> Result<TokenHttpResponse, TransportError> {
        Ok(TokenHttpResponse {
            status: 200,
            body: self.body.clone(),
        })
    }
}

/// Mints a real [`AccessToken`] carrying [`CANARY`], through the credential
/// crate's own exchange path and a fake transport.
fn canary_token(expires_in_seconds: i64) -> AccessToken {
    let transport = ScriptedTransport {
        body: format!(
            r#"{{"access_token":"{CANARY}","refresh_token":"RT","expires_in":{expires_in_seconds}}}"#
        ),
    };
    let clock = FixedClock(FIXED_MILLIS);
    match exchange_code(
        &transport,
        &clock,
        "https://example.invalid/token",
        "client-id",
        "code",
        "http://127.0.0.1:1/callback",
        "verifier",
    ) {
        Ok(set) => set.access_token,
        Err(error) => panic!("the fake exchange must succeed: {error}"),
    }
}

/// An [`AccessTokenSource`] that answers with a fixed token or a fixed failure.
struct StubTokens {
    answer: Result<AccessToken, CredentialFailure>,
}

impl StubTokens {
    fn granting(token: AccessToken) -> Self {
        StubTokens { answer: Ok(token) }
    }

    fn failing(failure: CredentialFailure) -> Self {
        StubTokens {
            answer: Err(failure),
        }
    }
}

impl AccessTokenSource for StubTokens {
    fn mint(
        &self,
        _field_id: &str,
        _settings: &CredentialSettings,
    ) -> Result<AccessToken, CredentialFailure> {
        self.answer.clone()
    }
}

/// A [`CredentialInspector`] with a fixed answer, so status never reads a real
/// credential store.
struct StubInspector(CredentialState);

impl CredentialInspector for StubInspector {
    fn state(&self, _settings: &CredentialSettings) -> CredentialState {
        self.0.clone()
    }
}

fn credential_config() -> BTreeMap<String, String> {
    let mut config = BTreeMap::new();
    config.insert(PROFILE_KEY.to_owned(), "work".to_owned());
    config
}

/// An absolute path holding no executable. A run that reaches the spawn fails
/// visibly on it; a run that refuses earlier never touches it, which is
/// precisely what these tests distinguish.
fn unspawnable(temp: &TempDir) -> PathBuf {
    temp.path().join("no-such-field-binary")
}

/// A notebook with one Outlook Mail Field configured to authenticate.
fn notebook_with_authenticating_field(
    temp: &TempDir,
) -> Result<(Notebook, Kernel<FixedClock, CountingRandom>), AppError> {
    let root = temp.path().join("notebook");
    let mut kernel = kernel()?;
    init(&mut kernel, &root, Some("credential-tests"))?;
    let notebook = Notebook::open(&root)?;
    add_field(
        &notebook,
        &validate_field_id("outlook_mail", "work")?,
        unspawnable(temp),
        credential_config(),
        true,
    )?;
    Ok((notebook, kernel))
}

fn options(tokens: StubTokens) -> SyncOptions {
    SyncOptions {
        credentials: CredentialSource::Injected(Arc::new(tokens)),
        ..SyncOptions::default()
    }
}

/// Every path a run could have left something at, when it should have left
/// nothing at all.
fn nothing_happened(notebook: &Notebook, field_id: &str) {
    assert!(
        !cursor_exists(notebook, field_id),
        "a credential failure must not commit a cursor"
    );
    let staging = staging_dir(notebook, field_id, "any");
    let per_field = staging.parent().unwrap_or(Path::new("."));
    assert!(
        !per_field.exists(),
        "a credential failure must not create a staging directory: {}",
        per_field.display()
    );
    let notes = std::fs::read_dir(notebook.notes_dir())
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(notes, 0, "a credential failure must write no Note");
}

#[test]
fn a_missing_credential_fails_before_anything_is_spawned() -> Result<(), AppError> {
    let temp = temp("credentials-absent");
    let (notebook, mut kernel) = notebook_with_authenticating_field(&temp)?;

    let outcome = sync(
        &mut kernel,
        &notebook,
        Some("outlook_mail_work"),
        &options(StubTokens::failing(CredentialFailure::NotAuthenticated {
            field_id: "outlook_mail_work".to_owned(),
            profile: "work".to_owned(),
        })),
    )?;

    let report = outcome
        .fields
        .first()
        .unwrap_or_else(|| panic!("one Field was synced"));
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    let failure = report
        .failure
        .as_deref()
        .unwrap_or_else(|| panic!("a refusal carries its reason"));
    assert!(
        failure.contains("fieldnotes fields auth outlook_mail_work"),
        "the refusal must say what to run: {failure}"
    );
    // The Field's executable does not exist, so reaching the spawn would have
    // produced a different message entirely. This is how "before anything is
    // spawned" is observable.
    assert!(
        !failure.contains("cannot start"),
        "the run must refuse before the spawn: {failure}"
    );
    assert!(!report.cursor_committed);
    assert_eq!(report.credential, None);
    nothing_happened(&notebook, "outlook_mail_work");
    Ok(())
}

#[test]
fn an_expired_or_revoked_credential_is_an_instruction_not_a_stack_trace() -> Result<(), AppError> {
    for failure in [
        CredentialFailure::Expired {
            field_id: "outlook_mail_work".to_owned(),
            profile: "work".to_owned(),
        },
        CredentialFailure::Revoked {
            field_id: "outlook_mail_work".to_owned(),
            profile: "work".to_owned(),
        },
    ] {
        let temp = temp("credentials-stale");
        let (notebook, mut kernel) = notebook_with_authenticating_field(&temp)?;
        let outcome = sync(
            &mut kernel,
            &notebook,
            Some("outlook_mail_work"),
            &options(StubTokens::failing(failure.clone())),
        )?;
        let report = outcome
            .fields
            .first()
            .unwrap_or_else(|| panic!("one Field was synced"));
        assert_eq!(report.outcome, FieldRunOutcome::Failed);
        let text = report.failure.clone().unwrap_or_default();
        assert!(
            text.contains("fieldnotes fields auth outlook_mail_work"),
            "{text}"
        );
        assert!(!report.cursor_committed);
        nothing_happened(&notebook, "outlook_mail_work");
    }
    Ok(())
}

#[test]
fn a_run_with_no_credential_source_refuses_rather_than_reaching_for_a_keychain()
-> Result<(), AppError> {
    let temp = temp("credentials-no-source");
    let (notebook, mut kernel) = notebook_with_authenticating_field(&temp)?;
    // The library default: a `sync` nobody wired a credential source into.
    let outcome = sync(
        &mut kernel,
        &notebook,
        Some("outlook_mail_work"),
        &SyncOptions::default(),
    )?;
    let report = outcome
        .fields
        .first()
        .unwrap_or_else(|| panic!("one Field was synced"));
    assert_eq!(report.outcome, FieldRunOutcome::Failed);
    assert!(
        report
            .failure
            .clone()
            .unwrap_or_default()
            .contains("without a credential source")
    );
    nothing_happened(&notebook, "outlook_mail_work");
    Ok(())
}

#[test]
fn a_field_needing_no_credential_is_unaffected_by_credential_wiring() -> Result<(), AppError> {
    let temp = temp("credentials-irrelevant");
    let root = temp.path().join("notebook");
    let mut kernel = kernel()?;
    init(&mut kernel, &root, Some("credential-tests"))?;
    let notebook = Notebook::open(&root)?;
    add_field(
        &notebook,
        &validate_field_id("local", "work")?,
        unspawnable(&temp),
        BTreeMap::new(),
        true,
    )?;
    // No credential source, no profile: the run proceeds to the spawn and
    // fails there, which is the pre-`0.1.3` behavior, unchanged.
    let outcome = sync(
        &mut kernel,
        &notebook,
        Some("local_work"),
        &SyncOptions::default(),
    )?;
    let report = outcome
        .fields
        .first()
        .unwrap_or_else(|| panic!("one Field was synced"));
    let failure = report.failure.clone().unwrap_or_default();
    assert!(
        failure.contains("cannot start") || failure.contains("cannot run"),
        "a Field needing no credential still reaches its spawn: {failure}"
    );
    assert_eq!(report.credential, None);
    Ok(())
}

#[test]
fn a_successful_mint_happens_before_the_spawn_and_lets_the_run_continue() -> Result<(), AppError> {
    let temp = temp("credentials-minted");
    let (notebook, mut kernel) = notebook_with_authenticating_field(&temp)?;
    let outcome = sync(
        &mut kernel,
        &notebook,
        Some("outlook_mail_work"),
        &options(StubTokens::granting(canary_token(3600))),
    )?;
    let report = outcome
        .fields
        .first()
        .unwrap_or_else(|| panic!("one Field was synced"));
    // The mint succeeded, so the run proceeded — and then failed at the one
    // place it now can: starting the executable that is not there. A run that
    // minted nothing would have failed with a credential message instead.
    let failure = report.failure.clone().unwrap_or_default();
    assert!(
        failure.contains("cannot start") || failure.contains("cannot run"),
        "a minted credential must let the run reach its spawn: {failure}"
    );
    assert!(
        !failure.contains(CANARY),
        "no failure message may carry material: {failure}"
    );
    // `describe` failed, so no manifest declared authentication and no channel
    // was ever opened.
    assert_eq!(report.credential, None);
    nothing_happened(&notebook, "outlook_mail_work");
    Ok(())
}

#[test]
fn fields_status_answers_whether_a_field_is_authenticated() -> Result<(), AppError> {
    let temp = temp("credentials-status");
    let (notebook, _kernel) = notebook_with_authenticating_field(&temp)?;

    let stored = field_status_with(
        &notebook,
        Some("outlook_mail_work"),
        &StubInspector(CredentialState::Stored),
    )?;
    let report = &stored[0];
    assert_eq!(report.credential_profile.as_deref(), Some("work"));
    assert_eq!(report.credential_provider.as_deref(), Some("keychain"));
    assert_eq!(report.credential_state, CredentialState::Stored);

    let absent = field_status_with(
        &notebook,
        Some("outlook_mail_work"),
        &StubInspector(CredentialState::Absent),
    )?;
    assert_eq!(absent[0].credential_state, CredentialState::Absent);
    assert_eq!(absent[0].credential_state.as_str(), "absent");

    // An unavailable store is distinguished from an absent credential: one is
    // "not authenticated", the other is "could not be asked".
    let unavailable = field_status_with(
        &notebook,
        Some("outlook_mail_work"),
        &StubInspector(CredentialState::Unavailable(
            "keychain is locked".to_owned(),
        )),
    )?;
    assert_eq!(unavailable[0].credential_state.as_str(), "unavailable");

    // The default entry point probes nothing at all, so a caller that does not
    // want to touch a credential store does not.
    let unprobed = field_status(&notebook, Some("outlook_mail_work"))?;
    assert_eq!(
        unprobed[0].credential_state,
        CredentialState::NotConfigured,
        "the unprobed entry point must not claim a credential is stored"
    );

    // `self` needs no credential and says so.
    let built_in = field_status(&notebook, Some("self"))?;
    assert_eq!(built_in[0].credential_state, CredentialState::NotRequired);
    assert_eq!(built_in[0].credential_profile, None);
    Ok(())
}

#[test]
fn a_field_with_no_profile_configured_reports_not_required_until_a_manifest_says_otherwise()
-> Result<(), AppError> {
    let temp = temp("credentials-status-unconfigured");
    let root = temp.path().join("notebook");
    let mut kernel = kernel()?;
    init(&mut kernel, &root, Some("credential-tests"))?;
    let notebook = Notebook::open(&root)?;
    add_field(
        &notebook,
        &validate_field_id("local", "work")?,
        unspawnable(&temp),
        BTreeMap::new(),
        true,
    )?;
    let reports = field_status(&notebook, Some("local_work"))?;
    assert_eq!(reports[0].credential_state, CredentialState::NotRequired);
    Ok(())
}

#[test]
fn configuring_a_credential_override_without_a_profile_is_refused_at_configuration_time()
-> Result<(), AppError> {
    let temp = temp("credentials-config-guard");
    let root = temp.path().join("notebook");
    let mut kernel = kernel()?;
    init(&mut kernel, &root, Some("credential-tests"))?;
    let notebook = Notebook::open(&root)?;
    let mut config = BTreeMap::new();
    config.insert("oauth_tenant_id".to_owned(), "organizations".to_owned());
    let refused = add_field(
        &notebook,
        &validate_field_id("outlook_mail", "typo")?,
        unspawnable(&temp),
        config,
        true,
    );
    assert!(matches!(refused, Err(AppError::Credential(_))));

    // An invalid profile name is refused too, rather than surfacing at the
    // first sync.
    let mut config = BTreeMap::new();
    config.insert(PROFILE_KEY.to_owned(), "Not A Profile".to_owned());
    assert!(matches!(
        add_field(
            &notebook,
            &validate_field_id("outlook_mail", "bad")?,
            unspawnable(&temp),
            config,
            true,
        ),
        Err(AppError::Credential(_))
    ));

    // And an unknown provider, rather than silently defaulting to one.
    let mut config = BTreeMap::new();
    config.insert(PROFILE_KEY.to_owned(), "work".to_owned());
    config.insert(PROVIDER_KEY.to_owned(), "plaintext_file".to_owned());
    assert!(matches!(
        add_field(
            &notebook,
            &validate_field_id("outlook_mail", "file")?,
            unspawnable(&temp),
            config,
            true,
        ),
        Err(AppError::Credential(_))
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// The protected channel, over a real per-platform endpoint
// ---------------------------------------------------------------------------

fn settings() -> CredentialSettings {
    match settings_from_config("outlook_mail_work", &credential_config()) {
        Ok(settings) => settings,
        Err(error) => panic!("fixture configuration must resolve: {error}"),
    }
}

fn grant_spec(token: AccessToken, run: &str, grant: &str) -> GrantSpec {
    GrantSpec {
        run_id: match RunId::parse(run) {
            Ok(id) => id,
            Err(error) => panic!("fixture run identifier must parse: {error}"),
        },
        profile: match ProfileRef::parse(settings().profile.as_str()) {
            Ok(profile) => profile,
            Err(error) => panic!("fixture profile must parse: {error}"),
        },
        grant_id: match GrantId::parse(grant) {
            Ok(id) => id,
            Err(error) => panic!("fixture grant identifier must parse: {error}"),
        },
        scopes: vec!["Mail.Read".to_owned()],
        expires_at: match OffsetDatetime::parse("2026-08-22T09:45:00+02:00") {
            Ok(instant) => instant,
            Err(error) => panic!("fixture expiry must parse: {error}"),
        },
        lifetime: Duration::from_secs(60),
        token,
        max_frame_bytes: Limits::defaults().max_frame_bytes,
    }
}

const RUN: &str = "1a4c9f2e-0000-4000-8000-00000000c1a0";
/// One grant identifier per test. A real one is generated per run and is
/// therefore unique; these are distinct for the same reason, since the endpoint
/// a grant is served on is named after it.
const GRANT_DELIVERY: &str = "9f14c0a3b7e25d689f14c0a3b7e25d68";
const GRANT_REFUSAL: &str = "1122334455667788112233445566aabb";
const GRANT_TEARDOWN: &str = "ccddeeff00112233ccddeeff00112233";

fn request(
    grant: &str,
    purpose: CredentialPurpose,
    scopes: Option<Vec<String>>,
) -> CredentialFrame {
    CredentialFrame::Request(Box::new(CredentialRequest {
        v: ProtocolV1,
        frame_type: CredentialRequestTag,
        run_id: match RunId::parse(RUN) {
            Ok(id) => id,
            Err(error) => panic!("fixture run identifier must parse: {error}"),
        },
        grant_id: match GrantId::parse(grant) {
            Ok(id) => id,
            Err(error) => panic!("fixture grant identifier must parse: {error}"),
        },
        purpose,
        scopes,
    }))
}

/// Connects to the channel as a Field would, exchanges one frame pair, and
/// returns the response.
///
/// The client side is plain standard library: this is exactly the code a Field
/// (or the Field SDK) needs, and proving it needs nothing else is part of the
/// point.
#[cfg(unix)]
fn ask(channel: &ProtectedChannel, frame: &CredentialFrame) -> CredentialFrame {
    use fieldnotes_field_protocol::framing::{FrameReader, FrameWriter};
    use std::io::BufReader;
    use std::os::unix::net::UnixStream;

    let path = channel
        .grant()
        .channel
        .path
        .clone()
        .unwrap_or_else(|| panic!("a unix-socket channel names its path"));
    let stream = match UnixStream::connect(&path) {
        Ok(stream) => stream,
        Err(error) => panic!("a Field must be able to connect to {path}: {error}"),
    };
    let max = Limits::defaults().max_frame_bytes;
    if let Err(error) = FrameWriter::new(&stream, max).write_credential_frame(frame) {
        panic!("writing a credential request must succeed: {error}");
    }
    let read_half = match stream.try_clone() {
        Ok(half) => half,
        Err(error) => panic!("cloning the channel stream must succeed: {error}"),
    };
    match FrameReader::new(BufReader::new(read_half), max, max * 8).next_credential_frame() {
        Ok(Some(frame)) => frame,
        Ok(None) => panic!("core must answer a credential request"),
        Err(error) => panic!("reading the credential response must succeed: {error}"),
    }
}

#[cfg(unix)]
#[test]
fn the_protected_channel_delivers_the_access_token_and_nothing_else() {
    let channel = match ProtectedChannel::open(grant_spec(canary_token(3600), RUN, GRANT_DELIVERY))
    {
        Ok(channel) => channel,
        Err(error) => panic!("the channel must open: {error}"),
    };

    // The grant is a reference: the collection request carries this, and it
    // must not contain the canary anywhere.
    let grant_json = match serde_json::to_string(channel.grant()) {
        Ok(text) => text,
        Err(error) => panic!("the grant must serialize: {error}"),
    };
    assert!(
        !grant_json.contains(CANARY),
        "the credential grant must carry no material: {grant_json}"
    );
    assert!(grant_json.contains("unix_socket_path"));
    assert!(
        !format!("{channel:?}").contains(CANARY),
        "the channel's own Debug output must carry no material"
    );

    match ask(
        &channel,
        &request(GRANT_DELIVERY, CredentialPurpose::AccessToken, None),
    ) {
        CredentialFrame::Response(response) => {
            assert_eq!(response.outcome, CredentialOutcome::Granted);
            let material = response
                .material
                .as_ref()
                .unwrap_or_else(|| panic!("a granted response carries material"));
            assert_eq!(material.value, CANARY);
            assert_eq!(
                material.scopes.as_deref(),
                Some(["Mail.Read".to_owned()].as_slice())
            );
            // The protocol's own redacting Debug is the second layer.
            assert!(!format!("{material:?}").contains(CANARY));
        }
        other => panic!("core must answer with a response frame: {other:?}"),
    }
    assert_eq!(channel.counts().granted, 1);
    assert_eq!(channel.counts().refused, 0);
}

#[cfg(unix)]
#[test]
fn the_channel_refuses_a_grant_it_did_not_issue_and_a_scope_it_did_not_grant() {
    let channel = match ProtectedChannel::open(grant_spec(canary_token(3600), RUN, GRANT_REFUSAL)) {
        Ok(channel) => channel,
        Err(error) => panic!("the channel must open: {error}"),
    };
    let other_grant = "0000000000000000ffffffffffffffff";

    for (frame, expected) in [
        (
            request(other_grant, CredentialPurpose::AccessToken, None),
            CredentialOutcome::UnknownGrant,
        ),
        (
            request(
                GRANT_REFUSAL,
                CredentialPurpose::AccessToken,
                Some(vec!["Mail.ReadWrite".to_owned()]),
            ),
            CredentialOutcome::Denied,
        ),
        (
            request(GRANT_REFUSAL, CredentialPurpose::BasicCredentials, None),
            CredentialOutcome::Denied,
        ),
    ] {
        match ask(&channel, &frame) {
            CredentialFrame::Response(response) => {
                assert_eq!(response.outcome, expected);
                assert!(
                    response.material.is_none(),
                    "a refusal must carry no material"
                );
                let message = response
                    .message
                    .as_ref()
                    .unwrap_or_else(|| panic!("a refusal says what to do about it"))
                    .as_str()
                    .to_owned();
                assert!(!message.contains(CANARY), "{message}");
            }
            other => panic!("core must answer with a response frame: {other:?}"),
        }
    }
    assert_eq!(channel.counts().granted, 0);
    assert_eq!(channel.counts().refused, 3);
}

/// A world-reachable endpoint that hands out an access token is the kind of
/// defect that passes every functional test, so the permissions are asserted
/// rather than assumed.
#[cfg(unix)]
#[test]
fn the_endpoint_is_reachable_only_by_this_user() {
    use std::os::unix::fs::PermissionsExt;

    let channel = match ProtectedChannel::open(grant_spec(
        canary_token(3600),
        RUN,
        "aaaabbbbccccddddaaaabbbbccccdddd",
    )) {
        Ok(channel) => channel,
        Err(error) => panic!("the channel must open: {error}"),
    };
    let socket = PathBuf::from(
        channel
            .grant()
            .channel
            .path
            .clone()
            .unwrap_or_else(|| panic!("a unix-socket channel names its path")),
    );
    let directory = socket
        .parent()
        .unwrap_or_else(|| panic!("the socket lives in a directory"));

    let mode = |path: &Path| match std::fs::metadata(path) {
        Ok(metadata) => metadata.permissions().mode() & 0o777,
        Err(error) => panic!("could not read {}: {error}", path.display()),
    };
    assert_eq!(
        mode(directory),
        0o700,
        "the endpoint directory must be reachable only by this user"
    );
    assert_eq!(
        mode(&socket),
        0o600,
        "the socket itself must be reachable only by this user"
    );
}

#[cfg(unix)]
#[test]
fn closing_the_channel_removes_the_endpoint() {
    let mut channel =
        match ProtectedChannel::open(grant_spec(canary_token(3600), RUN, GRANT_TEARDOWN)) {
            Ok(channel) => channel,
            Err(error) => panic!("the channel must open: {error}"),
        };
    let path = PathBuf::from(
        channel
            .grant()
            .channel
            .path
            .clone()
            .unwrap_or_else(|| panic!("a unix-socket channel names its path")),
    );
    assert!(path.exists(), "the endpoint exists while the run does");
    channel.close();
    assert!(
        !path.exists(),
        "the endpoint must not outlive the run: {}",
        path.display()
    );
    // Closing twice is safe, and so is the drop that follows.
    channel.close();
}
