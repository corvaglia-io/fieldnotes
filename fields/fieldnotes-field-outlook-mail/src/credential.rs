//! Obtaining the access token on the protected credential channel.
//!
//! A2 section 12 puts credential material on **one** channel, separate from
//! standard input, output, and error, named in the collection request rather
//! than in the environment. The Field writes one `credential_request` naming
//! its grant and purpose; core answers with one `credential_response` carrying
//! either `material` or an actionable non-granted outcome.
//!
//! # What this module never does
//!
//! It never logs the material, never puts it in a diagnostic, never puts it in
//! a cursor, never writes it to a file, and never returns it as a bare
//! `String`: the granted value is moved directly into
//! [`fieldnotes_msgraph::AccessToken`], whose `Debug` implementation prints
//! `AccessToken(REDACTED)` and which has no `Display` at all, so a stray
//! interpolation cannot compile into a leak. The transport additionally
//! registers the value with its own redactor before the first request, so even
//! a Graph error body that echoed the token back is scrubbed.
//!
//! # Why only the socket and pipe channel kinds are implemented here
//!
//! A2 admits four channel mechanisms. `unix_socket_path` and
//! `windows_named_pipe` are reachable from safe Rust
//! ([`std::os::unix::net::UnixStream::connect`] and
//! [`std::fs::OpenOptions::open`] respectively). `inherited_fd` and
//! `duplicated_handle` are not: turning a raw descriptor or handle into an I/O
//! object requires `unsafe`, which this workspace forbids outright
//! (`unsafe_code = "forbid"`), or a platform crate this Field is not the right
//! place to introduce. A2 section 12's own consequences say this belongs in
//! the shared SDK -- "every Field needs channel-handling code, so the shared
//! SDK crate must provide it rather than leaving each connector to reimplement
//! it" -- so the two descriptor-passing mechanisms are refused here with an
//! actionable diagnostic instead of being half-implemented. See the crate's
//! final report.

use std::io::{BufReader, Read, Write};

use fieldnotes_field_protocol::framing::{FrameReader, FrameWriter};
use fieldnotes_field_protocol::grammar::{CredentialRequestTag, ProtocolV1};
use fieldnotes_field_protocol::message::{
    ChannelKind, CredentialFrame, CredentialGrant, CredentialOutcome, CredentialPurpose,
    CredentialRequest, MaterialKind,
};
use fieldnotes_msgraph::AccessToken;

/// The longest `credential_response` frame this Field will read.
///
/// Bounded because the channel's other end is still an external process from
/// this Field's point of view, and an unbounded read is an unbounded
/// allocation.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Why no usable access token was obtained.
#[derive(Debug)]
pub(crate) enum CredentialError {
    /// The collection request carried no credential grant, although this
    /// Field's manifest declares that it requires one.
    NoGrant,
    /// The channel mechanism the grant names is not implemented by this build.
    UnsupportedChannel(ChannelKind),
    /// The channel could not be opened, written to, or read from.
    Channel(String),
    /// Core answered, but not with material.
    NotGranted {
        /// Core's outcome.
        outcome: CredentialOutcome,
        /// Core's own actionable, secret-free explanation, when it sent one.
        message: Option<String>,
    },
    /// The response did not belong to this run or this grant.
    Mismatched,
    /// The response was not a well-formed `credential_response`.
    Malformed(String),
    /// The material was not a bearer token, which is the only kind this Field
    /// can present to Microsoft Graph.
    WrongMaterial(MaterialKind),
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::NoGrant => f.write_str(
                "this Field reads Microsoft Graph and needs an access token, but the collection \
                 request carried no credential grant; configure a credential profile for this \
                 Field",
            ),
            CredentialError::UnsupportedChannel(kind) => write!(
                f,
                "the credential channel mechanism {kind:?} is not implemented by this build; it \
                 needs descriptor-passing support that belongs in the shared Field SDK. A \
                 socket- or pipe-based channel works today."
            ),
            CredentialError::Channel(reason) => {
                write!(f, "the protected credential channel failed: {reason}")
            }
            CredentialError::NotGranted { outcome, message } => {
                write!(f, "core did not grant credential material ({outcome:?})")?;
                match message {
                    Some(message) => write!(f, ": {message}"),
                    None => Ok(()),
                }
            }
            CredentialError::Mismatched => f.write_str(
                "the credential response named a different run or grant than the one requested, \
                 so it was refused",
            ),
            CredentialError::Malformed(reason) => {
                write!(f, "the credential response did not validate: {reason}")
            }
            CredentialError::WrongMaterial(kind) => write!(
                f,
                "core granted {kind:?} material, but Microsoft Graph accepts only a bearer token"
            ),
        }
    }
}

