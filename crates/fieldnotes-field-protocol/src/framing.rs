//! Newline-delimited JSON framing, with a hard byte ceiling, and the bounded
//! standard-error ring buffer.
//!
//! Framing is one JSON object per line, UTF-8 without a BOM, terminated by one
//! LF. There is no envelope wrapping the stream and no length prefix.
//!
//! # Reading is bounded, not optimistic
//!
//! Core must scan for LF with a hard byte bound rather than reading whole lines
//! optimistically, because a Field can emit a line longer than memory.
//! [`FrameReader`] stops at the ceiling and **never buffers the remainder** of
//! an oversized frame: it reports [`RejectionCode::ProtocolOversizedFrame`] and
//! is poisoned, because any protocol violation fails the run anyway.
//!
//! A Field must not write anything but frames to standard output. A stray
//! `print` is a failed run, which is exactly why logs go to standard error.

use std::io::{BufRead, Write};

use crate::codes::RejectionCode;
use crate::message::{CoreFrame, CredentialFrame, FieldEvent, SchemaError};

/// A framing-level failure, with the code core rejects the run with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    /// The rejection code core reports.
    pub code: RejectionCode,
    /// Why, in reviewable terms.
    pub detail: String,
}

impl FrameError {
    fn new(code: RejectionCode, detail: impl Into<String>) -> Self {
        FrameError {
            code,
            detail: detail.into(),
        }
    }
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for FrameError {}

impl From<SchemaError> for FrameError {
    fn from(error: SchemaError) -> Self {
        FrameError {
            code: error.code,
            detail: error.message,
        }
    }
}

/// One raw frame: the JSON value and the byte length of the line including its
/// terminating LF.
#[derive(Debug, Clone, PartialEq)]
pub struct RawFrame {
    /// The decoded JSON value.
    pub value: serde_json::Value,
    /// The wire length of the line, including its LF.
    pub wire_bytes: u64,
}

/// A bounded newline-delimited JSON reader over a Field's standard output.
#[derive(Debug)]
pub struct FrameReader<R> {
    source: R,
    max_frame_bytes: usize,
    max_total_bytes: u64,
    total_bytes: u64,
    poisoned: bool,
}

impl<R: BufRead> FrameReader<R> {
    /// Wraps `source` with a per-frame and a per-run byte ceiling.
    #[must_use]
    pub fn new(source: R, max_frame_bytes: u64, max_total_bytes: u64) -> Self {
        FrameReader {
            source,
            max_frame_bytes: usize::try_from(max_frame_bytes).unwrap_or(usize::MAX),
            max_total_bytes,
            total_bytes: 0,
            poisoned: false,
        }
    }

    /// The bytes read from standard output so far.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Reads the next frame.
    ///
    /// `Ok(None)` is a clean end of stream at a frame boundary. A stream that
    /// ends mid-line is [`RejectionCode::ProtocolTruncatedFrame`], not a
    /// partially parsed frame.
    pub fn next_frame(&mut self) -> Result<Option<RawFrame>, FrameError> {
        if self.poisoned {
            return Err(FrameError::new(
                RejectionCode::ProtocolUnexpectedOrder,
                "the frame reader stopped consuming output after a protocol violation",
            ));
        }
        let line = match self.read_line_bounded() {
            Ok(Some(line)) => line,
            Ok(None) => return Ok(None),
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        };
        let wire_bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
        self.total_bytes = self.total_bytes.saturating_add(wire_bytes);
        if self.total_bytes > self.max_total_bytes {
            self.poisoned = true;
            return Err(FrameError::new(
                RejectionCode::ProtocolLimitExceeded,
                format!(
                    "standard output reached {} bytes, past the run's {} byte bound",
                    self.total_bytes, self.max_total_bytes
                ),
            ));
        }
        let text = match std::str::from_utf8(&line) {
            Ok(text) => text,
            Err(error) => {
                self.poisoned = true;
                return Err(FrameError::new(
                    RejectionCode::ProtocolInvalidUtf8,
                    format!(
                        "frame bytes are not valid UTF-8 at offset {}",
                        error.valid_up_to()
                    ),
                ));
            }
        };
        // A blank line is not a frame; it is output a Field should not have
        // written, and v1 has no permissive branch for it.
        let value: serde_json::Value = match serde_json::from_str(text.trim_end_matches('\n')) {
            Ok(value) => value,
            Err(error) => {
                self.poisoned = true;
                return Err(FrameError::new(
                    RejectionCode::ProtocolNotJson,
                    format!("frame is not JSON: {error}"),
                ));
            }
        };
        Ok(Some(RawFrame { value, wire_bytes }))
    }

