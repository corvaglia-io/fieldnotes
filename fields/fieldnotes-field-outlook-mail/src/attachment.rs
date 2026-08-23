//! Applying the run's retention policy to one message's attachments, and
//! staging the bytes that survive it.
//!
//! # The policy is applied before any byte is fetched
//!
//! A2 section 5 and section 14 make the retention policy two numbers plus a
//! set: `limits.max_artifact_bytes` bounds one artifact, and
//! `artifact_media_types` names the included media types. Graph reports an
//! attachment's declared size and media type in the attachment *list*, without
//! its bytes, so this module decides retention from that metadata and fetches
//! `contentBytes` only for an attachment the policy already admitted. An
//! excluded attachment therefore costs one listing and no download at all.
//!
//! # Declining is never a failure
//!
//! An attachment excluded by size or by media type produces a `not_retained`
//! artifact reference carrying its `attachment_ref`, which core projects onto
//! the shared `skipped_attachments` Note property. The Note is still created,
//! the run still succeeds, and no error diagnostic is emitted -- "stays at
//! source" is a policy decision, not a fault (A2 section 14).

use std::fmt;

use fieldnotes_domain::RandomSource;
use fieldnotes_field_protocol::grammar::{AttachmentRef, MediaType, MediaTypeMatcher, Sha256Hex};
use fieldnotes_field_protocol::limits::{Limits, artifact_media_type_included, media_type_essence};
use fieldnotes_field_protocol::message::{ArtifactKind, ArtifactRef, ArtifactRole};
use fieldnotes_msgraph::{GraphError, HttpTransport, RetryClock};

use crate::api::GraphAttachment;
use crate::body::AttachmentLine;
use crate::mail::MailReader;

/// What the run's retention policy decided about one attachment, before any
/// byte was fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Retention {
    /// The policy admits these bytes; fetch and stage them.
    Retain,
    /// The declared size is over the run's single-artifact threshold.
    OverSizeThreshold {
        /// The size the source declared.
        declared_bytes: u64,
    },
    /// The declared media type is outside the run's include set.
    MediaTypeExcluded {
        /// The parameter-stripped, lowercased media type.
        essence: String,
    },
    /// The attachment is not a file attachment, so it has no original bytes at
    /// the mail endpoint at all.
    NoOriginalBytes,
    /// Retaining these bytes would push the run past its staged-byte budget.
    OverRunBudget,
}

impl Retention {
    /// Whether the bytes should be fetched at all.
    pub(crate) fn retains(&self) -> bool {
        matches!(self, Retention::Retain)
    }

    /// The reviewable body-evidence sentence for a decline.
    fn evidence(&self) -> String {
        match self {
            Retention::Retain => "retained as an original artifact".to_owned(),
            Retention::OverSizeThreshold { declared_bytes } => format!(
                "not retained: {declared_bytes} bytes is over this run's retention threshold, so \
                 it stays at its source"
            ),
            Retention::MediaTypeExcluded { essence } => format!(
                "not retained: media type {essence} is outside this run's retention include set, \
                 so it stays at its source"
            ),
            Retention::NoOriginalBytes => {
                "not retained: this attachment kind has no original bytes at the mail endpoint"
                    .to_owned()
            }
            Retention::OverRunBudget => {
                "not retained: this run's staged-byte budget is already accounted for".to_owned()
            }
        }
    }
}

/// Decides retention for one attachment from its metadata alone.
///
/// `already_staged_bytes` is what this run has staged so far, so the run's
/// staged-byte budget is respected without discovering it by being rejected.
#[must_use]
pub(crate) fn plan(
    attachment: &GraphAttachment,
    limits: &Limits,
    policy: &[MediaTypeMatcher],
    already_staged_bytes: u64,
) -> Retention {
    if !attachment.is_file_attachment() {
        return Retention::NoOriginalBytes;
    }
    if let Some(declared) = attachment.declared_bytes() {
        if declared > limits.max_artifact_bytes {
            return Retention::OverSizeThreshold {
                declared_bytes: declared,
            };
        }
        if already_staged_bytes.saturating_add(declared) > limits.max_run_artifact_bytes {
            return Retention::OverRunBudget;
        }
    }
    // A media type this Field cannot parse into a `type/subtype` essence is
    // not a *known* media type, so the include set cannot exclude it; it is
    // retained on size alone, with no declared media type, exactly as the
    // `local` Field treats content it cannot classify.
    if let Some(declared) = &attachment.content_type {
        let essence = media_type_essence(declared);
        if MediaType::parse(&essence).is_ok() && !artifact_media_type_included(policy, &essence) {
            return Retention::MediaTypeExcluded { essence };
        }
    }
    Retention::Retain
}