impl std::error::Error for CredentialError {}

impl CredentialError {
    /// The diagnostic code and exit code this failure reports as.
    pub(crate) fn classify(
        &self,
    ) -> (
        fieldnotes_field_protocol::codes::DiagnosticCode,
        fieldnotes_field_protocol::codes::ExitCode,
    ) {
        use fieldnotes_field_protocol::codes::{DiagnosticCode, ExitCode};
        match self {
            CredentialError::NoGrant | CredentialError::UnsupportedChannel(_) => {
                (DiagnosticCode::ConfigInvalid, ExitCode::ConfigInvalid)
            }
            CredentialError::NotGranted {
                outcome: CredentialOutcome::Denied,
                ..
            } => (DiagnosticCode::PermissionDenied, ExitCode::Authorization),
            _ => (DiagnosticCode::AuthReauthRequired, ExitCode::Authentication),
        }
    }
}

/// One duplex byte channel: the protected channel's two directions.
///
/// Boxed as `Read + Write` so the socket and pipe mechanisms share one
/// exchange implementation, and so the exchange itself is written once against
/// the protocol's own [`FrameReader`]/[`FrameWriter`] rather than against a
/// hand-rolled line protocol.
type Channel = Box<dyn ReadWrite>;

/// The duplex I/O a protected channel needs.
trait ReadWrite: Read + Write {}

impl<T: Read + Write> ReadWrite for T {}

/// Opens the channel the grant names.
fn open(grant: &CredentialGrant) -> Result<Channel, CredentialError> {
    match grant.channel.kind {
        #[cfg(unix)]
        ChannelKind::UnixSocketPath => {
            let path = grant
                .channel
                .path
                .as_deref()
                .ok_or(CredentialError::UnsupportedChannel(grant.channel.kind))?;
            let stream = std::os::unix::net::UnixStream::connect(path)
                .map_err(|error| CredentialError::Channel(error.to_string()))?;
            Ok(Box::new(stream))
        }
        ChannelKind::WindowsNamedPipe => {
            let path = grant
                .channel
                .path
                .as_deref()
                .ok_or(CredentialError::UnsupportedChannel(grant.channel.kind))?;
            let pipe = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|error| CredentialError::Channel(error.to_string()))?;
            Ok(Box::new(pipe))
        }
        kind => Err(CredentialError::UnsupportedChannel(kind)),
    }
}

/// Builds the one request frame this Field writes on the channel.
fn request_frame(
    run_id: &fieldnotes_field_protocol::grammar::RunId,
    grant: &CredentialGrant,
) -> CredentialRequest {
    CredentialRequest {
        v: ProtocolV1,
        frame_type: CredentialRequestTag,
        run_id: run_id.clone(),
        grant_id: grant.grant_id.clone(),
        purpose: CredentialPurpose::AccessToken,
        scopes: Some(vec![crate::constants::GRAPH_SCOPE.to_owned()]),
    }
}

