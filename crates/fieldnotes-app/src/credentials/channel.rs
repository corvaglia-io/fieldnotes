//! The protected credential channel: the only path a secret takes to a Field.
//!
//! A2 section 12 fixes the shape and the invariant and leaves the per-platform
//! mechanism to this gate. This module implements it as a **per-run endpoint
//! core creates, names in the collection request, serves for the length of one
//! run, and destroys when the run ends**:
//!
//! 1. core creates a private directory and binds a Unix-domain socket inside
//!    it, before the Field process is spawned;
//! 2. the collection request carries a
//!    [`CredentialGrant`] —
//!    the non-secret profile name, a single-use `grant_id`, the channel
//!    descriptor naming that socket path, an expiry, and the granted scopes;
//! 3. the Field connects and writes a `credential_request` naming its grant;
//! 4. core answers `credential_response` with the minted **access token**, or
//!    with an actionable non-granted outcome;
//! 5. when the run ends, core stops serving, unlinks the socket, removes the
//!    directory, and drops the token, whose buffer is zeroized on drop.
//!
//! # Why a path-based channel, and never a descriptor-based one
//!
//! A2 originally admitted four mechanisms, and **two of them could not be
//! built anywhere in this workspace.** `inherited_fd` and `duplicated_handle`
//! both required turning a raw descriptor or handle into an I/O object,
//! which needs `unsafe`; `unsafe_code = "forbid"` is set in the root
//! manifest and a crate cannot locally override a `forbid`. They would also
//! have needed the *spawn* to carry an extra descriptor, which
//! [`FieldSpawn`](fieldnotes_field_protocol::host::FieldSpawn) does not do.
//! This was a genuine contradiction between two approved documents — the
//! protocol named a mechanism the project's own lint policy prohibits — and
//! it is now resolved rather than worked around with `unsafe`: ADR 0013
//! narrowed A2 section 12 to the two path-based kinds below, and
//! [`ChannelKind`] no longer has variants for the other two at all, so a
//! frame naming one is unrepresentable rather than merely refused.
//!
//! **Core therefore neither implements nor can even construct those two
//! kinds.** A descriptor core cannot pass is also one no Field can open: the
//! completed Outlook Mail, Calendar, and Contacts Fields independently
//! reached the same conclusion before the amendment landed, and refused them
//! on their side too. Offering one would have failed at run time on every
//! platform.
//!
//! A path-based endpoint needs nothing from the spawn at all: the path travels
//! in the request, exactly as A2 requires ("named in the request rather than in
//! the environment"), and both client ends are plain standard library —
//! [`std::os::unix::net::UnixStream::connect`] on Unix and
//! [`std::fs::OpenOptions::open`] on a Windows named pipe — so a Field takes on
//! no dependency to talk to core.
//!
//! # What stops another local process from taking the token
//!
//! Three things, in order of strength:
//!
//! - the socket lives in a directory **created** with mode `0700` (not chmodded
//!   to it afterwards), and the socket itself is `0600`, so on a multi-user
//!   machine no other user can reach it (`docs/security.md` trusts the local
//!   operating-system account, and nothing weaker);
//! - a connection must present this run's exact `grant_id`, which core
//!   generated from the injected random source for this one run and sent to the
//!   child on its standard input;
//! - the endpoint exists only while the run does. [`ProtectedChannel::close`]
//!   runs from [`Drop`], so every path out of a run — success, refusal, panic,
//!   or an early `?` — stops the listener, unlinks the socket, removes the
//!   directory, and drops the token. A process killed outright leaves an inert
//!   socket file with nothing listening on it and no token anywhere;
//! - core stops honoring the grant after its expiry even if an endpoint somehow
//!   outlived it.
//!
//! # This platform
//!
//! Unix (macOS and Linux) is implemented here, as `unix_socket_path`. Windows
//! is `windows_named_pipe` — the descriptor shape is settled and the Contacts
//! Field already implements the client end — and what is missing is only the
//! *server*: `CreateNamedPipe` is not in the standard library, so it needs
//! either `unsafe` FFI (forbidden, as above) or a safe wrapper dependency, and
//! either way it belongs with the rest of the per-platform child-process
//! handling in [`fieldnotes_field_protocol::host`] rather than in an
//! application module. Until it lands there,
//! [`ProtectedChannel::open`] fails closed with that explanation and `sync`
//! refuses the run rather than starting a Field it cannot authenticate.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use fieldnotes_credentials::oauth::AccessToken;
use fieldnotes_field_protocol::grammar::{GrantId, MediumText, OffsetDatetime, ProfileRef, RunId};
use fieldnotes_field_protocol::message::{
    ChannelDescriptor, ChannelKind, CredentialGrant, CredentialMaterial, CredentialOutcome,
    CredentialPurpose, CredentialResponse, MaterialKind, Validate,
};

