//! The loopback redirect listener.
//!
//! RFC 8252 ("OAuth 2.0 for Native Apps") is what makes the authorization
//! code flow work for a desktop application with no fixed web origin: the
//! app opens an ephemeral, loopback-only HTTP listener, hands
//! `http://{host}:{port}{path}` to the authorization server as its redirect
//! URI for this one attempt, and the system browser's final navigation after
//! login delivers the authorization code to that listener instead of to a
//! public server this crate would otherwise have to run.
//!
//! The advertised host and the bound address are deliberately different
//! things. An authorization server compares the redirect URI against the value
//! registered for the application, waiving only the port for loopback, so the
//! URI must use the spelling the registration names — conventionally
//! `http://localhost` with no path. Sending `127.0.0.1`, or any path, against
//! such a registration fails as a redirect-URI mismatch even though it names
//! the same interface.
//!
//! Everything the listener reads is untrusted input: any local process (or,
//! on a shared machine, any other local user, depending on platform
//! permissions) can connect to a loopback port and send it anything. This
//! module treats the request accordingly:
//!
//! - it binds only `127.0.0.1` (a literal loopback address, so there is no
//!   DNS or hosts-file resolution step between "what this crate binds" and
//!   "what it believes it bound") regardless of the name it advertises;
//! - it accepts exactly one connection and then stops listening;
//! - it validates the returned `state` against the exact value generated for
//!   this attempt before trusting anything else in the request, which is
//!   what makes a `state` mismatch a hard CSRF rejection rather than a
//!   warning;
//! - every read is bounded in size and by a deadline, so a connection that
//!   sends nothing, or sends slowly, cannot hang the flow forever.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::pkce::State;

/// The maximum bytes read from one connection before giving up.
///
/// An authorization redirect's query string is at most a few kilobytes even
/// with a long `error_description`; this bound exists so a connection cannot
/// force this process to buffer an unbounded amount of untrusted input.
const MAX_REQUEST_BYTES: usize = 16 * 1024;

/// How long a single accepted connection has to finish sending its request
/// before this listener gives up on it.
const PER_CONNECTION_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// The host name advertised in the redirect URI.
///
/// This is deliberately not the literal address the listener binds. An
/// authorization server matches a redirect URI against the value registered
/// for the application, and a public-client loopback registration is usually
/// `http://localhost`: the server waives the port for loopback but compares the
/// host and path exactly. Advertising `127.0.0.1` against such a registration
/// is rejected with a redirect-URI mismatch even though it names the same
/// interface. Binding stays on the literal loopback address so no name
/// resolution sits between the browser and this process.
pub const DEFAULT_REDIRECT_HOST: &str = "localhost";

/// A bound loopback listener, ready to receive exactly one OAuth redirect.
pub struct LoopbackListener {
    listener: TcpListener,
    host: String,
    path: String,
}

impl LoopbackListener {
    /// Binds an ephemeral port on the IPv4 loopback address, advertising
    /// [`DEFAULT_REDIRECT_HOST`].
    ///
    /// Binding port `0` asks the operating system to choose an unused port,
    /// which is what makes this usable without the caller reserving one in
    /// advance or racing another process for a fixed port.
    pub fn bind(path: impl Into<String>) -> std::io::Result<Self> {
        Self::bind_advertising(DEFAULT_REDIRECT_HOST, path)
    }

    /// Binds as [`LoopbackListener::bind`] does, advertising `host` in the
    /// redirect URI instead of the default.
    ///
    /// The bound address is unaffected: only the URI handed to the
    /// authorization server changes, so a deployment whose registration names
    /// a different loopback spelling can match it without this process
    /// resolving a name.
    pub fn bind_advertising(
        host: impl Into<String>,
        path: impl Into<String>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        Ok(LoopbackListener {
            listener,
            host: host.into(),
            path: path.into(),
        })
    }

    /// The port the operating system assigned.
    pub fn port(&self) -> std::io::Result<u16> {
        Ok(self.listener.local_addr()?.port())
    }

    /// The redirect URI to hand the authorization server for this attempt:
    /// `http://{host}:{port}{path}`.
    ///
    /// An empty path yields no path segment at all, which is the form a
    /// public-client loopback registration of `http://localhost` matches.
    pub fn redirect_uri(&self) -> std::io::Result<String> {
        Ok(format!(
            "http://{}:{}{}",
            self.host,
            self.port()?,
            self.path
        ))
    }