/// Reads one `credential_response` frame from `text` and extracts the bearer
/// token.
///
/// Split out from the channel I/O so the frame handling -- including every
/// non-granted outcome and every mismatch -- is testable without a socket.
/// Decoding goes through [`CredentialFrame::decode`], so the schema check core
/// applies on its side of the channel is the same one applied here.
fn token_from_response(
    text: &str,
    run_id: &fieldnotes_field_protocol::grammar::RunId,
    grant: &CredentialGrant,
) -> Result<AccessToken, CredentialError> {
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|error| CredentialError::Malformed(error.to_string()))?;
    let response = match CredentialFrame::decode(value) {
        Ok(CredentialFrame::Response(response)) => *response,
        Ok(CredentialFrame::Request(_)) => {
            return Err(CredentialError::Malformed(
                "core answered with a credential_request, not a response".to_owned(),
            ));
        }
        Err(error) => return Err(CredentialError::Malformed(error.message)),
    };
    if response.run_id.as_str() != run_id.as_str()
        || response.grant_id.as_str() != grant.grant_id.as_str()
    {
        return Err(CredentialError::Mismatched);
    }
    if response.outcome != CredentialOutcome::Granted {
        return Err(CredentialError::NotGranted {
            outcome: response.outcome,
            message: response
                .message
                .as_ref()
                .map(|message| message.as_str().to_owned()),
        });
    }
    let material = response.material.ok_or(CredentialError::Malformed(
        "a granted response carries material".to_owned(),
    ))?;
    if material.kind != MaterialKind::BearerToken {
        return Err(CredentialError::WrongMaterial(material.kind));
    }
    // The only place the secret is touched: moved straight into a type that
    // cannot be printed.
    Ok(AccessToken::new(material.value))
}

/// Obtains this run's access token on the protected channel.
pub(crate) fn acquire(
    run_id: &fieldnotes_field_protocol::grammar::RunId,
    grant: Option<&CredentialGrant>,
) -> Result<AccessToken, CredentialError> {
    let grant = grant.ok_or(CredentialError::NoGrant)?;
    let channel = open(grant)?;
    let mut reader = BufReader::new(channel);
    let bound = u64::try_from(MAX_RESPONSE_BYTES).unwrap_or(u64::MAX);
    let request = CredentialFrame::Request(Box::new(request_frame(run_id, grant)));
    {
        let mut writer = FrameWriter::new(reader.get_mut(), bound);
        writer
            .write_credential_frame(&request)
            .map_err(|error| CredentialError::Channel(error.to_string()))?;
    }
    let mut frames = FrameReader::new(&mut reader, bound, bound);
    let raw = match frames.next_frame() {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            return Err(CredentialError::Channel(
                "the credential channel closed before core answered".to_owned(),
            ));
        }
        Err(error) => return Err(CredentialError::Channel(error.to_string())),
    };
    let text = serde_json::to_string(&raw.value)
        .map_err(|error| CredentialError::Malformed(error.to_string()))?;
    token_from_response(&text, run_id, grant)
}

#[cfg(test)]
mod tests {
    use super::{CredentialError, request_frame, token_from_response};
    use fieldnotes_field_protocol::grammar::{GrantId, OffsetDatetime, ProfileRef, RunId};
    use fieldnotes_field_protocol::message::{
        ChannelDescriptor, ChannelKind, CredentialGrant, CredentialOutcome, Validate as _,
    };

    const GRANT: &str = "0123456789abcdef0123456789abcdef";
    const RUN: &str = "1a4c9f2e-0000-4000-8000-000000000002";
    const TOKEN_CANARY: &str = "FIXTURE-NOT-A-REAL-TOKEN-canary-channel-9f31";