use super::CredentialFailure;

/// How often the accept loop wakes to check for a connection or for shutdown.
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// How long one connected Field has to send a request, and to read its answer,
/// before core gives up on that connection.
const PER_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// How many times one run may be granted material.
///
/// A run needs one grant; a Field that lost its copy may ask again, and a
/// core-owned refresh (A2 section 12's `renew` purpose) is a second ask. A
/// bound exists so a misbehaving child cannot spin on the channel for the whole
/// run.
const MAX_GRANTS_PER_RUN: u64 = 16;

/// Everything the channel needs to serve exactly one run's grant.
pub struct GrantSpec {
    /// Core's identifier for this run.
    pub run_id: RunId,
    /// The non-secret profile name the grant references.
    pub profile: ProfileRef,
    /// This run's single-use channel authorization token.
    pub grant_id: GrantId,
    /// The granted scopes, exactly as the Field's manifest declared them.
    pub scopes: Vec<String>,
    /// The wall-clock instant the grant stops being honored, for the request
    /// frame.
    pub expires_at: OffsetDatetime,
    /// How long the grant is honored, measured monotonically.
    ///
    /// Separate from `expires_at` deliberately: enforcement must not depend on
    /// a wall clock that can step sideways mid-run.
    pub lifetime: Duration,
    /// The minted access token. Moved into the serving thread, and dropped
    /// (zeroized) when the channel closes.
    pub token: AccessToken,
    /// The run's frame ceiling, applied to the channel exactly as to the other
    /// streams.
    pub max_frame_bytes: u64,
}

/// Counts of what crossed the channel, for reporting. Never any material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelCounts {
    /// How many `credential_request` frames were read.
    pub requests: u64,
    /// How many requests were answered with material.
    pub granted: u64,
    /// How many requests were refused, for any reason.
    pub refused: u64,
}

#[derive(Debug, Default)]
struct SharedState {
    stop: AtomicBool,
    requests: AtomicU64,
    granted: AtomicU64,
    refused: AtomicU64,
}

/// A live protected channel, serving one run's grant.
///
/// Dropping it stops serving and removes the endpoint, so no error path can
/// leave a socket behind that would outlive the run it belongs to.
pub struct ProtectedChannel {
    grant: CredentialGrant,
    state: Arc<SharedState>,
    worker: Option<std::thread::JoinHandle<()>>,
    directory: PathBuf,
}

impl ProtectedChannel {
    /// Creates the endpoint and starts serving.
    ///
    /// Called after a usable access token exists and before the Field process
    /// is spawned, so a run never starts with a channel that cannot answer.
    pub fn open(spec: GrantSpec) -> Result<Self, CredentialFailure> {
        let started = start(spec)?;
        started
            .grant
            .validate()
            .map_err(|error| CredentialFailure::Channel {
                detail: format!("core's own credential grant is invalid: {error}"),
            })?;
        Ok(ProtectedChannel {
            grant: started.grant,
            state: started.state,
            worker: Some(started.worker),
            directory: started.directory,
        })
    }

    /// The reference — never a value — the collection request carries.
    #[must_use]
    pub fn grant(&self) -> &CredentialGrant {
        &self.grant
    }

    /// What crossed the channel so far.
    #[must_use]
    pub fn counts(&self) -> ChannelCounts {
        ChannelCounts {
            requests: self.state.requests.load(Ordering::Relaxed),
            granted: self.state.granted.load(Ordering::Relaxed),
            refused: self.state.refused.load(Ordering::Relaxed),
        }
    }