/// Why one attachment could not be turned into an artifact reference at all.
///
/// Every variant is a per-attachment problem, never a run failure: the caller
/// reports it as a diagnostic, records the attachment as not retained, and
/// keeps the rest of the message.
#[derive(Debug)]
pub(crate) enum AttachmentError {
    /// The attachment carried no identifier, so it has no stable reference.
    NoIdentity,
    /// The reference did not satisfy its own transport guard.
    UnusableReference(String),
    /// The message has more attachments than the run's artifact-reference
    /// bound admits, so the remainder was not reported.
    TooManyReferences {
        /// The run's bound.
        bound: usize,
    },
    /// Graph could not be read.
    Graph(GraphError),
    /// Graph returned no `contentBytes` for a file attachment.
    NoContent,
    /// The base64 payload could not be decoded.
    Undecodable(crate::base64::Base64Error),
    /// The bytes could not be staged for core.
    Staging(String),
    /// The bytes that actually arrived were over the run's threshold, even
    /// though the declared size was not.
    ActuallyOversize {
        /// How many bytes actually arrived.
        actual_bytes: u64,
    },
}

impl fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttachmentError::NoIdentity => f.write_str(
                "an attachment arrived with no identifier, so it has no stable reference and was \
                 skipped",
            ),
            AttachmentError::UnusableReference(reason) => {
                write!(f, "attachment reference guard: {reason}")
            }
            AttachmentError::TooManyReferences { bound } => write!(
                f,
                "this message has more attachments than the run's bound of {bound} artifact \
                 references, so the remainder was not reported"
            ),
            AttachmentError::Graph(error) => write!(f, "{error}"),
            AttachmentError::NoContent => {
                f.write_str("a file attachment returned no content, so no bytes were retained")
            }
            AttachmentError::Undecodable(error) => write!(f, "{error}"),
            AttachmentError::Staging(reason) => write!(f, "could not stage attachment: {reason}"),
            AttachmentError::ActuallyOversize { actual_bytes } => write!(
                f,
                "the attachment delivered {actual_bytes} bytes, over this run's retention \
                 threshold, so it was not retained"
            ),
        }
    }
}

impl std::error::Error for AttachmentError {}

/// Everything one message's attachments produced.
#[derive(Debug, Default)]
pub(crate) struct AttachmentOutcome {
    /// The artifact references for the record, in source order.
    pub(crate) artifacts: Vec<ArtifactRef>,
    /// The per-attachment body evidence, in the same order.
    pub(crate) evidence: Vec<AttachmentLine>,
    /// Bytes actually staged for this message.
    pub(crate) staged_bytes: u64,
    /// Per-attachment problems worth a diagnostic. None of these fails the
    /// run.
    pub(crate) issues: Vec<AttachmentError>,
}

fn reference_for(attachment_id: &str) -> Result<AttachmentRef, AttachmentError> {
    AttachmentRef::parse(&format!(
        "{}/{attachment_id}",
        crate::constants::ATTACHMENT_KIND
    ))
    .map_err(|error| AttachmentError::UnusableReference(error.to_string()))
}

/// The attachment's own filename, bounded to what the schema admits as
/// display evidence. Never a path component: core derives every path itself.
fn display_filename(attachment: &GraphAttachment) -> Option<String> {
    let name = attachment.name.as_ref()?.trim();
    if name.is_empty() || name.len() > 255 {
        None
    } else {
        Some(name.to_owned())
    }
}

/// The reviewable body-evidence sentence for one attachment, noting whether
/// the message renders it inline.
///
/// An inline attachment -- a signature image, say -- is still a real
/// attachment with real bytes, so this Field applies the run's retention
/// policy to it like any other rather than inventing a Field-local policy of
/// its own. Saying so in the evidence is what makes a Note with three
/// signature logos legible instead of puzzling.
fn evidence_note(retention: &Retention, attachment: &GraphAttachment) -> String {
    let base = retention.evidence();
    if attachment.is_inline == Some(true) {
        format!("{base}; the message renders this attachment inline")
    } else {
        base
    }
}

fn declared_media_type(attachment: &GraphAttachment) -> Option<MediaType> {
    let declared = attachment.content_type.as_ref()?;
    MediaType::parse(&media_type_essence(declared)).ok()
}

/// Builds the `not_retained` reference for an attachment whose bytes this run
/// did not keep.
///
/// `attachment_ref` is the only stable identity a declined artifact has, since
/// it has no bytes and therefore no digest, so it is always present here.
fn not_retained(
    attachment: &GraphAttachment,
    attachment_id: &str,
) -> Result<ArtifactRef, AttachmentError> {
    Ok(ArtifactRef {
        kind: ArtifactKind::NotRetained,
        handle: None,
        sha256: None,
        byte_length: attachment.declared_bytes(),
        media_type: declared_media_type(attachment),
        role: ArtifactRole::Attachment,
        source_filename: display_filename(attachment),
        attachment_ref: Some(reference_for(attachment_id)?),
    })
}