    /// Reads and decodes the next Field-to-core event.
    pub fn next_event(&mut self) -> Result<Option<(FieldEvent, u64)>, FrameError> {
        match self.next_frame()? {
            None => Ok(None),
            Some(frame) => match FieldEvent::decode(frame.value) {
                Ok(event) => Ok(Some((event, frame.wire_bytes))),
                Err(error) => {
                    self.poisoned = true;
                    Err(error.into())
                }
            },
        }
    }

    /// Reads and decodes the next core-to-Field frame, from a Field's side.
    pub fn next_core_frame(&mut self) -> Result<Option<CoreFrame>, FrameError> {
        match self.next_frame()? {
            None => Ok(None),
            Some(frame) => match CoreFrame::decode(frame.value) {
                Ok(decoded) => Ok(Some(decoded)),
                Err(error) => {
                    self.poisoned = true;
                    Err(error.into())
                }
            },
        }
    }

    /// Reads and decodes the next protected-channel frame.
    pub fn next_credential_frame(&mut self) -> Result<Option<CredentialFrame>, FrameError> {
        match self.next_frame()? {
            None => Ok(None),
            Some(frame) => match CredentialFrame::decode(frame.value) {
                Ok(decoded) => Ok(Some(decoded)),
                Err(error) => {
                    self.poisoned = true;
                    Err(error.into())
                }
            },
        }
    }

    /// Scans for LF without ever holding more than the frame ceiling.
    fn read_line_bounded(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        let mut line: Vec<u8> = Vec::new();
        loop {
            let available = match self.source.fill_buf() {
                Ok(available) => available,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return Err(FrameError::new(
                        RejectionCode::ProtocolTruncatedFrame,
                        format!("standard output could not be read: {error}"),
                    ));
                }
            };
            if available.is_empty() {
                if line.is_empty() {
                    return Ok(None);
                }
                return Err(FrameError::new(
                    RejectionCode::ProtocolTruncatedFrame,
                    "standard output ended in the middle of a frame, so the final frame never \
                     terminated; core rejects it rather than parsing as far as it got",
                ));
            }
            // Only look at as much as the ceiling still permits, so an
            // over-long line is refused without the remainder ever being
            // copied into the frame buffer.
            let remaining = self.max_frame_bytes.saturating_sub(line.len());
            let horizon = available.len().min(remaining.saturating_add(1));
            let window = &available[..horizon];
            match window.iter().position(|byte| *byte == b'\n') {
                Some(index) => {
                    if line.len().saturating_add(index + 1) > self.max_frame_bytes {
                        return Err(FrameError::new(
                            RejectionCode::ProtocolOversizedFrame,
                            format!(
                                "a frame of {} bytes including its LF exceeds the {} byte ceiling",
                                line.len() + index + 1,
                                self.max_frame_bytes
                            ),
                        ));
                    }
                    line.extend_from_slice(&window[..=index]);
                    self.source.consume(index + 1);
                    return Ok(Some(line));
                }
                None => {
                    if line.len().saturating_add(window.len()) > self.max_frame_bytes {
                        return Err(FrameError::new(
                            RejectionCode::ProtocolOversizedFrame,
                            format!(
                                "a frame exceeded the {} byte ceiling before its terminating LF; \
                                 core stops reading the line at the limit and never buffers the \
                                 remainder",
                                self.max_frame_bytes
                            ),
                        ));
                    }
                    line.extend_from_slice(window);
                    self.source.consume(horizon);
                }
            }
        }
    }
}

/// A newline-delimited JSON writer.
///
/// Every frame is one line terminated by exactly one LF, flushed immediately so
/// a reader on the other side is never left waiting on a buffer.
#[derive(Debug)]
pub struct FrameWriter<W> {
    sink: W,
    max_frame_bytes: usize,
}

impl<W: Write> FrameWriter<W> {
    /// Wraps `sink` with a per-frame byte ceiling.
    #[must_use]
    pub fn new(sink: W, max_frame_bytes: u64) -> Self {
        FrameWriter {
            sink,
            max_frame_bytes: usize::try_from(max_frame_bytes).unwrap_or(usize::MAX),
        }
    }