    /// Stops serving, joins the serving thread, and removes the endpoint.
    ///
    /// Idempotent, and also performed on drop.
    pub fn close(&mut self) {
        self.state.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // The directory holds only the socket, and it was created for this run
        // alone.
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

impl Drop for ProtectedChannel {
    fn drop(&mut self) {
        self.close();
    }
}

impl core::fmt::Debug for ProtectedChannel {
    /// Prints the non-secret grant reference and the counts, and nothing else:
    /// the token lives in the serving thread and is not reachable from here at
    /// all, which is a stronger guarantee than a redacting field.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProtectedChannel")
            .field("profile_ref", &self.grant.profile_ref)
            .field("channel", &self.grant.channel)
            .field("counts", &self.counts())
            .finish_non_exhaustive()
    }
}

/// What [`start`] produced.
struct Started {
    grant: CredentialGrant,
    state: Arc<SharedState>,
    worker: std::thread::JoinHandle<()>,
    directory: PathBuf,
}

/// Builds the private directory the endpoint lives in.
///
/// The system temporary directory rather than the notebook's own sync state,
/// for one hard reason: a Unix socket path must fit `sockaddr_un.sun_path`,
/// which is 104 bytes on macOS, and a notebook can live at an arbitrarily deep
/// path. Nothing durable and nothing secret is written here — the socket is a
/// rendezvous point, and the material never touches the filesystem.
fn endpoint_directory(grant_id: &GrantId) -> Result<PathBuf, CredentialFailure> {
    let short: String = grant_id.as_str().chars().take(12).collect();
    let directory = std::env::temp_dir().join(format!("fn-cred-{short}"));
    // A leftover directory can only be one this process is about to replace: a
    // grant identifier is generated per run and the endpoint is named after it.
    let _ = std::fs::remove_dir_all(&directory);
    create_private_directory(&directory)?;
    Ok(directory)
}

/// Creates the endpoint directory **already** restricted to this user.
///
/// The mode is set at creation rather than with a `chmod` afterwards, which
/// matters: `create_dir_all` would briefly create it group- and
/// world-searchable (mode `0777` less the umask), and a directory descriptor
/// another local user opened during that window would keep working after the
/// permissions were tightened. Nothing is *in* the directory during that window
/// today, so the window is not exploitable as written — which is exactly the
/// kind of reasoning that stops being true after an unrelated edit, so the
/// window is closed instead of argued about.
#[cfg(unix)]
fn create_private_directory(directory: &std::path::Path) -> Result<(), CredentialFailure> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|error| CredentialFailure::Channel {
            detail: format!(
                "could not create the private channel directory {}: {error}",
                directory.display()
            ),
        })
}

#[cfg(not(unix))]
fn create_private_directory(directory: &std::path::Path) -> Result<(), CredentialFailure> {
    std::fs::create_dir_all(directory).map_err(|error| CredentialFailure::Channel {
        detail: format!(
            "could not create the channel directory {}: {error}",
            directory.display()
        ),
    })
}

/// Restricts the bound socket itself to this user.
///
/// Defense in depth behind the `0700` directory, which is the control that
/// actually holds on every Unix: Linux checks write permission on a socket file
/// at `connect` time, while macOS historically does not check it at all. Setting
/// `0600` costs nothing and means the endpoint is protected on Linux even if the
/// directory permission were ever weakened.
#[cfg(unix)]
fn restrict_socket(path: &std::path::Path) -> Result<(), CredentialFailure> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|error| {
        CredentialFailure::Channel {
            detail: format!("could not restrict the channel socket: {error}"),
        }
    })
}