    /// Waits for the single redirect callback, validates it, and returns the
    /// authorization code.
    ///
    /// Consumes the listener: whether this call succeeds, times out, or
    /// rejects the callback, the socket is not reused for a second attempt.
    /// `expected_state` must be the exact [`State`] generated for this
    /// attempt; a mismatch is reported as
    /// [`LoopbackError::StateMismatch`] before anything else in the request
    /// is trusted.
    pub fn await_callback(
        self,
        expected_state: &State,
        timeout: Duration,
    ) -> Result<CallbackResult, LoopbackError> {
        let stream = accept_within(&self.listener, timeout)?;
        let AcceptedRequest {
            request_line,
            mut stream,
        } = read_request(stream)?;
        let query = extract_query(&request_line, &self.path)?;
        let params = parse_query_params(&query);

        let state = params.get("state").map(String::as_str);
        if state != Some(expected_state.as_str()) {
            respond(&mut stream, ERROR_RESPONSE_BODY);
            return Err(LoopbackError::StateMismatch);
        }

        if let Some(error) = params.get("error") {
            let description = params
                .get("error_description")
                .map(String::as_str)
                .unwrap_or("")
                .chars()
                .take(512)
                .collect();
            respond(&mut stream, ERROR_RESPONSE_BODY);
            return Err(LoopbackError::AuthorizationDenied {
                error: truncate(error, 128),
                description,
            });
        }

        let Some(code) = params.get("code") else {
            respond(&mut stream, ERROR_RESPONSE_BODY);
            return Err(LoopbackError::MissingCode);
        };
        let code = code.clone();

        respond(&mut stream, SUCCESS_RESPONSE_BODY);
        Ok(CallbackResult { code })
    }
}

/// The authorization code recovered from a validated redirect.
///
/// Not itself a bearer credential (it is single-use and only redeemable with
/// this attempt's PKCE verifier, at the token endpoint, over TLS), but it is
/// still attempt-specific and short-lived, so callers should not retain it
/// beyond the immediate token exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackResult {
    /// The authorization code to redeem at the token endpoint.
    pub code: String,
}

/// Errors produced while waiting for or validating the redirect callback.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopbackError {
    /// No connection arrived before the deadline.
    Timeout,
    /// A connection arrived but did not send a complete, well-formed HTTP
    /// request within the bounds this listener enforces.
    MalformedRequest,
    /// The returned `state` did not match the value generated for this
    /// attempt. Treated as a potential cross-site request forgery attempt,
    /// not a retryable condition.
    StateMismatch,
    /// The authorization server reported that authorization was not granted
    /// (for example, the user declined consent).
    AuthorizationDenied {
        /// The OAuth `error` code, bounded and copied from the untrusted
        /// redirect.
        error: String,
        /// The OAuth `error_description`, bounded and copied from the
        /// untrusted redirect.
        description: String,
    },
    /// The request validated (correct path, correct `state`) but carried no
    /// `code` parameter.
    MissingCode,
    /// A local I/O failure (binding, accepting, or reading the socket).
    Io(String),
}

impl core::fmt::Display for LoopbackError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoopbackError::Timeout => {
                write!(
                    f,
                    "no redirect arrived before the loopback listener's deadline"
                )
            }
            LoopbackError::MalformedRequest => {
                write!(f, "the redirect request was not well-formed")
            }
            LoopbackError::StateMismatch => write!(
                f,
                "the redirect's state parameter did not match this attempt; possible cross-site request forgery"
            ),
            LoopbackError::AuthorizationDenied { error, description } => {
                write!(f, "authorization was not granted: {error} ({description})")
            }
            LoopbackError::MissingCode => {
                write!(f, "the redirect carried no authorization code")
            }
            LoopbackError::Io(reason) => write!(f, "loopback listener I/O error: {reason}"),
        }
    }
}

impl std::error::Error for LoopbackError {}

impl From<std::io::Error> for LoopbackError {
    fn from(error: std::io::Error) -> Self {
        LoopbackError::Io(error.to_string())
    }
}

struct AcceptedRequest {
    request_line: String,
    stream: TcpStream,
}