/// Lists one message's attachments, applies the run's retention policy, and
/// stages the bytes that survive it.
///
/// `seq` is the record's per-run sequence number, which names the staging
/// handles so a staged file is unambiguously paired with the record that
/// references it and can never collide within a run.
pub(crate) fn collect<T, C, R>(
    reader: &MailReader<'_, T, C, R>,
    message_id: &str,
    seq: u64,
    staging_dir: &std::path::Path,
    limits: &Limits,
    policy: &[MediaTypeMatcher],
    already_staged_bytes: u64,
) -> AttachmentOutcome
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    let mut outcome = AttachmentOutcome::default();
    let max_references = usize::try_from(limits.max_artifacts_per_record).unwrap_or(usize::MAX);

    for (index, item) in reader.attachments(message_id).enumerate() {
        if outcome.artifacts.len() >= max_references {
            outcome.issues.push(AttachmentError::TooManyReferences {
                bound: max_references,
            });
            break;
        }
        let attachment = match item {
            Ok(attachment) => attachment,
            Err(error) => {
                outcome.issues.push(AttachmentError::Graph(error));
                break;
            }
        };
        let Some(attachment_id) = attachment.id.as_deref().filter(|id| !id.is_empty()) else {
            outcome.issues.push(AttachmentError::NoIdentity);
            continue;
        };

        let retention = plan(
            &attachment,
            limits,
            policy,
            already_staged_bytes.saturating_add(outcome.staged_bytes),
        );
        let label = display_filename(&attachment)
            .unwrap_or_else(|| format!("{}/{attachment_id}", crate::constants::ATTACHMENT_KIND));

        if !retention.retains() {
            match not_retained(&attachment, attachment_id) {
                Ok(reference) => {
                    outcome.evidence.push(AttachmentLine {
                        label,
                        byte_length: attachment.declared_bytes(),
                        note: evidence_note(&retention, &attachment),
                    });
                    outcome.artifacts.push(reference);
                }
                Err(error) => outcome.issues.push(error),
            }
            continue;
        }

        match stage_one(
            reader,
            message_id,
            attachment_id,
            &attachment,
            seq,
            index,
            staging_dir,
            limits,
        ) {
            Ok((reference, staged_bytes)) => {
                outcome.staged_bytes = outcome.staged_bytes.saturating_add(staged_bytes);
                outcome.evidence.push(AttachmentLine {
                    label,
                    byte_length: Some(staged_bytes),
                    note: evidence_note(&Retention::Retain, &attachment),
                });
                outcome.artifacts.push(reference);
            }
            Err(error) => {
                // The bytes could not be retained after all. The attachment is
                // still recorded, as not retained, so the Note still says the
                // message had it: losing the fact would be worse than losing
                // the bytes.
                let note = format!("not retained: {error}");
                match not_retained(&attachment, attachment_id) {
                    Ok(reference) => {
                        outcome.evidence.push(AttachmentLine {
                            label,
                            byte_length: attachment.declared_bytes(),
                            note,
                        });
                        outcome.artifacts.push(reference);
                    }
                    Err(reference_error) => outcome.issues.push(reference_error),
                }
                outcome.issues.push(error);
            }
        }
    }
    outcome
}

#[allow(clippy::too_many_arguments)]
fn stage_one<T, C, R>(
    reader: &MailReader<'_, T, C, R>,
    message_id: &str,
    attachment_id: &str,
    metadata: &GraphAttachment,
    seq: u64,
    index: usize,
    staging_dir: &std::path::Path,
    limits: &Limits,
) -> Result<(ArtifactRef, u64), AttachmentError>
where
    T: HttpTransport,
    C: RetryClock,
    R: RandomSource,
{
    let fetched = reader
        .attachment_content(message_id, attachment_id)
        .map_err(AttachmentError::Graph)?;
    let encoded = fetched
        .content_bytes
        .as_deref()
        .ok_or(AttachmentError::NoContent)?;
    let bytes = crate::base64::decode(encoded).map_err(AttachmentError::Undecodable)?;
    let byte_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if byte_length > limits.max_artifact_bytes {
        return Err(AttachmentError::ActuallyOversize {
            actual_bytes: byte_length,
        });
    }
    let handle = format!("a{seq:07}-{index:03}");
    let digest = fieldnotes_field_sdk::stage::stage_and_hash(staging_dir, &handle, &bytes)
        .map_err(|error| AttachmentError::Staging(error.to_string()))?;
    let sha256 =
        Sha256Hex::parse(&digest).map_err(|error| AttachmentError::Staging(error.to_string()))?;
    Ok((
        ArtifactRef {
            kind: ArtifactKind::Staged,
            handle: Some(handle),
            sha256: Some(sha256),
            byte_length: Some(byte_length),
            // Prefer the media type the *fetched* resource declared, falling
            // back to the listing's. Either way it is the source's claim, never
            // inferred from a filename: A1 section 2 forbids a source filename
            // selecting the stored extension.
            media_type: declared_media_type(&fetched).or_else(|| declared_media_type(metadata)),
            role: ArtifactRole::Attachment,
            source_filename: display_filename(&fetched).or_else(|| display_filename(metadata)),
            attachment_ref: None,
        },
        byte_length,
    ))
}