#[cfg(unix)]
fn start(spec: GrantSpec) -> Result<Started, CredentialFailure> {
    use std::os::unix::net::UnixListener;

    let directory = endpoint_directory(&spec.grant_id)?;
    // One byte of name, so the whole path stays comfortably inside the
    // platform's `sun_path` bound.
    let socket_path = directory.join("s");
    let listener = UnixListener::bind(&socket_path).map_err(|error| {
        let _ = std::fs::remove_dir_all(&directory);
        CredentialFailure::Channel {
            detail: format!("could not bind the channel socket: {error}"),
        }
    })?;
    if let Err(error) = listener.set_nonblocking(true) {
        let _ = std::fs::remove_dir_all(&directory);
        return Err(CredentialFailure::Channel {
            detail: format!("could not configure the channel socket: {error}"),
        });
    }
    if let Err(failure) = restrict_socket(&socket_path) {
        let _ = std::fs::remove_dir_all(&directory);
        return Err(failure);
    }
    let path_text = socket_path
        .to_str()
        .ok_or_else(|| CredentialFailure::Channel {
            detail: "the channel socket path is not valid UTF-8".to_owned(),
        })?
        .to_owned();
    let descriptor = ChannelDescriptor {
        kind: ChannelKind::UnixSocketPath,
        path: Some(path_text),
    };
    descriptor
        .validate()
        .map_err(|error| CredentialFailure::Channel {
            detail: format!("core's own channel descriptor is invalid: {error}"),
        })?;

    let grant = CredentialGrant {
        profile_ref: spec.profile,
        grant_id: spec.grant_id.clone(),
        channel: descriptor,
        expires_at: spec.expires_at,
        scopes: if spec.scopes.is_empty() {
            None
        } else {
            Some(spec.scopes.clone())
        },
    };
    let state = Arc::new(SharedState::default());
    let worker_state = Arc::clone(&state);
    let session = Session {
        run_id: spec.run_id,
        grant_id: spec.grant_id,
        scopes: spec.scopes,
        material_expires_at: spec.expires_at,
        deadline: Instant::now() + spec.lifetime,
        max_frame_bytes: spec.max_frame_bytes,
        // The token is *moved* here and reachable from nowhere else — not from
        // `ProtectedChannel`, not from `sync`. It is dropped, and its buffer
        // zeroized, when this thread ends.
        token: spec.token,
    };
    let worker = std::thread::spawn(move || {
        serve_loop(&listener, &session, &worker_state);
    });
    Ok(Started {
        grant,
        state,
        worker,
        directory,
    })
}

#[cfg(not(unix))]
fn start(spec: GrantSpec) -> Result<Started, CredentialFailure> {
    let _ = spec;
    Err(CredentialFailure::Channel {
        detail: "core has no protected-channel server for this platform yet. The mechanism is \
                 settled and the Field side of it already exists: a `windows_named_pipe` channel, \
                 whose path the collection request carries and whose client end a Field opens with \
                 std::fs::OpenOptions. What is missing is the server end, which needs CreateNamedPipe \
                 and therefore either unsafe FFI (forbidden workspace-wide) or a safe wrapper \
                 dependency, and which belongs with the rest of the per-platform child-process \
                 handling in the Field-protocol host crate. Core refuses to start a run it cannot \
                 authenticate rather than delivering a credential any other way."
            .to_owned(),
    })
}

/// Everything one channel session answers with, including the token.
///
/// Deliberately has no `Debug` implementation: there is no formatting call for
/// a stray `{:?}` to make.
#[cfg(unix)]
struct Session {
    run_id: RunId,
    grant_id: GrantId,
    scopes: Vec<String>,
    material_expires_at: OffsetDatetime,
    deadline: Instant,
    max_frame_bytes: u64,
    token: AccessToken,
}

#[cfg(unix)]
fn serve_loop(
    listener: &std::os::unix::net::UnixListener,
    session: &Session,
    state: &Arc<SharedState>,
) {
    while !state.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => serve_connection(stream, session, state),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            // A failed accept is not a reason to stop honoring the grant for
            // the rest of the run; the next poll tries again.
            Err(_) => std::thread::sleep(ACCEPT_POLL_INTERVAL),
        }
    }
}

#[cfg(unix)]
fn serve_connection(
    stream: std::os::unix::net::UnixStream,
    session: &Session,
    state: &Arc<SharedState>,
) {
    use fieldnotes_field_protocol::framing::{FrameReader, FrameWriter};
    use fieldnotes_field_protocol::message::CredentialFrame;

    let _ = stream.set_read_timeout(Some(PER_CONNECTION_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PER_CONNECTION_TIMEOUT));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = FrameReader::new(
        std::io::BufReader::new(read_half),
        session.max_frame_bytes,
        // One connection may carry a handful of requests, so the per-connection
        // total is a small multiple of the frame ceiling rather than unbounded.
        session
            .max_frame_bytes
            .saturating_mul(MAX_GRANTS_PER_RUN * 4),
    );
    let mut writer = FrameWriter::new(&stream, session.max_frame_bytes);
    loop {
        if state.stop.load(Ordering::SeqCst) {
            return;
        }
        let frame = match reader.next_credential_frame() {
            Ok(Some(frame)) => frame,
            // A clean end of stream, or anything malformed: core says nothing
            // more on this connection. A malformed channel frame is not a
            // reason to volunteer material.
            Ok(None) | Err(_) => return,
        };
        let request = match frame {
            CredentialFrame::Request(request) => request,
            // Core does not accept a response on its own channel.
            CredentialFrame::Response(_) => {
                state.refused.fetch_add(1, Ordering::Relaxed);
                let _ = writer.write_credential_frame(&CredentialFrame::Response(Box::new(
                    refusal(session, CredentialOutcome::Denied, "core answers credential requests on this channel and accepts no response frame"),
                )));
                return;
            }
        };
        state.requests.fetch_add(1, Ordering::Relaxed);
        let response = decide(session, state, &request);
        if writer
            .write_credential_frame(&CredentialFrame::Response(Box::new(response)))
            .is_err()
        {
            return;
        }
    }
}

