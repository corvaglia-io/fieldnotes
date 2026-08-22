//! The checked-in transcripts, used as test vectors.
//!
//! The strongest evidence available before A2 is approved is that the schemas,
//! the transcripts, and the implementation all agree. These tests read
//! `tests/fixtures/protocol/proposed-v1/transcripts/` and assert:
//!
//! - every transcript parses against the fixture transcript format;
//! - every `expect_reject` and every core `error_code` names a code that exists
//!   in the **closed** v1 rejection vocabulary;
//! - a frame marked `valid: true` decodes and validates, and a frame marked
//!   `valid: false` does not;
//! - when a frame that fails to decode names an `expect_reject`, the code the
//!   implementation produces is **exactly** the code the transcript names;
//! - every valid frame round-trips: re-encoding the decoded data-transfer object
//!   reproduces the original JSON, so no member is silently dropped;
//! - per-run sequence numbers start at 1 and increase by exactly 1, excluding
//!   frames rejected with a `protocol.` code, whose sequence numbers are by
//!   definition untrustworthy;
//! - each checkpoint's `records_covered` equals the record frames actually
//!   present in the range it covers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fieldnotes_field_protocol::codes::RejectionCode;
use fieldnotes_field_protocol::conformance::{Channel, Transcript, TranscriptLine};
use fieldnotes_field_protocol::message::{CoreFrame, CredentialFrame, FieldEvent};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("protocol")
        .join("proposed-v1")
        .join("transcripts")
}

fn transcripts() -> Vec<Transcript> {
    match Transcript::load_directory(&corpus()) {
        Ok(loaded) => loaded,
        Err(error) => panic!("the proposed protocol corpus must parse: {error}"),
    }
}

/// Decodes one frame line through the union its direction and channel select,
/// returning the rejection code on failure.
fn decode(
    line: &fieldnotes_field_protocol::conformance::FrameLine,
) -> Result<String, RejectionCode> {
    let value = line.frame.clone();
    let outcome = match line.channel {
        Channel::Credential => CredentialFrame::decode(value).and_then(|frame| {
            frame.to_json().map_err(|error| {
                fieldnotes_field_protocol::message::SchemaError::invalid(error.to_string())
            })
        }),
        Channel::Stdin => CoreFrame::decode(value).and_then(|frame| {
            frame.to_json().map_err(|error| {
                fieldnotes_field_protocol::message::SchemaError::invalid(error.to_string())
            })
        }),
        Channel::Stdout => FieldEvent::decode(value).and_then(|event| {
            event.to_json().map_err(|error| {
                fieldnotes_field_protocol::message::SchemaError::invalid(error.to_string())
            })
        }),
        Channel::Stderr => panic!("a frame line never travels on standard error"),
    };
    match outcome {
        Ok(reencoded) => Ok(serde_json::to_string(&reencoded).unwrap_or_default()),
        Err(error) => Err(error.code),
    }
}

#[test]
fn every_transcript_in_the_corpus_parses() {
    let loaded = transcripts();
    assert_eq!(
        loaded.len(),
        16,
        "the corpus README lists sixteen transcripts"
    );
    for transcript in &loaded {
        let header = transcript
            .header()
            .unwrap_or_else(|| panic!("{}: the first line is the header", transcript.name));
        assert_eq!(header.line, "header");
        assert_eq!(
            header.transcript, transcript.name,
            "a transcript names itself"
        );
        let extra_headers = transcript
            .lines
            .iter()
            .skip(1)
            .filter(|line| matches!(line, TranscriptLine::Header(_)))
            .count();
        assert_eq!(extra_headers, 0, "{}: one header only", transcript.name);
    }
}

#[test]
fn every_named_rejection_code_exists_in_the_closed_vocabulary() {
    for transcript in transcripts() {
        for frame in transcript.frames() {
            if let Some(parsed) = frame.expected_rejection() {
                assert!(
                    parsed.is_some(),
                    "{}: expect_reject {:?} is not in the closed v1 vocabulary",
                    transcript.name,
                    frame.expect_reject
                );
            }
        }
        for raw in transcript.raw_lines() {
            if let Some(parsed) = raw.expected_rejection() {
                assert!(
                    parsed.is_some(),
                    "{}: expect_reject {:?} is not in the closed v1 vocabulary",
                    transcript.name,
                    raw.expect_reject
                );
            }
        }
        for core in transcript.core_lines() {
            if let Some(parsed) = core.rejection() {
                assert!(
                    parsed.is_some(),
                    "{}: error_code {:?} is not in the closed v1 vocabulary",
                    transcript.name,
                    core.error_code
                );
            }
        }
    }
}

#[test]
fn a_frame_marked_valid_decodes_and_one_marked_invalid_does_not() {
    let mut valid = 0_usize;
    let mut invalid = 0_usize;
    for transcript in transcripts() {
        for frame in transcript.frames() {
            match (frame.valid, decode(frame)) {
                (true, Ok(_)) => valid += 1,
                (false, Err(_)) => invalid += 1,
                (true, Err(code)) => panic!(
                    "{}: frame {:?} is marked valid but the implementation rejects it as {code}",
                    transcript.name,
                    frame.frame_type()
                ),
                (false, Ok(_)) => panic!(
                    "{}: frame {:?} is marked invalid but the implementation accepts it",
                    transcript.name,
                    frame.frame_type()
                ),
            }
        }
    }
    assert!(valid > 30, "the corpus exercises many valid frames");
    assert!(invalid >= 4, "the corpus exercises the negative cases too");
}