    /// Writes one frame, refusing to emit a frame past the ceiling so a
    /// well-behaved Field self-polices rather than being killed.
    pub fn write_value(&mut self, value: &serde_json::Value) -> Result<u64, FrameError> {
        let mut encoded = serde_json::to_vec(value).map_err(|error| {
            FrameError::new(
                RejectionCode::ProtocolNotJson,
                format!("frame could not be encoded: {error}"),
            )
        })?;
        encoded.push(b'\n');
        if encoded.len() > self.max_frame_bytes {
            return Err(FrameError::new(
                RejectionCode::ProtocolOversizedFrame,
                format!(
                    "the encoded frame is {} bytes, past the {} byte ceiling",
                    encoded.len(),
                    self.max_frame_bytes
                ),
            ));
        }
        self.sink.write_all(&encoded).map_err(|error| {
            FrameError::new(
                RejectionCode::ProtocolTruncatedFrame,
                format!("frame could not be written: {error}"),
            )
        })?;
        self.sink.flush().map_err(|error| {
            FrameError::new(
                RejectionCode::ProtocolTruncatedFrame,
                format!("frame could not be flushed: {error}"),
            )
        })?;
        Ok(u64::try_from(encoded.len()).unwrap_or(u64::MAX))
    }

    /// Writes one Field-to-core event.
    pub fn write_event(&mut self, event: &FieldEvent) -> Result<u64, FrameError> {
        let value = event.to_json().map_err(|error| {
            FrameError::new(
                RejectionCode::ProtocolNotJson,
                format!("event could not be encoded: {error}"),
            )
        })?;
        self.write_value(&value)
    }

    /// Writes one core-to-Field frame.
    pub fn write_core_frame(&mut self, frame: &CoreFrame) -> Result<u64, FrameError> {
        let value = frame.to_json().map_err(|error| {
            FrameError::new(
                RejectionCode::ProtocolNotJson,
                format!("frame could not be encoded: {error}"),
            )
        })?;
        self.write_value(&value)
    }

    /// Writes one protected-channel frame.
    pub fn write_credential_frame(&mut self, frame: &CredentialFrame) -> Result<u64, FrameError> {
        let value = frame.to_json().map_err(|error| {
            FrameError::new(
                RejectionCode::ProtocolNotJson,
                format!("frame could not be encoded: {error}"),
            )
        })?;
        self.write_value(&value)
    }
}

/// A bounded ring buffer for a Field's standard error.
///
/// Core never persists raw standard error. It captures it here, redacts it, and
/// notes truncation. Overflowing the buffer is
/// [`RejectionCode::ProtocolStderrFlood`] only when a caller chooses to treat it
/// that way; the capture itself simply keeps the most recent bytes, so a noisy
/// connector cannot hold an unbounded core run.
#[derive(Debug, Clone)]
pub struct StderrCapture {
    capacity: usize,
    buffer: std::collections::VecDeque<u8>,
    dropped: u64,
}

impl StderrCapture {
    /// A capture holding at most `capacity` bytes.
    #[must_use]
    pub fn new(capacity: u64) -> Self {
        StderrCapture {
            capacity: usize::try_from(capacity).unwrap_or(usize::MAX),
            buffer: std::collections::VecDeque::new(),
            dropped: 0,
        }
    }

