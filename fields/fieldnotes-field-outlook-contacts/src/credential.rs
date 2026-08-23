//! Obtaining an access token on the protected credential channel (A2 section
//! 12).
//!
//! This module never acquires, refreshes, or stores anything beyond one
//! run's material: it opens exactly the channel core named in the grant,
//! asks for exactly the scopes the manifest declared, and hands the answer
//! straight to the caller. It reuses the protocol library's own framing
//! (`FrameWriter::write_credential_frame`, `FrameReader::next_frame`, and
//! `CredentialFrame::decode`) rather than hand-rolling NDJSON a second time;
//! only the channel *transport* -- turning a [`ChannelDescriptor`] into a
//! duplex byte stream -- is this crate's own, because nothing in the
//! workspace provides it yet (see this crate's final report).
//!
//! # What this release can and cannot open
//!
//! [`ChannelKind::UnixSocketPath`] and [`ChannelKind::WindowsNamedPipe`] are
//! addressed by path and connected to with a safe standard-library call
//! (`UnixStream::connect`, or opening the pipe path as a file on Windows).
//! [`ChannelKind::InheritedFd`] and [`ChannelKind::DuplicatedHandle`] name a
//! raw OS descriptor number, and importing one safely needs
//! `std::os::fd::FromRawFd` or `std::os::windows::io::FromRawHandle` --
//! both `unsafe fn`, in a crate where `unsafe_code` is `forbid` workspace-wide.
//! This release therefore returns an actionable
//! [`CredentialChannelError::UnsupportedChannelKind`] for those two kinds
//! rather than reaching for `unsafe`; see the final report for why this is a
//! shared-layer gap rather than something to solve per Field.

use std::io::{BufReader, Read, Write};

use fieldnotes_field_protocol::framing::{FrameReader, FrameWriter};
use fieldnotes_field_protocol::grammar::{CredentialRequestTag, RunId};
use fieldnotes_field_protocol::message::{
    ChannelDescriptor, ChannelKind, CredentialFrame, CredentialMaterial, CredentialOutcome,
    CredentialPurpose, CredentialRequest, CredentialResponse,
};

/// The largest single credential-channel frame this Field will read or
/// write. `CredentialMaterial::value` alone may be up to 65,536 bytes
/// (A2's own bound), so the frame ceiling is set comfortably above that
/// rather than reusing the protocol's general 1 MiB frame ceiling, which
/// would be needlessly generous for a channel that carries nothing but one
/// small request and one small response.
const MAX_CREDENTIAL_FRAME_BYTES: u64 = 131_072;

/// Why obtaining credential material failed.
#[derive(Debug)]
pub(crate) enum CredentialChannelError {
    /// This release cannot open a channel of this kind. Never `unsafe`; see
    /// the module documentation.
    UnsupportedChannelKind(ChannelKind),
    /// The channel could not be opened or read/written.
    Io(std::io::Error),
    /// The frame exchange itself failed (malformed, oversized, or the wrong
    /// frame type arrived).
    Protocol(String),
    /// Core answered, but declined to grant material.
    NotGranted {
        outcome: CredentialOutcome,
        message: Option<String>,
    },
}

impl std::fmt::Display for CredentialChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialChannelError::UnsupportedChannelKind(kind) => write!(
                f,
                "this release cannot open a {kind:?} protected channel; a path-based channel \
                 (a Unix domain socket or a Windows named pipe) is required"
            ),
            CredentialChannelError::Io(error) => write!(f, "protected channel I/O failed: {error}"),
            CredentialChannelError::Protocol(detail) => {
                write!(f, "protected channel exchange failed: {detail}")
            }
            CredentialChannelError::NotGranted { outcome, message } => write!(
                f,
                "credential material was not granted ({outcome:?}){}",
                message
                    .as_deref()
                    .map(|message| format!(": {message}"))
                    .unwrap_or_default()
            ),
        }
    }
}

impl std::error::Error for CredentialChannelError {}

/// A duplex byte channel: a reader half and a writer half of the same
/// underlying connection.
pub(crate) struct Channel<R, W> {
    pub(crate) reader: R,
    pub(crate) writer: W,
}

/// A boxed, type-erased duplex channel, since [`open`] returns one of several
/// concrete stream types depending on the descriptor's [`ChannelKind`].
type BoxedChannel = Channel<Box<dyn Read>, Box<dyn Write>>;