#[test]
fn a_frame_rejected_at_decode_uses_exactly_the_code_the_transcript_names() {
    let mut checked = 0_usize;
    for transcript in transcripts() {
        for frame in transcript.frames() {
            if frame.valid {
                continue;
            }
            let Some(Some(expected)) = frame.expected_rejection() else {
                continue;
            };
            match decode(frame) {
                Err(actual) => {
                    assert_eq!(
                        actual,
                        expected,
                        "{}: frame {:?} must be rejected as {expected}, not {actual}",
                        transcript.name,
                        frame.frame_type()
                    );
                    checked += 1;
                }
                Ok(_) => panic!(
                    "{}: frame {:?} must be rejected",
                    transcript.name,
                    frame.frame_type()
                ),
            }
        }
    }
    assert!(
        checked >= 4,
        "the corpus pins at least four decode-time rejection codes"
    );
}

#[test]
fn every_valid_frame_round_trips_without_losing_a_member() {
    for transcript in transcripts() {
        for frame in transcript.frames() {
            if !frame.valid {
                continue;
            }
            let Ok(reencoded) = decode(frame) else {
                panic!(
                    "{}: a valid frame must decode: {:?}",
                    transcript.name,
                    frame.frame_type()
                )
            };
            let parsed: serde_json::Value = match serde_json::from_str(&reencoded) {
                Ok(value) => value,
                Err(error) => panic!("{}: re-encoding is not JSON: {error}", transcript.name),
            };
            assert_eq!(
                parsed,
                frame.frame,
                "{}: frame {:?} lost or changed a member on round trip",
                transcript.name,
                frame.frame_type()
            );
        }
    }
}

#[test]
fn per_run_sequence_numbers_start_at_one_and_increase_by_exactly_one() {
    for transcript in transcripts() {
        // Keyed by run, because one transcript may show two runs.
        let mut last: BTreeMap<String, u64> = BTreeMap::new();
        for frame in transcript.frames() {
            if frame.channel != Channel::Stdout {
                continue;
            }
            // A frame rejected with a `protocol.` code has an untrustworthy
            // sequence number, so it is excluded from continuity.
            if let Some(Some(code)) = frame.expected_rejection()
                && code.is_transport_level()
            {
                continue;
            }
            let Some(seq) = frame.frame.get("seq").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let run = frame
                .frame
                .get("run_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let expected = last.get(&run).map_or(1, |previous| previous + 1);
            assert_eq!(
                seq, expected,
                "{}: run {run} expected seq {expected}, found {seq}",
                transcript.name
            );
            last.insert(run, seq);
        }
    }
}

#[test]
fn checkpoint_coverage_matches_the_records_actually_present() {
    for transcript in transcripts() {
        let mut records: BTreeMap<String, Vec<u64>> = BTreeMap::new();
        let mut previous_coverage: BTreeMap<String, u64> = BTreeMap::new();
        for frame in transcript.frames() {
            if frame.channel != Channel::Stdout {
                continue;
            }
            let run = frame
                .frame
                .get("run_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let rejected = frame.expect_reject.is_some();
            match frame.frame_type() {
                Some("record") if !rejected => {
                    if let Some(seq) = frame.frame.get("seq").and_then(serde_json::Value::as_u64) {
                        records.entry(run).or_default().push(seq);
                    }
                }
                Some("checkpoint") if !rejected => {
                    let covers = frame
                        .frame
                        .get("covers_record_seq_through")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default();
                    let claimed = frame
                        .frame
                        .get("records_covered")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default();
                    let seq = frame
                        .frame
                        .get("seq")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default();
                    assert!(
                        covers < seq,
                        "{}: a checkpoint covers records below its own seq",
                        transcript.name
                    );
                    let floor = previous_coverage.get(&run).copied().unwrap_or(0);
                    assert!(
                        covers >= floor,
                        "{}: checkpoint coverage regressed",
                        transcript.name
                    );
                    let actual = records
                        .get(&run)
                        .map(|seqs| {
                            seqs.iter()
                                .filter(|seq| **seq > floor && **seq <= covers)
                                .count()
                        })
                        .unwrap_or(0);
                    assert_eq!(
                        u64::try_from(actual).unwrap_or_default(),
                        claimed,
                        "{}: checkpoint at seq {seq} claims {claimed} records but {actual} are \
                         present in ({floor}, {covers}]",
                        transcript.name
                    );
                    previous_coverage.insert(run, covers);
                }
                _ => {}
            }
        }
    }
}

#[test]
fn the_header_outcome_matches_the_final_exit_line() {
    for transcript in transcripts() {
        let Some(header) = transcript.header() else {
            continue;
        };
        let exits: Vec<_> = transcript.exits().collect();
        if exits.is_empty() {
            // Transcript 07's second half is aborted before any exit line.
            continue;
        }
        let matching = exits
            .iter()
            .any(|exit| exit.outcome == header.expected_outcome);
        assert!(
            matching,
            "{}: no exit line reports the header's expected outcome {:?}",
            transcript.name, header.expected_outcome
        );
        if let Some(expected) = header.expected_exit_code {
            let matching_code = exits.iter().any(|exit| exit.code == expected);
            assert!(
                matching_code,
                "{}: no exit line reports the header's expected exit code {expected}",
                transcript.name
            );
        }
    }
}
