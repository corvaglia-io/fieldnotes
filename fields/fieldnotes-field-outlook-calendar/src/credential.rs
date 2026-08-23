//! Obtaining a Graph access token over the protected credential channel A2
//! section 12 describes.
//!
//! Core never puts credential material in `config`, on the command line, or
//! anywhere else this Field can see except this one channel: `collect_request`
//! carries only a [`CredentialGrant`] *reference* -- a `profile_ref`, a
//! single-use `grant_id`, an `expires_at`, and a
//! [`ChannelDescriptor`](fieldnotes_field_protocol::message::ChannelDescriptor) naming
//! how to reach core for the actual secret. This module is the Field-side
//! half of that exchange: it opens the described channel, writes one
//! `credential_request` frame, and reads back the one `credential_response`
//! frame core answers with, exactly as A2 specifies the exchange (the same
//! newline-delimited-JSON shape as standard input and output, just on a
//! different file descriptor).
//!
//! # Only `unix_socket_path` is implemented here
//!
//! [`ChannelKind`] admits exactly two kinds: `unix_socket_path` and
//! `windows_named_pipe`. It used to admit two more, `inherited_fd` and
//! `duplicated_handle`, both of which require wrapping a raw OS handle
//! ([`std::os::fd::FromRawFd`] or the Windows equivalent) -- `unsafe`, and
//! `unsafe_code` is `forbid`-level in this workspace's lints, a level no
//! crate, including this one, can locally override. ADR 0013 removed both
//! variants from [`ChannelKind`] entirely rather than leaving them to be
//! refused here at run time, so a grant naming either is now
//! unrepresentable, not merely rejected. Do not re-add them without first
//! resolving that same `unsafe` contradiction.
//!
//! `windows_named_pipe` needs no `unsafe` either -- its client end is just
//! `std::fs::OpenOptions::open` against the pipe's path, as the Outlook Mail
//! Field's channel module demonstrates -- this crate simply has not built
//! that client yet. This Field refuses every kind it has not implemented
//! cleanly, with a diagnostic naming which kind it cannot open, rather than
//! attempting one it has not built.

use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

use fieldnotes_field_protocol::grammar::{CredentialRequestTag, ProtocolV1};
use fieldnotes_field_protocol::message::{
    ChannelKind, CollectRequest, CredentialGrant, CredentialOutcome, CredentialPurpose,
    CredentialRequest, CredentialResponse, MaterialKind, Validate,
};
use fieldnotes_msgraph::AccessToken;

/// The most response bytes this Field reads off the protected channel before
/// giving up. Bounded because a channel core described is still a stream of
/// bytes from a process this Field trusts less than its own logic.
const MAX_RESPONSE_BYTES: u64 = 131_072;

/// The channel round-trip's read/write timeout.
const CHANNEL_TIMEOUT: Duration = Duration::from_secs(20);

/// Why this Field could not obtain an access token.
#[derive(Debug)]
pub(crate) enum CredentialFailure {
    /// `collect_request.credential` was absent, though this Field's manifest
    /// declares `auth.kind: oauth_authorization_code`.
    NoGrant,
    /// The described channel kind is not implemented by this Field build
    /// (see the module documentation).
    UnsupportedChannel(ChannelKind),
    /// The channel descriptor itself was malformed for its declared kind.
    MalformedChannel(String),
    /// Opening or using the channel failed at the transport level.
    Io(String),
    /// The response did not decode or validate as a `credential_response`.
    Malformed(String),
    /// Core denied the request.
    Denied(Option<String>),
    /// The grant had already expired.
    Expired(Option<String>),
    /// Core does not recognize this grant.
    UnknownGrant(Option<String>),
    /// Core cannot obtain material right now.
    Unavailable(Option<String>),
    /// Core granted material of a kind this Field cannot use against Graph.
    UnexpectedMaterialKind,
}

