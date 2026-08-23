//! Argv parsing and request-frame reading shared by every Field's `main`.
//!
//! Protocol v1 gives every Field executable exactly the same shape: one argv
//! token selecting `describe` or `collect`, then exactly one matching request
//! frame on standard input before anything else. Every conforming Field
//! reports the same handful of structural failures the same way -- an
//! unrecognized or missing operation token, standard input closed before a
//! request arrived, a request of the wrong frame type, or a request that
//! fails its own schema -- so this module states that boilerplate once
//! rather than once per Field.
//!
//! None of it is protocol-*content* logic: [`fieldnotes_field_protocol`]
//! still owns frame decoding, schema validation, and every wire type. This
//! module only owns the small amount of glue between a process's `argv` and
//! `stdin` and that decoding.
//!
//! # Why the failure case is a raw `u8`, not a [`std::process::ExitCode`]
//!
//! [`std::process::ExitCode`] is intentionally opaque: nothing can read the
//! numeric value back out of one. A Field that exits immediately on a
//! structural failure does not need to -- it only ever wraps the code once,
//! at the point of return -- but a Field like the fixture Field, which lets
//! an environment variable override the exit code a scenario would otherwise
//! produce, needs the numeric value in hand until the very end of `main`. A
//! raw `u8` serves both: wrap it in [`std::process::ExitCode::from`]
//! immediately, or thread it through further logic first.

use std::io::BufRead;

use fieldnotes_field_protocol::codes::ExitCode as ProtocolExitCode;
use fieldnotes_field_protocol::host::{Operation, read_core_frame};
use fieldnotes_field_protocol::message::{CollectRequest, CoreFrame, DescribeRequest};

/// Writes one line to standard error, then flushes it.
///
/// Standard output carries protocol frames and nothing else, so every log
/// line -- a Field's own, or one of this module's -- goes to standard error
/// instead. Flushing immediately means a reader on the other side never
/// waits on a buffer for a line that was already written.
pub fn report(message: &str) {
    use std::io::Write;
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "{message}");
    let _ = stderr.flush();
}

/// Parses `arguments` (a process's `argv`, excluding `argv[0]`) into the one
/// operation token protocol v1 admits.
///
/// Reports a usage error, prefixed with `field_name`, to standard error and
/// returns the exit code to use on anything else: zero tokens, more than one,
/// or a single token that is not `describe` or `collect`.
pub fn parse_operation(arguments: &[String], field_name: &str) -> Result<Operation, u8> {
    match arguments {
        [token] if token == Operation::Describe.as_str() => Ok(Operation::Describe),
        [token] if token == Operation::Collect.as_str() => Ok(Operation::Collect),
        [token] => {
            report(&format!(
                "{field_name}: unknown operation {token:?}; protocol v1 has exactly two, \
                 'describe' and 'collect'"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
        _ => {
            report(&format!(
                "{field_name}: exactly one operation token is expected, either 'describe' or \
                 'collect'"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
    }
}

/// Reads the one `describe_request` a describe run expects from `input`.
///
/// Reports a usage error, prefixed with `field_name`, and returns the exit
/// code to use when: a different frame type arrived, standard input closed
/// before anything arrived, or the frame failed its own schema.
pub fn read_describe_request<R: BufRead>(
    input: R,
    max_frame_bytes: u64,
    field_name: &str,
) -> Result<DescribeRequest, u8> {
    match read_core_frame(input, max_frame_bytes) {
        Ok(Some(CoreFrame::Describe(request))) => Ok(*request),
        Ok(Some(_)) => {
            report(&format!(
                "{field_name}: a describe run expects exactly one describe_request on standard \
                 input"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
        Ok(None) => {
            report(&format!(
                "{field_name}: standard input closed before any request arrived"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
        Err(error) => {
            report(&format!(
                "{field_name}: the describe request did not validate: {error}"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
    }
}

/// Reads the one `collect_request` a collect run expects from `input`,
/// before any other frame.
///
/// Reports a usage error, prefixed with `field_name`, and returns the exit
/// code to use when: a different frame type arrived, standard input closed
/// before anything arrived, or the frame failed its own schema.
pub fn read_collect_request<R: BufRead>(
    input: R,
    max_frame_bytes: u64,
    field_name: &str,
) -> Result<CollectRequest, u8> {
    match read_core_frame(input, max_frame_bytes) {
        Ok(Some(CoreFrame::Collect(request))) => Ok(*request),
        Ok(Some(_)) => {
            report(&format!(
                "{field_name}: a collect run expects a collect_request on standard input first"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
        Ok(None) => {
            report(&format!(
                "{field_name}: standard input closed before any request arrived"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
        Err(error) => {
            report(&format!(
                "{field_name}: the collection request did not validate: {error}"
            ));
            Err(ProtocolExitCode::Usage.as_raw())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_operation, read_collect_request, read_describe_request};
    use fieldnotes_field_protocol::host::Operation;
    use std::io::Cursor;

    #[test]
    fn a_recognized_single_token_selects_its_operation() {
        assert!(matches!(
            parse_operation(&["describe".to_owned()], "field"),
            Ok(Operation::Describe)
        ));
        assert!(matches!(
            parse_operation(&["collect".to_owned()], "field"),
            Ok(Operation::Collect)
        ));
    }

    #[test]
    fn an_unrecognized_token_is_a_usage_error() {
        assert!(parse_operation(&["sync".to_owned()], "field").is_err());
    }

    #[test]
    fn zero_or_many_tokens_is_a_usage_error() {
        assert!(parse_operation(&[], "field").is_err());
        assert!(parse_operation(&["describe".to_owned(), "extra".to_owned()], "field").is_err());
    }

    #[test]
    fn closed_input_before_any_request_is_a_usage_error() {
        let input = Cursor::new(Vec::new());
        assert!(read_describe_request(input, 4096, "field").is_err());
        let input = Cursor::new(Vec::new());
        assert!(read_collect_request(input, 4096, "field").is_err());
    }

    #[test]
    fn a_request_of_the_wrong_type_is_a_usage_error() {
        let describe = serde_json::json!({
            "v": 1,
            "type": "describe_request",
            "run_id": "1a4c9f2e-0000-4000-8000-000000000001",
            "supported_protocol_versions": [1]
        });
        let mut line = serde_json::to_vec(&describe).unwrap_or_default();
        line.push(b'\n');
        // A collect run fed a describe_request must be refused as the wrong
        // frame type, not silently accepted.
        let input = Cursor::new(line);
        assert!(read_collect_request(input, 4096, "field").is_err());
    }
}
