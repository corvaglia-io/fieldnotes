//! Enough child-process support to drive a Field executable and observe its
//! protocol behavior.
//!
//! This is the host half of the boundary, not a sync orchestrator. It starts one
//! configured executable for one bounded operation, writes core frames to its
//! standard input, reads bounded events from its standard output, captures its
//! standard error into a ring buffer, and classifies how it ended.
//!
//! # Executable trust and discovery
//!
//! Core runs only a Field executable at a **configured, pinned absolute path**.
//! There is no `PATH` search and no name-based discovery: v0.1 must not execute
//! a plausible binary and hand it credentials because it returned a plausible
//! manifest. [`FieldSpawn::new`] refuses a relative path for that reason.
//!
//! # The child environment is built, not inherited
//!
//! Core builds a sanitized allowlisted environment. An inherited environment
//! leaks to grandchild processes and, on some systems, to other processes; it
//! appears in crash dumps and debugging tools; and it has the process's whole
//! lifetime. [`FieldSpawn::argv`] and [`FieldSpawn::environment`] expose exactly
//! what the child was given, so a secret-canary test can assert the canary is in
//! neither.

use std::collections::BTreeMap;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::codes::RejectionCode;
use crate::framing::{FrameError, FrameReader, FrameWriter, RawFrame, StderrCapture};
use crate::limits::Limits;
use crate::message::{CoreFrame, FieldEvent};
use crate::session::ExitObservation;

/// The one argv token that selects a Field's operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Return the manifest and negotiate the protocol.
    Describe,
    /// Collect one bounded run.
    Collect,
}

impl Operation {
    /// The argv token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Describe => "describe",
            Operation::Collect => "collect",
        }
    }
}

/// Environment names passed through to a child on this platform.
///
/// Everything else is dropped. On Unix a Rust binary needs nothing; on Windows a
/// process needs the system root and temporary directories to load its runtime.
#[cfg(windows)]
pub const PLATFORM_ENVIRONMENT_ALLOWLIST: [&str; 7] = [
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "TEMP",
    "TMP",
    "PATHEXT",
    "COMSPEC",
];

/// Environment names passed through to a child on this platform.
#[cfg(not(windows))]
pub const PLATFORM_ENVIRONMENT_ALLOWLIST: [&str; 0] = [];

/// A spawn recipe for one Field process.
#[derive(Debug, Clone)]
pub struct FieldSpawn {
    executable: PathBuf,
    operation: Operation,
    environment: BTreeMap<String, String>,
}

impl FieldSpawn {
    /// Pins the executable to run.
    ///
    /// Refuses a relative path, because a relative path is a discovery
    /// mechanism and core performs no discovery.
    pub fn new(executable: impl Into<PathBuf>, operation: Operation) -> io::Result<Self> {
        let executable = executable.into();
        if !executable.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a Field executable is a configured, pinned absolute path: core performs no PATH \
                 search and no name-based discovery",
            ));
        }
        let mut environment = BTreeMap::new();
        for name in PLATFORM_ENVIRONMENT_ALLOWLIST {
            if let Ok(value) = std::env::var(name) {
                environment.insert(name.to_owned(), value);
            }
        }
        Ok(FieldSpawn {
            executable,
            operation,
            environment,
        })
    }

    /// Adds one allowlisted environment entry.
    ///
    /// Never a secret: credential material crosses only on the protected
    /// channel.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    /// The exact argv the child will be given.
    #[must_use]
    pub fn argv(&self) -> Vec<String> {
        vec![
            self.executable.display().to_string(),
            self.operation.as_str().to_owned(),
        ]
    }

    /// The exact environment the child will be given.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Starts the child with piped standard input, output, and error.
    pub fn spawn(&self, limits: Limits) -> io::Result<FieldProcess> {
        let mut command = Command::new(&self.executable);
        command
            .arg(self.operation.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        for (name, value) in &self.environment {
            command.env(name, value);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the child has no standard output",
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "the child has no standard error")
        })?;

        let (sender, receiver) = sync_channel::<StdoutItem>(64);
        let frame_bytes = limits.max_frame_bytes;
        let total_bytes = limits.max_run_stdout_bytes;
        let stdout_thread = std::thread::spawn(move || {
            pump_stdout(BufReader::new(stdout), frame_bytes, total_bytes, &sender);
        });

        let capture = Arc::new(Mutex::new(StderrCapture::new(limits.max_stderr_bytes)));
        let capture_for_thread = Arc::clone(&capture);
        let stderr_thread = std::thread::spawn(move || {
            let mut source = stderr;
            let mut buffer = [0_u8; 4096];
            loop {
                match source.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if let Ok(mut capture) = capture_for_thread.lock() {
                            capture.push(&buffer[..read]);
                        }
                    }
                }
            }
        });

        Ok(FieldProcess {
            child,
            stdin,
            receiver,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            capture,
            limits,
            finished: false,
        })
    }
}

enum StdoutItem {
    Frame(RawFrame),
    End,
    Failed(FrameError),
}