    /// Appends bytes, discarding the oldest when the buffer is full.
    pub fn push(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.buffer.len() == self.capacity {
                self.buffer.pop_front();
                self.dropped = self.dropped.saturating_add(1);
            }
            if self.capacity > 0 {
                self.buffer.push_back(*byte);
            } else {
                self.dropped = self.dropped.saturating_add(1);
            }
        }
    }

    /// How many bytes were discarded because the buffer was full.
    #[must_use]
    pub fn dropped_bytes(&self) -> u64 {
        self.dropped
    }

    /// Whether anything was discarded.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.dropped > 0
    }

    /// The captured bytes as lossy UTF-8.
    ///
    /// Lossy because the ring buffer may have cut a multi-byte character, and a
    /// log line is evidence rather than protocol.
    #[must_use]
    pub fn to_lossy_string(&self) -> String {
        let bytes: Vec<u8> = self.buffer.iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(input: &str, max_frame: u64) -> FrameReader<std::io::Cursor<Vec<u8>>> {
        FrameReader::new(
            std::io::Cursor::new(input.as_bytes().to_vec()),
            max_frame,
            1 << 30,
        )
    }

    #[test]
    fn reads_one_object_per_line() -> Result<(), FrameError> {
        let mut reader = reader("{\"a\":1}\n{\"b\":2}\n", 4096);
        let first = reader.next_frame()?;
        assert_eq!(first.map(|frame| frame.wire_bytes), Some(8));
        assert!(reader.next_frame()?.is_some());
        assert!(reader.next_frame()?.is_none());
        Ok(())
    }

    #[test]
    fn a_non_json_line_is_not_json() {
        let mut reader = reader("Traceback (most recent call last): boom\n", 4096);
        match reader.next_frame() {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolNotJson),
            Ok(_) => panic!("a traceback on standard output is a failed run"),
        }
    }

    #[test]
    fn a_stream_ending_mid_line_is_truncated_not_parsed() {
        let mut reader = reader("{\"v\":1,\"type\":\"checkpoint\",\"seq\":9,\"cur", 4096);
        match reader.next_frame() {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolTruncatedFrame),
            Ok(_) => panic!("an unterminated frame must be rejected"),
        }
    }

    #[test]
    fn an_oversized_frame_is_refused_without_buffering_the_remainder() {
        let padding = "A".repeat(4096);
        let line = format!("{{\"v\":1,\"body\":\"{padding}\"}}\n");
        let mut reader = reader(&line, 512);
        match reader.next_frame() {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolOversizedFrame),
            Ok(_) => panic!("a frame past the ceiling must be rejected"),
        }
    }

    #[test]
    fn a_frame_exactly_at_the_ceiling_is_accepted() -> Result<(), FrameError> {
        // 15 bytes of JSON plus the LF is exactly 16.
        let mut reader = reader("{\"a\":\"0123456\"}\n", 16);
        assert!(reader.next_frame()?.is_some());
        Ok(())
    }

    #[test]
    fn invalid_utf8_is_rejected_as_such() {
        let mut bytes = b"{\"v\":1,\"type\":\"record\",\"body\":\"".to_vec();
        bytes.extend_from_slice(&[0xc3, 0x28]);
        bytes.extend_from_slice(b"\"}\n");
        let mut reader = FrameReader::new(std::io::Cursor::new(bytes), 4096, 1 << 30);
        match reader.next_frame() {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolInvalidUtf8),
            Ok(_) => panic!("invalid UTF-8 must be rejected"),
        }
    }

    #[test]
    fn the_reader_stops_consuming_after_a_violation() {
        let mut reader = reader("nonsense\n{\"a\":1}\n", 4096);
        assert!(reader.next_frame().is_err());
        match reader.next_frame() {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolUnexpectedOrder),
            Ok(_) => panic!("the reader must stay poisoned"),
        }
    }

    #[test]
    fn the_per_run_output_bound_is_enforced() {
        let mut reader = FrameReader::new(
            std::io::Cursor::new(b"{\"a\":1}\n{\"b\":2}\n".to_vec()),
            4096,
            8,
        );
        assert!(reader.next_frame().is_ok());
        match reader.next_frame() {
            Err(error) => assert_eq!(error.code, RejectionCode::ProtocolLimitExceeded),
            Ok(_) => panic!("the per-run output bound must be enforced"),
        }
    }

    #[test]
    fn the_writer_refuses_a_frame_past_the_ceiling() -> Result<(), FrameError> {
        let mut sink = Vec::new();
        let mut writer = FrameWriter::new(&mut sink, 16);
        let small = serde_json::json!({ "a": 1 });
        assert_eq!(writer.write_value(&small)?, 8);
        let large = serde_json::json!({ "a": "0123456789012345678901234567890" });
        assert!(writer.write_value(&large).is_err());
        Ok(())
    }

    #[test]
    fn stderr_capture_keeps_the_most_recent_bytes_and_notes_truncation() {
        let mut capture = StderrCapture::new(8);
        capture.push(b"0123456789");
        assert!(capture.truncated());
        assert_eq!(capture.dropped_bytes(), 2);
        assert_eq!(capture.to_lossy_string(), "23456789");
    }
}