/// Decides one request, counting the outcome.
#[cfg(unix)]
fn decide(
    session: &Session,
    state: &Arc<SharedState>,
    request: &fieldnotes_field_protocol::message::CredentialRequest,
) -> CredentialResponse {
    let refuse = |outcome: CredentialOutcome, message: &str| {
        state.refused.fetch_add(1, Ordering::Relaxed);
        refusal(session, outcome, message)
    };
    if request.validate().is_err() {
        return refuse(
            CredentialOutcome::Denied,
            "the credential request did not satisfy the protocol schema",
        );
    }
    // An unknown grant is answered as such rather than ignored: A2 gives the
    // outcome a name so a Field can report something actionable instead of
    // hanging.
    if request.grant_id != session.grant_id || request.run_id != session.run_id {
        return refuse(
            CredentialOutcome::UnknownGrant,
            "this grant is not the grant core issued for this run",
        );
    }
    if Instant::now() >= session.deadline {
        return refuse(
            CredentialOutcome::Expired,
            "the grant expired with the run it was issued for; core mints material for one run \
             only",
        );
    }
    match request.purpose {
        CredentialPurpose::AccessToken | CredentialPurpose::Renew => {}
        CredentialPurpose::ApiToken | CredentialPurpose::BasicCredentials => {
            return refuse(
                CredentialOutcome::Denied,
                "this profile holds an OAuth authorization-code credential, so core can deliver \
                 only a bearer access token",
            );
        }
    }
    // A Field may ask for narrower scopes than it was granted, but never for
    // more: core requests only what the manifest declares and delivers only
    // what it requested.
    if let Some(requested) = &request.scopes
        && let Some(extra) = requested
            .iter()
            .find(|scope| !session.scopes.iter().any(|granted| granted == *scope))
    {
        return refuse(
            CredentialOutcome::Denied,
            &format!(
                "scope `{extra}` is outside this grant; core requests only the scopes a Field's \
                 manifest declares"
            ),
        );
    }
    if state.granted.load(Ordering::Relaxed) >= MAX_GRANTS_PER_RUN {
        return refuse(
            CredentialOutcome::Denied,
            "this run has already been granted material the maximum number of times",
        );
    }
    // The one place material leaves its wrapper, named so a reviewer can grep
    // for it. It goes into a frame written to this run's own channel and
    // nowhere else.
    let material = CredentialMaterial {
        kind: MaterialKind::BearerToken,
        value: session.token.expose_secret().to_owned(),
        username: None,
        expires_at: Some(session.material_expires_at),
        scopes: if session.scopes.is_empty() {
            None
        } else {
            Some(session.scopes.clone())
        },
    };
    state.granted.fetch_add(1, Ordering::Relaxed);
    CredentialResponse {
        v: fieldnotes_field_protocol::grammar::ProtocolV1,
        frame_type: fieldnotes_field_protocol::grammar::CredentialResponseTag,
        run_id: session.run_id.clone(),
        grant_id: session.grant_id.clone(),
        outcome: CredentialOutcome::Granted,
        material: Some(material),
        message: None,
    }
}

/// A non-granted response carrying an actionable, secret-free message.
#[cfg(unix)]
fn refusal(session: &Session, outcome: CredentialOutcome, message: &str) -> CredentialResponse {
    CredentialResponse {
        v: fieldnotes_field_protocol::grammar::ProtocolV1,
        frame_type: fieldnotes_field_protocol::grammar::CredentialResponseTag,
        run_id: session.run_id.clone(),
        grant_id: session.grant_id.clone(),
        outcome,
        material: None,
        message: MediumText::parse(message).ok().or_else(|| {
            // Every message above is well inside the schema's bound; this
            // fallback exists so a refusal can never fail to be a refusal.
            MediumText::parse("core refused this credential request").ok()
        }),
    }
}