fn pump_stdout<R: io::BufRead>(
    source: R,
    max_frame_bytes: u64,
    max_total_bytes: u64,
    sender: &SyncSender<StdoutItem>,
) {
    let mut reader = FrameReader::new(source, max_frame_bytes, max_total_bytes);
    loop {
        match reader.next_frame() {
            Ok(Some(frame)) => {
                if sender.send(StdoutItem::Frame(frame)).is_err() {
                    return;
                }
            }
            Ok(None) => {
                let _ = sender.send(StdoutItem::End);
                return;
            }
            Err(error) => {
                let _ = sender.send(StdoutItem::Failed(error));
                return;
            }
        }
    }
}

/// A running Field process.
#[derive(Debug)]
pub struct FieldProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    receiver: Receiver<StdoutItem>,
    stdout_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
    capture: Arc<Mutex<StderrCapture>>,
    limits: Limits,
    finished: bool,
}

impl FieldProcess {
    /// Writes one core frame to the child's standard input.
    pub fn send(&mut self, frame: &CoreFrame) -> Result<(), FrameError> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(FrameError {
                code: RejectionCode::ProtocolTruncatedFrame,
                detail: "the child's standard input is already closed".to_owned(),
            });
        };
        let mut writer = FrameWriter::new(stdin, self.limits.max_frame_bytes);
        writer.write_core_frame(frame).map(|_| ())
    }

    /// Closes the child's standard input, which is how a Field learns there is
    /// nothing more to read.
    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// Reads the next raw frame, bounded by the idle timeout.
    pub fn next_frame(&mut self, idle: Duration) -> Result<Option<RawFrame>, FrameError> {
        if self.finished {
            return Ok(None);
        }
        match self.receiver.recv_timeout(idle) {
            Ok(StdoutItem::Frame(frame)) => Ok(Some(frame)),
            Ok(StdoutItem::End) => {
                self.finished = true;
                Ok(None)
            }
            Ok(StdoutItem::Failed(error)) => {
                self.finished = true;
                Err(error)
            }
            Err(RecvTimeoutError::Timeout) => Err(FrameError {
                code: RejectionCode::ProtocolIdleTimeout,
                detail: format!(
                    "no frame and no artifact progress within {} seconds",
                    idle.as_secs()
                ),
            }),
            Err(RecvTimeoutError::Disconnected) => {
                self.finished = true;
                Ok(None)
            }
        }
    }

    /// Reads and decodes the next Field-to-core event, bounded by the idle
    /// timeout.
    pub fn next_event(&mut self, idle: Duration) -> Result<Option<FieldEvent>, FrameError> {
        match self.next_frame(idle)? {
            None => Ok(None),
            Some(frame) => FieldEvent::decode(frame.value)
                .map(Some)
                .map_err(FrameError::from),
        }
    }

    /// The redacted-on-read captured standard error.
    ///
    /// Core never persists raw standard error; a caller passes this through
    /// [`crate::redact::Redactor`] before display or persistence.
    #[must_use]
    pub fn captured_stderr(&self) -> String {
        self.capture
            .lock()
            .map(|capture| capture.to_lossy_string())
            .unwrap_or_default()
    }

    /// Whether captured standard error overflowed its ring buffer.
    #[must_use]
    pub fn stderr_truncated(&self) -> bool {
        self.capture
            .lock()
            .map(|capture| capture.truncated())
            .unwrap_or(false)
    }

    /// How many standard-error bytes were dropped.
    #[must_use]
    pub fn stderr_dropped_bytes(&self) -> u64 {
        self.capture
            .lock()
            .map(|capture| capture.dropped_bytes())
            .unwrap_or(0)
    }

    /// Waits for the child to end, up to `timeout`.
    ///
    /// A child still running at the timeout is terminated and reported as
    /// [`ExitObservation::Timeout`], because a hung connector must not hold an
    /// unbounded core run.
    pub fn wait(&mut self, timeout: Duration) -> io::Result<ExitObservation> {
        self.close_stdin();
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait()? {
                Some(status) => return Ok(classify(&status)),
                None if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Ok(ExitObservation::Timeout);
                }
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    /// Terminates the child, for a cancellation grace period that expired or a
    /// protocol violation.
    pub fn terminate(&mut self) -> io::Result<ExitObservation> {
        self.close_stdin();
        self.child.kill()?;
        self.child.wait()?;
        Ok(ExitObservation::TerminatedByCore)
    }

    /// Waits for the standard-error reader, so a caller can be sure the capture
    /// is complete before asserting on it.
    ///
    /// Safe to call once the child has ended: the thread returns at end of file.
    /// The standard-output reader is deliberately **not** joined here, because a
    /// caller that stopped consuming output after a protocol violation would
    /// otherwise wait for a thread that is blocked trying to hand over a frame
    /// nobody will take. That thread ends when this value is dropped and the
    /// receiver goes away.
    pub fn join_stderr(&mut self) {
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }

    /// Waits for both reader threads.
    ///
    /// Only safe when standard output was consumed to its end; otherwise use
    /// [`FieldProcess::join_stderr`].
    pub fn join_readers(&mut self) {
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        self.join_stderr();
    }
}