impl fmt::Display for CredentialFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialFailure::NoGrant => write!(
                f,
                "collect_request carried no credential grant, though this Field's manifest \
                 declares auth.kind oauth_authorization_code"
            ),
            CredentialFailure::UnsupportedChannel(kind) => write!(
                f,
                "the described credential channel ({}) is not implemented by this Field; only \
                 a unix_socket_path channel on a Unix host is currently supported here",
                channel_kind_label(*kind)
            ),
            CredentialFailure::MalformedChannel(reason) => {
                write!(f, "the credential channel descriptor is unusable: {reason}")
            }
            CredentialFailure::Io(reason) => {
                write!(f, "the credential channel could not be used: {reason}")
            }
            CredentialFailure::Malformed(reason) => {
                write!(f, "the credential response did not validate: {reason}")
            }
            CredentialFailure::Denied(message) => {
                write!(f, "core denied the credential request{}", suffix(message))
            }
            CredentialFailure::Expired(message) => {
                write!(f, "the credential grant expired{}", suffix(message))
            }
            CredentialFailure::UnknownGrant(message) => {
                write!(f, "core does not recognize this grant{}", suffix(message))
            }
            CredentialFailure::Unavailable(message) => {
                write!(
                    f,
                    "core cannot obtain material right now{}",
                    suffix(message)
                )
            }
            CredentialFailure::UnexpectedMaterialKind => write!(
                f,
                "core granted credential material of a kind this Field cannot present to Graph \
                 as a bearer token"
            ),
        }
    }
}

fn suffix(message: &Option<String>) -> String {
    message
        .as_deref()
        .map_or_else(String::new, |text| format!(": {text}"))
}

fn channel_kind_label(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::UnixSocketPath => "unix_socket_path",
        ChannelKind::WindowsNamedPipe => "windows_named_pipe",
    }
}

/// Obtains a Graph access token for `request`, over the channel its
/// `credential` grant describes.
pub(crate) fn obtain(request: &CollectRequest) -> Result<AccessToken, CredentialFailure> {
    let Some(grant) = &request.credential else {
        return Err(CredentialFailure::NoGrant);
    };
    match grant.channel.kind {
        ChannelKind::UnixSocketPath => obtain_over_unix_socket(request, grant),
        other => Err(CredentialFailure::UnsupportedChannel(other)),
    }
}

#[cfg(unix)]
fn obtain_over_unix_socket(
    request: &CollectRequest,
    grant: &CredentialGrant,
) -> Result<AccessToken, CredentialFailure> {
    use std::os::unix::net::UnixStream;

    let path = grant.channel.path.as_deref().ok_or_else(|| {
        CredentialFailure::MalformedChannel("a unix_socket_path channel names its path".to_owned())
    })?;
    let mut stream =
        UnixStream::connect(path).map_err(|error| CredentialFailure::Io(error.to_string()))?;
    stream
        .set_read_timeout(Some(CHANNEL_TIMEOUT))
        .map_err(|error| CredentialFailure::Io(error.to_string()))?;
    stream
        .set_write_timeout(Some(CHANNEL_TIMEOUT))
        .map_err(|error| CredentialFailure::Io(error.to_string()))?;

    let credential_request = CredentialRequest {
        v: ProtocolV1,
        frame_type: CredentialRequestTag,
        run_id: request.run_id.clone(),
        grant_id: grant.grant_id.clone(),
        purpose: CredentialPurpose::AccessToken,
        scopes: Some(vec![crate::constants::GRAPH_SCOPE.to_owned()]),
    };
    let mut line = serde_json::to_vec(&credential_request)
        .map_err(|error| CredentialFailure::Malformed(error.to_string()))?;
    line.push(b'\n');
    stream
        .write_all(&line)
        .map_err(|error| CredentialFailure::Io(error.to_string()))?;
    stream
        .flush()
        .map_err(|error| CredentialFailure::Io(error.to_string()))?;

    let mut reader = BufReader::new(Read::take(stream, MAX_RESPONSE_BYTES));
    let mut response_line = String::new();
    let bytes_read = reader
        .read_line(&mut response_line)
        .map_err(|error| CredentialFailure::Io(error.to_string()))?;
    if bytes_read == 0 {
        return Err(CredentialFailure::Io(
            "the channel closed before a response arrived".to_owned(),
        ));
    }

    decode_response(response_line.trim_end())
}