/// Opens the channel a [`ChannelDescriptor`] names.
pub(crate) fn open(descriptor: &ChannelDescriptor) -> Result<BoxedChannel, CredentialChannelError> {
    match descriptor.kind {
        #[cfg(unix)]
        ChannelKind::UnixSocketPath => {
            let path = descriptor.path.as_deref().ok_or_else(|| {
                CredentialChannelError::Protocol(
                    "a Unix-socket-path channel must name a path".to_owned(),
                )
            })?;
            let stream = std::os::unix::net::UnixStream::connect(path)
                .map_err(CredentialChannelError::Io)?;
            let writer = stream.try_clone().map_err(CredentialChannelError::Io)?;
            Ok(Channel {
                reader: Box::new(stream),
                writer: Box::new(writer),
            })
        }
        #[cfg(not(unix))]
        ChannelKind::UnixSocketPath => Err(CredentialChannelError::UnsupportedChannelKind(
            descriptor.kind,
        )),
        ChannelKind::WindowsNamedPipe => {
            let path = descriptor.path.as_deref().ok_or_else(|| {
                CredentialChannelError::Protocol(
                    "a Windows-named-pipe channel must name a path".to_owned(),
                )
            })?;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(CredentialChannelError::Io)?;
            let writer = file.try_clone().map_err(CredentialChannelError::Io)?;
            Ok(Channel {
                reader: Box::new(file),
                writer: Box::new(writer),
            })
        }
        ChannelKind::InheritedFd | ChannelKind::DuplicatedHandle => Err(
            CredentialChannelError::UnsupportedChannelKind(descriptor.kind),
        ),
    }
}