    fn run_id() -> RunId {
        RunId::parse(RUN).unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    fn grant(kind: ChannelKind) -> CredentialGrant {
        CredentialGrant {
            profile_ref: ProfileRef::parse("outlook_mail_work")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            grant_id: GrantId::parse(GRANT).unwrap_or_else(|error| panic!("must parse: {error}")),
            channel: ChannelDescriptor {
                kind,
                fd: matches!(kind, ChannelKind::InheritedFd).then_some(7),
                handle: None,
                path: matches!(
                    kind,
                    ChannelKind::UnixSocketPath | ChannelKind::WindowsNamedPipe
                )
                .then(|| "/tmp/fieldnotes-fixture.sock".to_owned()),
            },
            expires_at: OffsetDatetime::parse("2099-01-01T00:00:00+00:00")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
            scopes: Some(vec!["Mail.Read".to_owned()]),
        }
    }

    fn granted_frame(token: &str) -> String {
        serde_json::json!({
            "v": 1,
            "type": "credential_response",
            "run_id": RUN,
            "grant_id": GRANT,
            "outcome": "granted",
            "material": { "kind": "bearer_token", "value": token }
        })
        .to_string()
    }

    #[test]
    fn the_request_frame_names_only_the_grant_the_purpose_and_the_scope() {
        let frame = request_frame(&run_id(), &grant(ChannelKind::UnixSocketPath));
        if let Err(error) = frame.validate() {
            panic!("the request must validate: {error:?}");
        }
        let json =
            serde_json::to_string(&frame).unwrap_or_else(|error| panic!("must serialize: {error}"));
        assert!(json.contains("\"purpose\":\"access_token\""));
        assert!(json.contains("Mail.Read"));
        assert!(
            !json.contains("value"),
            "a request never carries material: {json}"
        );
    }

    #[test]
    fn a_granted_bearer_token_never_prints_itself() {
        let token = token_from_response(
            &granted_frame(TOKEN_CANARY),
            &run_id(),
            &grant(ChannelKind::UnixSocketPath),
        )
        .unwrap_or_else(|error| panic!("must grant: {error}"));
        let printed = format!("{token:?}");
        assert_eq!(printed, "AccessToken(REDACTED)");
        assert!(!printed.contains("canary"));
    }

    #[test]
    fn a_denied_outcome_is_actionable_and_carries_no_material() {
        let frame = serde_json::json!({
            "v": 1, "type": "credential_response", "run_id": RUN, "grant_id": GRANT,
            "outcome": "denied",
            "message": "this profile is not authorized for Mail.Read"
        })
        .to_string();
        match token_from_response(&frame, &run_id(), &grant(ChannelKind::UnixSocketPath)) {
            Err(CredentialError::NotGranted { outcome, message }) => {
                assert_eq!(outcome, CredentialOutcome::Denied);
                assert_eq!(
                    message.as_deref(),
                    Some("this profile is not authorized for Mail.Read")
                );
            }
            other => panic!("a denial must be reported as one, got {other:?}"),
        }
    }

    #[test]
    fn an_expired_grant_is_reported_as_needing_re_authentication() {
        let frame = serde_json::json!({
            "v": 1, "type": "credential_response", "run_id": RUN, "grant_id": GRANT,
            "outcome": "expired", "message": "the grant for this run has expired"
        })
        .to_string();
        let error =
            match token_from_response(&frame, &run_id(), &grant(ChannelKind::UnixSocketPath)) {
                Err(error) => error,
                Ok(_) => panic!("an expired grant must not yield a token"),
            };
        assert_eq!(
            error.classify().1,
            fieldnotes_field_protocol::codes::ExitCode::Authentication
        );
    }

    #[test]
    fn a_response_for_another_run_or_grant_is_refused() {
        for frame in [
            serde_json::json!({
                "v": 1, "type": "credential_response",
                "run_id": "1a4c9f2e-0000-4000-8000-0000000000ff", "grant_id": GRANT,
                "outcome": "granted",
                "material": { "kind": "bearer_token", "value": TOKEN_CANARY }
            }),
            serde_json::json!({
                "v": 1, "type": "credential_response", "run_id": RUN,
                "grant_id": "ffffffffffffffffffffffffffffffff",
                "outcome": "granted",
                "material": { "kind": "bearer_token", "value": TOKEN_CANARY }
            }),
        ] {
            match token_from_response(
                &frame.to_string(),
                &run_id(),
                &grant(ChannelKind::UnixSocketPath),
            ) {
                Err(CredentialError::Mismatched) => {}
                other => panic!("a mismatched response must be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn material_that_is_not_a_bearer_token_is_refused() {
        let frame = serde_json::json!({
            "v": 1, "type": "credential_response", "run_id": RUN, "grant_id": GRANT,
            "outcome": "granted",
            "material": { "kind": "basic", "value": TOKEN_CANARY, "username": "sam" }
        })
        .to_string();
        assert!(matches!(
            token_from_response(&frame, &run_id(), &grant(ChannelKind::UnixSocketPath)),
            Err(CredentialError::WrongMaterial(_))
        ));
    }

    #[test]
    fn a_malformed_response_is_refused_rather_than_partially_read() {
        for frame in ["not json at all", "{\"v\":1}", "{}"] {
            assert!(matches!(
                token_from_response(frame, &run_id(), &grant(ChannelKind::UnixSocketPath)),
                Err(CredentialError::Malformed(_))
            ));
        }
    }

    /// The whole exchange over a real Unix domain socket, which is the channel
    /// mechanism core's own `0.1.3` implementation binds.
    ///
    /// This is the only test that runs the request-write and response-read
    /// path end to end, and it asserts both halves of the invariant: the token
    /// arrives, and the request this Field wrote carries no material at all.
    #[cfg(unix)]
    #[test]
    fn a_socket_channel_exchange_yields_the_token_and_writes_no_material() -> std::io::Result<()> {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        // A short path, in the system temp directory rather than a nested
        // per-test one: a Unix domain socket address is bounded by `SUN_LEN`
        // (104 bytes on macOS), which a descriptive nested path exceeds.
        let socket_path = std::env::temp_dir().join(format!("fnom{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)?;

        let served = std::thread::spawn(move || -> std::io::Result<String> {
            let (mut stream, _address) = listener.accept()?;
            let mut reader = BufReader::new(stream.try_clone()?);
            let mut request_line = String::new();
            reader.read_line(&mut request_line)?;
            stream.write_all(granted_frame(TOKEN_CANARY).as_bytes())?;
            stream.write_all(b"\n")?;
            stream.flush()?;
            Ok(request_line)
        });

        let mut grant = grant(ChannelKind::UnixSocketPath);
        grant.channel.path = Some(socket_path.display().to_string());
        let token = super::acquire(&run_id(), Some(&grant))
            .unwrap_or_else(|error| panic!("the exchange must grant a token: {error}"));
        assert_eq!(format!("{token:?}"), "AccessToken(REDACTED)");

        let request_line = match served.join() {
            Ok(result) => result?,
            Err(_) => panic!("the serving thread panicked"),
        };
        assert!(
            request_line.contains("credential_request"),
            "this Field must write exactly one credential_request: {request_line}"
        );
        assert!(
            !request_line.contains("canary") && !request_line.contains("value"),
            "a credential request carries a reference, never material: {request_line}"
        );
        let _ = std::fs::remove_file(&socket_path);
        Ok(())
    }

    #[test]
    fn a_descriptor_passing_channel_is_refused_actionably_rather_than_half_implemented() {
        let error = match super::open(&grant(ChannelKind::InheritedFd)) {
            Err(error) => error,
            Ok(_) => panic!("an inherited descriptor is not implemented by this build"),
        };
        assert!(matches!(error, CredentialError::UnsupportedChannel(_)));
        let message = error.to_string();
        assert!(message.contains("shared Field SDK"), "{message}");
        assert_eq!(
            error.classify().1,
            fieldnotes_field_protocol::codes::ExitCode::ConfigInvalid
        );
    }

    #[test]
    fn a_missing_grant_is_reported_as_a_configuration_problem() {
        let error = match super::acquire(&run_id(), None) {
            Err(error) => error,
            Ok(_) => panic!("no grant can yield no token"),
        };
        assert!(matches!(error, CredentialError::NoGrant));
        assert!(error.to_string().contains("credential profile"));
    }

    #[test]
    fn no_credential_error_message_can_carry_material() {
        let errors = [
            CredentialError::NoGrant,
            CredentialError::UnsupportedChannel(ChannelKind::InheritedFd),
            CredentialError::Channel("connection refused".to_owned()),
            CredentialError::Mismatched,
            CredentialError::Malformed("expected value".to_owned()),
            CredentialError::WrongMaterial(fieldnotes_field_protocol::message::MaterialKind::Basic),
            CredentialError::NotGranted {
                outcome: CredentialOutcome::Unavailable,
                message: Some("core cannot reach the credential store".to_owned()),
            },
        ];
        for error in errors {
            let rendered = format!("{error} {error:?}");
            assert!(
                !rendered.contains("canary") && !rendered.contains(TOKEN_CANARY),
                "{rendered}"
            );
        }
    }
}