#[cfg(not(unix))]
fn obtain_over_unix_socket(
    _request: &CollectRequest,
    _grant: &CredentialGrant,
) -> Result<AccessToken, CredentialFailure> {
    Err(CredentialFailure::UnsupportedChannel(
        ChannelKind::UnixSocketPath,
    ))
}

fn decode_response(text: &str) -> Result<AccessToken, CredentialFailure> {
    let response: CredentialResponse = serde_json::from_str(text)
        .map_err(|error| CredentialFailure::Malformed(error.to_string()))?;
    response
        .validate()
        .map_err(|error| CredentialFailure::Malformed(error.message))?;
    match response.outcome {
        CredentialOutcome::Granted => {
            // `validate()` above already guarantees `material` is present for
            // a granted outcome.
            let Some(material) = response.material else {
                return Err(CredentialFailure::Malformed(
                    "a granted response carries no material despite validating".to_owned(),
                ));
            };
            if material.kind != MaterialKind::BearerToken {
                return Err(CredentialFailure::UnexpectedMaterialKind);
            }
            Ok(AccessToken::new(material.value))
        }
        CredentialOutcome::Denied => Err(CredentialFailure::Denied(
            response.message.map(|text| text.to_string()),
        )),
        CredentialOutcome::Expired => Err(CredentialFailure::Expired(
            response.message.map(|text| text.to_string()),
        )),
        CredentialOutcome::UnknownGrant => Err(CredentialFailure::UnknownGrant(
            response.message.map(|text| text.to_string()),
        )),
        CredentialOutcome::Unavailable => Err(CredentialFailure::Unavailable(
            response.message.map(|text| text.to_string()),
        )),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{CredentialFailure, decode_response, obtain};
    use fieldnotes_field_protocol::grammar::{
        CollectRequestTag, FieldIdToken, GrantId, OffsetDatetime, ProfileRef, ProtocolV1, RunId,
    };
    use fieldnotes_field_protocol::limits::{Deadline, Limits, default_artifact_media_types};
    use fieldnotes_field_protocol::message::{
        ChannelDescriptor, ChannelKind, CollectRequest, CollectionMode, CredentialGrant,
    };
    use fieldnotes_field_protocol::value::ConfigMap;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    fn run_id() -> RunId {
        RunId::parse("1a4c9f2e-0000-4000-8000-000000000002")
            .unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    fn base_request(credential: Option<CredentialGrant>) -> CollectRequest {
        CollectRequest {
            v: ProtocolV1,
            frame_type: CollectRequestTag,
            run_id: run_id(),
            protocol_version: ProtocolV1,
            protocol_revision: 0,
            field_id: FieldIdToken::parse("outlook_calendar_work")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            mode: CollectionMode::Incremental,
            cursor: None,
            cursor_format_version: None,
            window: None,
            snapshot_scope: None,
            config: ConfigMap::new(),
            credential,
            artifact_staging_dir: "/tmp/staging".to_owned(),
            limits: Limits::ceilings(),
            deadline: Deadline {
                not_after: OffsetDatetime::parse("2099-01-01T00:00:00+00:00")
                    .unwrap_or_else(|error| panic!("must parse: {error}")),
                idle_seconds: Deadline::DEFAULT_IDLE_SECONDS,
                cancel_grace_seconds: Deadline::DEFAULT_CANCEL_GRACE_SECONDS,
            },
            artifact_media_types: default_artifact_media_types(),
            recollect_targets: None,
        }
    }

    fn grant_over(path: &str) -> CredentialGrant {
        CredentialGrant {
            profile_ref: ProfileRef::parse("outlook_calendar_work")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            grant_id: GrantId::parse("abcdef0123456789")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            channel: ChannelDescriptor {
                kind: ChannelKind::UnixSocketPath,
                path: Some(path.to_owned()),
            },
            expires_at: OffsetDatetime::parse("2099-01-01T00:00:00+00:00")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            scopes: Some(vec!["Calendars.Read".to_owned()]),
        }
    }

    /// A short-as-possible unique socket path directly under `/tmp`: a Unix
    /// domain socket path is bound by `sockaddr_un`'s tiny fixed buffer
    /// (108 bytes on Linux, 104 on macOS), which a path built from
    /// [`fieldnotes_test_support::TempDir`]'s own longer, more descriptive
    /// naming convention can easily exceed once `$TMPDIR` is a deep macOS
    /// per-user path.
    fn short_socket_path() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::path::PathBuf::from(format!("/tmp/fn-cal-{}-{unique}.sock", std::process::id()))
    }

    /// Runs a one-shot fake "core" that answers a single credential exchange
    /// with `response_json`, on a fresh temporary socket path.
    fn fake_core(response_json: serde_json::Value) -> (SocketGuard, thread::JoinHandle<()>) {
        let socket_path = short_socket_path();
        let listener =
            UnixListener::bind(&socket_path).unwrap_or_else(|error| panic!("bind: {error}"));
        let guard = SocketGuard(socket_path.clone());
        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(
                stream
                    .try_clone()
                    .unwrap_or_else(|error| panic!("clone: {error}")),
            );
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let mut bytes = serde_json::to_vec(&response_json).unwrap_or_default();
            bytes.push(b'\n');
            let _ = stream.write_all(&bytes);
        });
        (guard, handle)
    }

    /// Removes the bound socket file when a test case ends, successfully or
    /// not, and exposes the path for the test to connect to.
    struct SocketGuard(std::path::PathBuf);

    impl SocketGuard {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for SocketGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn no_grant_is_reported_plainly() {
        let request = base_request(None);
        assert!(matches!(obtain(&request), Err(CredentialFailure::NoGrant)));
    }

    #[test]
    fn a_granted_bearer_token_round_trips_over_a_real_unix_socket() {
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
        let (guard, handle) = fake_core(response);
        let request = base_request(Some(grant_over(
            guard.path().to_str().unwrap_or_else(|| panic!("utf8 path")),
        )));
        let token = obtain(&request).unwrap_or_else(|error| panic!("must obtain: {error}"));
        assert_eq!(format!("{token:?}"), "AccessToken(REDACTED)");
        handle.join().unwrap_or_else(|_| panic!("thread must join"));
    }

    #[test]
    fn a_denied_outcome_is_classified_distinctly() {
        let response = serde_json::json!({
            "v": 1,
            "type": "credential_response",
            "run_id": "1a4c9f2e-0000-4000-8000-000000000002",
            "grant_id": "abcdef0123456789",
            "outcome": "denied",
            "message": "an administrator has not granted Calendars.Read"
        });
        let (guard, handle) = fake_core(response);
        let request = base_request(Some(grant_over(
            guard.path().to_str().unwrap_or_else(|| panic!("utf8 path")),
        )));
        match obtain(&request) {
            Err(CredentialFailure::Denied(Some(message))) => {
                assert!(message.contains("Calendars.Read"));
            }
            other => panic!("expected a denied outcome, got {other:?}"),
        }
        handle.join().unwrap_or_else(|_| panic!("thread must join"));
    }

    // The former `an_inherited_fd_channel_is_refused_without_unsafe_code` test
    // is gone, not merely weakened: it exercised `ChannelKind::InheritedFd`,
    // which ADR 0013 removed from the type entirely. There is no longer a
    // grant to construct that names that kind. The still-real "this Field
    // has not implemented that kind" refusal is exercised below for
    // `windows_named_pipe`, the one remaining kind this crate does not open.
    #[test]
    fn a_windows_named_pipe_channel_is_refused_as_not_yet_implemented_here() {
        let request = base_request(Some(CredentialGrant {
            profile_ref: ProfileRef::parse("outlook_calendar_work")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            grant_id: GrantId::parse("abcdef0123456789")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            channel: ChannelDescriptor {
                kind: ChannelKind::WindowsNamedPipe,
                path: Some("\\\\.\\pipe\\fieldnotes-fixture".to_owned()),
            },
            expires_at: OffsetDatetime::parse("2099-01-01T00:00:00+00:00")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            scopes: None,
        }));
        assert!(matches!(
            obtain(&request),
            Err(CredentialFailure::UnsupportedChannel(
                ChannelKind::WindowsNamedPipe
            ))
        ));
    }

    #[test]
    fn a_malformed_response_body_is_reported_as_malformed_not_a_panic() {
        assert!(matches!(
            decode_response("not json at all"),
            Err(CredentialFailure::Malformed(_))
        ));
    }
}