/// Requests an access token on an already-open channel and returns the
/// granted material.
pub(crate) fn request_access_token<R: Read, W: Write>(
    channel: Channel<R, W>,
    run_id: &RunId,
    grant_id: &fieldnotes_field_protocol::grammar::GrantId,
    scopes: Vec<String>,
) -> Result<CredentialMaterial, CredentialChannelError> {
    let Channel {
        reader: raw_reader,
        writer: raw_writer,
    } = channel;
    let mut writer = FrameWriter::new(raw_writer, MAX_CREDENTIAL_FRAME_BYTES);
    let request = CredentialRequest {
        v: fieldnotes_field_protocol::grammar::ProtocolV1,
        frame_type: CredentialRequestTag,
        run_id: run_id.clone(),
        grant_id: grant_id.clone(),
        purpose: CredentialPurpose::AccessToken,
        scopes: (!scopes.is_empty()).then_some(scopes),
    };
    writer
        .write_credential_frame(&CredentialFrame::Request(Box::new(request)))
        .map_err(|error| CredentialChannelError::Protocol(error.to_string()))?;

    let mut reader = FrameReader::new(
        BufReader::new(raw_reader),
        MAX_CREDENTIAL_FRAME_BYTES,
        MAX_CREDENTIAL_FRAME_BYTES,
    );
    let raw = reader
        .next_frame()
        .map_err(|error| CredentialChannelError::Protocol(error.to_string()))?
        .ok_or_else(|| {
            CredentialChannelError::Protocol(
                "the protected channel closed before a response arrived".to_owned(),
            )
        })?;
    let frame = CredentialFrame::decode(raw.value)
        .map_err(|error| CredentialChannelError::Protocol(error.to_string()))?;
    let CredentialFrame::Response(response) = frame else {
        return Err(CredentialChannelError::Protocol(
            "expected a credential_response on the protected channel".to_owned(),
        ));
    };
    let CredentialResponse {
        outcome,
        material,
        message,
        ..
    } = *response;
    match outcome {
        CredentialOutcome::Granted => material.ok_or_else(|| {
            CredentialChannelError::Protocol("a granted response must carry material".to_owned())
        }),
        other => Err(CredentialChannelError::NotGranted {
            outcome: other,
            message: message.map(|text| text.as_str().to_owned()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{Channel, request_access_token};
    use fieldnotes_field_protocol::framing::FrameWriter;
    use fieldnotes_field_protocol::grammar::{CredentialResponseTag, GrantId, ProtocolV1, RunId};
    use fieldnotes_field_protocol::message::{
        CredentialFrame, CredentialMaterial, CredentialOutcome, CredentialResponse, MaterialKind,
    };

    fn run_id() -> RunId {
        RunId::parse("1a4c9f2e-0000-4000-8000-000000000001")
            .unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    fn grant_id() -> GrantId {
        GrantId::parse("0123456789abcdef0123456789abcdef")
            .unwrap_or_else(|error| panic!("must parse: {error}"))
    }

    /// Drives [`request_access_token`] over a real in-process OS pipe pair, a
    /// spawned thread playing core's side of the exchange -- this is the
    /// "in-process fixture channel A2 can exercise now" the end-to-end
    /// per-platform mechanism is deferred from (see the module docs and this
    /// crate's final report).
    #[test]
    fn a_granted_response_yields_the_material() {
        let (core_reader, field_writer) =
            std::io::pipe().unwrap_or_else(|error| panic!("pipe: {error}"));
        let (field_reader, core_writer) =
            std::io::pipe().unwrap_or_else(|error| panic!("pipe: {error}"));

        let core = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(core_reader);
            let _request =
                fieldnotes_field_protocol::framing::FrameReader::new(&mut reader, 131_072, 131_072)
                    .next_frame()
                    .unwrap_or_else(|error| panic!("core must read the request: {error}"))
                    .unwrap_or_else(|| panic!("core must see a request frame"));
            let response = CredentialResponse {
                v: ProtocolV1,
                frame_type: CredentialResponseTag,
                run_id: run_id(),
                grant_id: grant_id(),
                outcome: CredentialOutcome::Granted,
                material: Some(CredentialMaterial {
                    kind: MaterialKind::BearerToken,
                    value: "FIXTURE-NOT-A-REAL-TOKEN-canary".to_owned(),
                    username: None,
                    expires_at: None,
                    scopes: None,
                }),
                message: None,
            };
            let mut writer = FrameWriter::new(core_writer, 131_072);
            writer
                .write_credential_frame(&CredentialFrame::Response(Box::new(response)))
                .unwrap_or_else(|error| panic!("core must answer: {error}"));
        });

        let channel = Channel {
            reader: field_reader,
            writer: field_writer,
        };
        let material = request_access_token(channel, &run_id(), &grant_id(), vec![])
            .unwrap_or_else(|error| panic!("must be granted: {error}"));
        assert_eq!(material.value, "FIXTURE-NOT-A-REAL-TOKEN-canary");
        core.join()
            .unwrap_or_else(|_| panic!("core thread must not panic"));
    }

    #[test]
    fn a_denied_response_is_reported_as_not_granted() {
        let (core_reader, field_writer) =
            std::io::pipe().unwrap_or_else(|error| panic!("pipe: {error}"));
        let (field_reader, core_writer) =
            std::io::pipe().unwrap_or_else(|error| panic!("pipe: {error}"));

        let core = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(core_reader);
            let _request =
                fieldnotes_field_protocol::framing::FrameReader::new(&mut reader, 131_072, 131_072)
                    .next_frame()
                    .unwrap_or_else(|error| panic!("core must read the request: {error}"));
            let response = CredentialResponse {
                v: ProtocolV1,
                frame_type: CredentialResponseTag,
                run_id: run_id(),
                grant_id: grant_id(),
                outcome: CredentialOutcome::Denied,
                material: None,
                message: Some(
                    fieldnotes_field_protocol::grammar::MediumText::parse(
                        "an administrator must grant consent",
                    )
                    .unwrap_or_else(|error| panic!("must parse: {error}")),
                ),
            };
            let mut writer = FrameWriter::new(core_writer, 131_072);
            writer
                .write_credential_frame(&CredentialFrame::Response(Box::new(response)))
                .unwrap_or_else(|error| panic!("core must answer: {error}"));
        });

        let channel = Channel {
            reader: field_reader,
            writer: field_writer,
        };
        match request_access_token(channel, &run_id(), &grant_id(), vec![]) {
            Err(super::CredentialChannelError::NotGranted { outcome, .. }) => {
                assert_eq!(outcome, CredentialOutcome::Denied);
            }
            other => panic!("expected a not-granted refusal, got {other:?}"),
        }
        core.join()
            .unwrap_or_else(|_| panic!("core thread must not panic"));
    }
}