impl Drop for FieldProcess {
    fn drop(&mut self) {
        // A test that fails part way through must not leave a child running.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// Classifies a raw Windows exit code.
///
/// Extracted into a pure function, with no OS dependency, so its logic can be
/// exercised on any host rather than only when this crate is actually built
/// on Windows.
///
/// NTSTATUS "error"-severity codes have both of their two high bits set. A
/// process that ended by unhandled structured exception -- including the
/// exception `std::process::abort()` raises via `__fastfail`, `0xC0000409`
/// (`STATUS_STACK_BUFFER_OVERRUN`) -- surfaces here rather than through a
/// POSIX-style signal, and its value does not fit a `u8` the way an ordinary
/// exit code does.
#[cfg(any(windows, test))]
fn classify_windows_exit_code(raw: u32) -> ExitObservation {
    if raw & 0xC000_0000 == 0xC000_0000 {
        ExitObservation::WindowsAbnormalTermination(raw)
    } else {
        ExitObservation::Exited(u8::try_from(raw & 0xff).unwrap_or(0xff))
    }
}

fn classify(status: &std::process::ExitStatus) -> ExitObservation {
    if let Some(code) = status.code() {
        #[cfg(windows)]
        {
            return classify_windows_exit_code(code as u32);
        }
        #[cfg(not(windows))]
        {
            return ExitObservation::Exited(u8::try_from(code & 0xff).unwrap_or(0xff));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            // Core treats any signal termination as a failed run and normalizes
            // it rather than inventing a Field-level meaning for it.
            return ExitObservation::Signalled(u8::try_from(128 + (signal & 0x7f)).unwrap_or(0xff));
        }
    }
    ExitObservation::TerminatedByCore
}

/// Writes one frame to an arbitrary sink, for a Field writing to its own
/// standard output.
pub fn write_frame<W: Write>(
    sink: W,
    value: &serde_json::Value,
    max_frame_bytes: u64,
) -> Result<u64, FrameError> {
    FrameWriter::new(sink, max_frame_bytes).write_value(value)
}

/// Reads one core frame from an arbitrary source, for a Field reading its own
/// standard input.
pub fn read_core_frame<R: io::BufRead>(
    source: R,
    max_frame_bytes: u64,
) -> Result<Option<CoreFrame>, FrameError> {
    FrameReader::new(source, max_frame_bytes, u64::MAX).next_core_frame()
}

/// Whether `path` is usable as a pinned Field executable.
#[must_use]
pub fn is_pinned_executable(path: &Path) -> bool {
    path.is_absolute() && path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_executable_path_is_refused() {
        assert!(FieldSpawn::new("fieldnotes-field-fixture", Operation::Describe).is_err());
    }

    #[test]
    fn argv_carries_only_the_operation_token() -> io::Result<()> {
        #[cfg(windows)]
        let path = "C:\\fields\\fixture.exe";
        #[cfg(not(windows))]
        let path = "/opt/fields/fixture";
        let spawn = FieldSpawn::new(path, Operation::Collect)?;
        let argv = spawn.argv();
        assert_eq!(argv.len(), 2);
        assert_eq!(argv[1], "collect");
        Ok(())
    }

    #[test]
    fn the_environment_starts_from_an_allowlist_not_an_inheritance() -> io::Result<()> {
        #[cfg(windows)]
        let path = "C:\\fields\\fixture.exe";
        #[cfg(not(windows))]
        let path = "/opt/fields/fixture";
        let spawn = FieldSpawn::new(path, Operation::Describe)?;
        for name in spawn.environment().keys() {
            assert!(
                PLATFORM_ENVIRONMENT_ALLOWLIST.contains(&name.as_str()),
                "{name} is not on the platform allowlist"
            );
        }
        let with_extra = spawn.with_env("FIELDNOTES_FIXTURE_SCENARIO", "incremental");
        assert_eq!(
            with_extra
                .environment()
                .get("FIELDNOTES_FIXTURE_SCENARIO")
                .map(String::as_str),
            Some("incremental")
        );
        Ok(())
    }

    #[test]
    fn operation_tokens_are_the_two_v1_operations() {
        assert_eq!(Operation::Describe.as_str(), "describe");
        assert_eq!(Operation::Collect.as_str(), "collect");
    }

    #[test]
    fn windows_abnormal_termination_carries_the_full_code_rather_than_a_narrowed_u8() {
        // 0xC0000409 is STATUS_STACK_BUFFER_OVERRUN, what `std::process::abort()`
        // surfaces as on Windows. Its low byte alone, 0x09, would misread as
        // ordinary exit code 9 ("configuration invalid") if ever narrowed.
        match classify_windows_exit_code(0xC000_0409) {
            ExitObservation::WindowsAbnormalTermination(code) => {
                assert_eq!(code, 0xC000_0409);
            }
            other => {
                panic!("an NTSTATUS error code must not be read as an ordinary exit: {other:?}")
            }
        }
        // An ordinary exit code in the low byte, with neither high bit set,
        // classifies normally.
        assert_eq!(classify_windows_exit_code(0), ExitObservation::Exited(0));
        assert_eq!(classify_windows_exit_code(9), ExitObservation::Exited(9));
    }
}