#[cfg(test)]
mod tests {
    use super::{Retention, plan};
    use fieldnotes_field_protocol::limits::{Limits, default_artifact_media_types};

    fn attachment(json: &str) -> crate::api::GraphAttachment {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("must deserialize: {error}"))
    }

    fn limits() -> Limits {
        Limits::defaults()
    }

    #[test]
    fn a_small_included_file_attachment_is_retained() {
        let value = attachment(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1",
                "name":"notes.txt","contentType":"text/plain","size":40}"##,
        );
        assert_eq!(
            plan(&value, &limits(), &default_artifact_media_types(), 0),
            Retention::Retain
        );
    }

    #[test]
    fn an_oversize_attachment_is_declined_on_its_declared_size_alone() {
        let value = attachment(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1",
                "name":"scan.pdf","contentType":"application/pdf","size":41943040}"##,
        );
        assert_eq!(
            plan(&value, &limits(), &default_artifact_media_types(), 0),
            Retention::OverSizeThreshold {
                declared_bytes: 41_943_040
            }
        );
    }

    #[test]
    fn a_media_type_outside_the_include_set_is_declined_even_when_small() {
        let value = attachment(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1",
                "name":"clip.mp4","contentType":"video/mp4","size":1024}"##,
        );
        assert_eq!(
            plan(&value, &limits(), &default_artifact_media_types(), 0),
            Retention::MediaTypeExcluded {
                essence: "video/mp4".to_owned()
            }
        );
    }

    #[test]
    fn media_type_parameters_and_case_do_not_defeat_the_include_set() {
        let value = attachment(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1",
                "name":"clip.mp4","contentType":"Video/MP4; codecs=avc1","size":1024}"##,
        );
        assert_eq!(
            plan(&value, &limits(), &default_artifact_media_types(), 0),
            Retention::MediaTypeExcluded {
                essence: "video/mp4".to_owned()
            }
        );
    }

    #[test]
    fn an_item_or_reference_attachment_has_no_original_bytes() {
        for odata_type in [
            "#microsoft.graph.itemAttachment",
            "#microsoft.graph.referenceAttachment",
        ] {
            let value = attachment(&format!(
                r##"{{"@odata.type":"{odata_type}","id":"a1","name":"Embedded","size":100}}"##
            ));
            assert_eq!(
                plan(&value, &limits(), &default_artifact_media_types(), 0),
                Retention::NoOriginalBytes
            );
        }
    }

    #[test]
    fn an_unparseable_media_type_is_retained_on_size_alone_rather_than_guessed_at() {
        let value = attachment(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1",
                "name":"thing","contentType":"not a media type at all","size":10}"##,
        );
        assert_eq!(
            plan(&value, &limits(), &default_artifact_media_types(), 0),
            Retention::Retain
        );
    }

    #[test]
    fn the_runs_staged_byte_budget_is_respected_before_any_download() {
        let value = attachment(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1",
                "name":"notes.txt","contentType":"text/plain","size":40}"##,
        );
        let tight = Limits {
            max_run_artifact_bytes: 10,
            ..Limits::defaults()
        };
        assert_eq!(
            plan(&value, &tight, &default_artifact_media_types(), 0),
            Retention::OverRunBudget
        );
    }

    #[test]
    fn a_configured_include_set_that_admits_video_retains_it() {
        let value = attachment(
            r##"{"@odata.type":"#microsoft.graph.fileAttachment","id":"a1",
                "name":"clip.mp4","contentType":"video/mp4","size":1024}"##,
        );
        let policy = vec![
            fieldnotes_field_protocol::grammar::MediaTypeMatcher::parse("video/*")
                .unwrap_or_else(|error| panic!("must parse: {error}")),
        ];
        assert_eq!(plan(&value, &limits(), &policy, 0), Retention::Retain);
    }
}
