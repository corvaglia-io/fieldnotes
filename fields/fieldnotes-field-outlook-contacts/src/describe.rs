//! The `describe` operation: version negotiation and this Field's manifest.

use std::io::BufRead;
use std::process::ExitCode;

use fieldnotes_field_protocol::codes::ExitCode as ProtocolExit;
use fieldnotes_field_protocol::limits::Limits;
use fieldnotes_field_protocol::version::{PROTOCOL_VERSION, select_version};
use fieldnotes_field_sdk::dispatch::read_describe_request;
use fieldnotes_field_sdk::emit::Emitter;

/// Runs the describe operation: reads the one `describe_request`, negotiates
/// a shared protocol version, and answers with this Field's manifest.
///
/// A Field that supports no version core offered emits no manifest at all --
/// a manifest it cannot express correctly is worse than none -- and exits
/// with the negotiation code (A2 section 2).
pub(crate) fn run(mut input: impl BufRead) -> ExitCode {
    let request = match read_describe_request(
        &mut input,
        Limits::ceilings().max_frame_bytes,
        "fieldnotes-field-outlook-contacts",
    ) {
        Ok(request) => request,
        Err(code) => return ExitCode::from(code),
    };

    if select_version(
        request.supported_protocol_versions.as_slice(),
        &[PROTOCOL_VERSION],
    )
    .is_none()
    {
        crate::report(&format!(
            "fieldnotes-field-outlook-contacts: protocol version mismatch: core offered {:?}, \
             this build supports [{PROTOCOL_VERSION}]. Upgrade Fieldnotes or install a matching \
             Field build.",
            request.supported_protocol_versions.as_slice()
        ));
        return ExitCode::from(ProtocolExit::Negotiation.as_raw());
    }
    let manifest = crate::manifest::build(request.run_id.clone());
    // A describe run almost never states limits, since it has almost nothing
    // to bound; fall back to the frozen ceiling when core omitted them.
    let max_frame_bytes = request
        .limits
        .map_or(Limits::ceilings().max_frame_bytes, |limits| {
            limits.max_frame_bytes
        });
    let limits = Limits {
        max_frame_bytes,
        ..Limits::ceilings()
    };
    let mut emitter = Emitter::new(std::io::stdout(), limits);
    if emitter.manifest(manifest) {
        ExitCode::from(ProtocolExit::Completed.as_raw())
    } else {
        let detail = emitter.last_write_error().map_or_else(
            || "the manifest could not be written".to_owned(),
            |error| error.to_string(),
        );
        crate::report(&format!("fieldnotes-field-outlook-contacts: {detail}"));
        ExitCode::from(ProtocolExit::Internal.as_raw())
    }
}