/// Accepts exactly one connection within `timeout`.
///
/// `TcpListener::accept` has no timeout parameter of its own, and this
/// listener deliberately never enters non-blocking mode to get one: on some
/// platforms (observed on macOS/BSD, where a plain `accept()` is used rather
/// than Linux's flag-carrying `accept4()`) a socket returned by `accept()`
/// can itself come back non-blocking when the *listening* socket is
/// non-blocking. That previously made the request read below race the
/// client's send — if the browser's bytes had not arrived yet by the time
/// `read` was first called, it failed immediately with `WouldBlock`/`EAGAIN`
/// instead of waiting, which is exactly the low-frequency failure this
/// function now avoids by construction.
///
/// Instead, the deadline is enforced by handing the ordinary *blocking*
/// `accept()` call to a background thread and waiting for it here with a
/// bounded channel receive. A one-shot listener that accepts a single
/// connection does not need a polling loop to get a deadline: `recv_timeout`
/// gives the same bound with no busy-wait and no non-blocking state to get
/// wrong.
///
/// If the deadline elapses first, the background thread is left running: it
/// holds only a cloned listener handle, touches nothing this function's
/// caller still owns, and will simply finish (accepting a connection nobody
/// reads from, or observing the listener drop) on its own.
fn accept_within(listener: &TcpListener, timeout: Duration) -> Result<TcpStream, LoopbackError> {
    let listener = listener.try_clone()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let _ = std::thread::spawn(move || {
        let _ = sender.send(listener.accept());
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok((stream, _peer))) => Ok(stream),
        Ok(Err(error)) => Err(LoopbackError::Io(error.to_string())),
        Err(
            std::sync::mpsc::RecvTimeoutError::Timeout
            | std::sync::mpsc::RecvTimeoutError::Disconnected,
        ) => Err(LoopbackError::Timeout),
    }
}

/// Reads one bounded HTTP request from `stream` and extracts its request
/// line, keeping the stream open so a response can be written back.
///
/// The deadline is enforced with `SO_RCVTIMEO` (via `set_read_timeout`), but
/// the exact `ErrorKind` a platform reports when that timeout elapses is not
/// standardized: it may be `WouldBlock` or `TimedOut`. Both are treated the
/// same way here — as "no data yet," not as a failure — and the loop keeps
/// waiting, re-arming the read for whatever time remains, until its own
/// deadline (computed independently of the socket option) is actually
/// reached. Only then is it reported, as [`LoopbackError::MalformedRequest`]:
/// a connection that never finished sending a request within the bound this
/// listener enforces is indistinguishable from one that sent nothing at all.
/// Any other I/O error is a genuine failure and is still reported as such.
fn read_request(mut stream: TcpStream) -> Result<AcceptedRequest, LoopbackError> {
    let deadline = Instant::now() + PER_CONNECTION_READ_TIMEOUT;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if buffer.len() >= MAX_REQUEST_BYTES {
            return Err(LoopbackError::MalformedRequest);
        }
        // A blank line (end of headers) is all this listener needs; the
        // body, if any, is never read or used.
        if find_header_terminator(&buffer).is_some() {
            break;
        }
        let remaining = deadline.checked_duration_since(Instant::now());
        let Some(remaining) = remaining.filter(|remaining| !remaining.is_zero()) else {
            return Err(LoopbackError::MalformedRequest);
        };
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= deadline {
                    return Err(LoopbackError::MalformedRequest);
                }
                // Not actually past the deadline yet (a spurious or
                // early-armed wakeup) — go around and re-arm for the time
                // that is left, rather than treating this as a failure.
            }
            Err(error) => return Err(error.into()),
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    let request_line = text
        .lines()
        .next()
        .ok_or(LoopbackError::MalformedRequest)?
        .to_owned();
    Ok(AcceptedRequest {
        request_line,
        stream,
    })
}

