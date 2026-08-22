//! The `describe` operation: version negotiation and this Field's manifest.

use std::io::BufRead;
use std::process::ExitCode;

use fieldnotes_field_protocol::codes::ExitCode as ProtocolExit;
use fieldnotes_field_protocol::framing::FrameWriter;
use fieldnotes_field_protocol::host::read_core_frame;
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::message::{CoreFrame, FieldEvent};
use fieldnotes_field_protocol::version::{PROTOCOL_VERSION, select_version};

/// Runs the describe operation: reads the one `describe_request`, negotiates
/// a shared protocol version, and answers with this Field's manifest.
///
/// A Field that supports no version core offered emits no manifest at all --
/// a manifest it cannot express correctly is worse than none -- and exits
/// with the negotiation code (A2 section 2).
pub(crate) fn run(mut input: impl BufRead) -> ExitCode {
    match read_core_frame(&mut input, Limits::ceilings().max_frame_bytes) {
        Ok(Some(CoreFrame::Describe(request))) => {
            if select_version(
                request.supported_protocol_versions.as_slice(),
                &[PROTOCOL_VERSION],
            )
            .is_none()
            {
                crate::report(&format!(
                    "fieldnotes-field-local: protocol version mismatch: core offered {:?}, this \
                     build supports [{PROTOCOL_VERSION}]. Upgrade Fieldnotes or install a \
                     matching Field build.",
                    request.supported_protocol_versions.as_slice()
                ));
                return ExitCode::from(ProtocolExit::Negotiation.as_raw());
            }
            let manifest = crate::manifest::build(request.run_id.clone());
            // A describe run almost never states limits, since it has
            // almost nothing to bound; fall back to the frozen ceiling when
            // core omitted them.
            let max_frame_bytes = request
                .limits
                .map_or(Limits::ceilings().max_frame_bytes, |limits| {
                    limits.max_frame_bytes
                });
            let mut writer = FrameWriter::new(std::io::stdout(), max_frame_bytes);
            match writer.write_event(&FieldEvent::Manifest(Box::new(manifest))) {
                Ok(_) => ExitCode::from(ProtocolExit::Completed.as_raw()),
                Err(error) => {
                    crate::report(&format!("fieldnotes-field-local: {error}"));
                    ExitCode::from(ProtocolExit::Internal.as_raw())
                }
            }
        }
        Ok(Some(_)) => {
            crate::report(
                "fieldnotes-field-local: a describe run expects exactly one describe_request on \
                 standard input",
            );
            ExitCode::from(ProtocolExit::Usage.as_raw())
        }
        Ok(None) => {
            crate::report(
                "fieldnotes-field-local: standard input closed before any request arrived",
            );
            ExitCode::from(ProtocolExit::Usage.as_raw())
        }
        Err(error) => {
            crate::report(&format!(
                "fieldnotes-field-local: the describe request did not validate: {error}"
            ));
            ExitCode::from(ProtocolExit::Usage.as_raw())
        }
    }
}