fn find_header_terminator(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// Extracts and validates the query string from a request line, requiring
/// a `GET` request to exactly `expected_path`.
///
/// An empty `expected_path` matches a request for `/`. A redirect URI with no
/// path segment is the form a public-client loopback registration matches, but
/// a browser still requests the root, so the two spellings name the same
/// target and must compare equal.
fn extract_query(request_line: &str, expected_path: &str) -> Result<String, LoopbackError> {
    let mut parts = request_line.split(' ');
    let method = parts.next().ok_or(LoopbackError::MalformedRequest)?;
    let target = parts.next().ok_or(LoopbackError::MalformedRequest)?;
    if method != "GET" {
        return Err(LoopbackError::MalformedRequest);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let expected = if expected_path.is_empty() {
        "/"
    } else {
        expected_path
    };
    if path != expected {
        return Err(LoopbackError::MalformedRequest);
    }
    Ok(query.to_owned())
}

/// Parses `application/x-www-form-urlencoded`-shaped query parameters,
/// percent-decoding each value. Unknown or duplicate parameters are not an
/// error; only `state`, `code`, `error`, and `error_description` are ever
/// read by this module.
fn parse_query_params(query: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let decoded = percent_encoding::percent_decode_str(value)
            .decode_utf8_lossy()
            .into_owned();
        params.insert(name.to_owned(), decoded);
    }
    params
}

fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

const SUCCESS_RESPONSE_BODY: &str = "<!doctype html><title>Fieldnotes</title><p>Authentication complete. You can close this window.</p>";
const ERROR_RESPONSE_BODY: &str = "<!doctype html><title>Fieldnotes</title><p>Authentication could not be completed. You can close this window and try again.</p>";

/// Writes a minimal, fixed HTML response and closes the connection.
///
/// The response body is one of the two constants above; nothing from the
/// request (including any parameter this crate parsed) is ever echoed back
/// into it, so there is no reflected-content risk in the browser tab the
/// user is looking at.
fn respond(stream: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    // Best-effort: the browser already has what it needs (the request
    // completed); a write failure here does not change the outcome of the
    // authorization attempt.
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::io::BufReader;

    fn connect_and_send(port: u16, request: &str) -> TcpStream {
        let mut stream = loop {
            if let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
                break stream;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let _ = stream.write_all(request.as_bytes());
        stream
    }

    fn read_status_line(stream: TcpStream) -> String {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        line
    }

    #[test]
    fn binds_only_the_ipv4_loopback_address() -> Result<(), Box<dyn std::error::Error>> {
        let listener = LoopbackListener::bind("/callback")?;
        let addr = listener.listener.local_addr()?;
        assert!(addr.ip().is_loopback());
        assert_eq!(addr.ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
        Ok(())
    }

    #[test]
    fn redirect_uri_embeds_the_bound_port_and_path() -> Result<(), Box<dyn std::error::Error>> {
        let listener = LoopbackListener::bind("/callback")?;
        let port = listener.port()?;
        assert_eq!(
            listener.redirect_uri()?,
            format!("http://localhost:{port}/callback"),
            "the advertised host is the registered loopback name, not the bound address"
        );
        Ok(())
    }

    #[test]
    fn an_empty_path_advertises_no_path_segment_but_still_matches_the_root_request()
    -> Result<(), Box<dyn std::error::Error>> {
        // A public-client loopback registration of `http://localhost` waives
        // the port but compares the path exactly, so the redirect URI must
        // carry no path at all. The browser nonetheless requests `/`, so the
        // two spellings have to name the same target.
        let listener = LoopbackListener::bind("")?;
        let port = listener.port()?;
        assert_eq!(listener.redirect_uri()?, format!("http://localhost:{port}"));
        assert_eq!(
            extract_query("GET /?code=abc&state=xyz HTTP/1.1", ""),
            Ok("code=abc&state=xyz".to_owned())
        );
        Ok(())
    }

    #[test]
    fn a_deployment_may_advertise_a_different_loopback_spelling()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = LoopbackListener::bind_advertising("127.0.0.1", "")?;
        let port = listener.port()?;
        assert_eq!(listener.redirect_uri()?, format!("http://127.0.0.1:{port}"));
        Ok(())
    }

    #[test]
    fn accepts_a_matching_state_and_code() -> Result<(), Box<dyn std::error::Error>> {
        let listener = LoopbackListener::bind("/callback")?;
        let port = listener.port()?;
        let mut random = FixedRandom;
        let state = State::generate(&mut random);
        let state_value = state.as_str().to_owned();

        let handle =
            std::thread::spawn(move || listener.await_callback(&state, Duration::from_secs(5)));
        let request = format!(
            "GET /callback?code=abc123&state={state_value} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        );
        let stream = connect_and_send(port, &request);
        let status_line = read_status_line(stream);
        assert!(status_line.starts_with("HTTP/1.1 200"));

        let result = handle
            .join()
            .unwrap_or_else(|_| Err(LoopbackError::Io("test thread panicked".to_owned())))?;
        assert_eq!(result.code, "abc123");
        Ok(())
    }

    #[test]
    fn accepts_a_connection_that_sends_its_request_after_a_short_delay()
    -> Result<(), Box<dyn std::error::Error>> {
        // Regression test for the CI flake where the accept path treated
        // `WouldBlock`/`EAGAIN` as fatal instead of "not ready yet." The bug
        // depended on the client connecting *before* it had anything to
        // send: on the affected platform, the accepted socket could come
        // back non-blocking, so the very first read raced the client and
        // failed outright instead of waiting for the request to arrive.
        // Connecting immediately and then deliberately waiting before
        // writing anything reproduces exactly that window on every run,
        // rather than relying on scheduling luck the way the original flake
        // did.
        let listener = LoopbackListener::bind("/callback")?;
        let port = listener.port()?;
        let mut random = FixedRandom;
        let state = State::generate(&mut random);
        let state_value = state.as_str().to_owned();

        let handle =
            std::thread::spawn(move || listener.await_callback(&state, Duration::from_secs(5)));

        let mut stream = loop {
            if let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
                break stream;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        // Connect now, but hold off on sending the request: this is the
        // window in which the old code's first read could observe no data
        // yet and fail instead of waiting for it.
        std::thread::sleep(Duration::from_millis(200));
        let request = format!(
            "GET /callback?code=abc123&state={state_value} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        );
        stream.write_all(request.as_bytes())?;
        let status_line = read_status_line(stream);
        assert!(status_line.starts_with("HTTP/1.1 200"));

        let result = handle
            .join()
            .unwrap_or_else(|_| Err(LoopbackError::Io("test thread panicked".to_owned())))?;
        assert_eq!(result.code, "abc123");
        Ok(())
    }

    #[test]
    fn rejects_a_mismatched_state_as_a_hard_error() -> Result<(), Box<dyn std::error::Error>> {
        let listener = LoopbackListener::bind("/callback")?;
        let port = listener.port()?;
        let mut random = FixedRandom;
        let expected = State::generate(&mut random);

        let handle =
            std::thread::spawn(move || listener.await_callback(&expected, Duration::from_secs(5)));
        let request =
            "GET /callback?code=abc123&state=attacker-supplied HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let stream = connect_and_send(port, request);
        let _ = read_status_line(stream);

        let result = handle
            .join()
            .unwrap_or_else(|_| Err(LoopbackError::Io("test thread panicked".to_owned())));
        assert_eq!(result, Err(LoopbackError::StateMismatch));
        Ok(())
    }

    #[test]
    fn times_out_when_nothing_connects() -> Result<(), Box<dyn std::error::Error>> {
        let listener = LoopbackListener::bind("/callback")?;
        let mut random = FixedRandom;
        let state = State::generate(&mut random);
        let result = listener.await_callback(&state, Duration::from_millis(80));
        assert_eq!(result, Err(LoopbackError::Timeout));
        Ok(())
    }

    #[test]
    fn reports_authorization_denied_from_an_error_redirect()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = LoopbackListener::bind("/callback")?;
        let port = listener.port()?;
        let mut random = FixedRandom;
        let expected = State::generate(&mut random);
        let state_value = expected.as_str().to_owned();

        let handle =
            std::thread::spawn(move || listener.await_callback(&expected, Duration::from_secs(5)));
        let request = format!(
            "GET /callback?error=access_denied&error_description=User%20declined&state={state_value} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
        );
        let stream = connect_and_send(port, &request);
        let _ = read_status_line(stream);

        let result = handle
            .join()
            .unwrap_or_else(|_| Err(LoopbackError::Io("test thread panicked".to_owned())));
        assert_eq!(
            result,
            Err(LoopbackError::AuthorizationDenied {
                error: "access_denied".to_owned(),
                description: "User declined".to_owned(),
            })
        );
        Ok(())
    }

    struct FixedRandom;
    impl fieldnotes_domain::RandomSource for FixedRandom {
        fn fill_bytes(&mut self, buffer: &mut [u8]) {
            buffer.fill(0x7a);
        }
    }
}
