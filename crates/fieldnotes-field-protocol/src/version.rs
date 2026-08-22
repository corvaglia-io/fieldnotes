//! Protocol-version negotiation, which fails closed and actionably in both
//! directions.
//!
//! Negotiation happens entirely inside the describe run, before any credential
//! grant, staging directory, or collect run exists:
//!
//! 1. core writes a describe request naming every major version it supports and
//!    the highest revision it understands;
//! 2. the Field selects one major version from that set and answers with a
//!    manifest declaring the version it selected, its own revision, and every
//!    version it supports;
//! 3. the negotiated revision is the minimum of the two declared revisions, and
//!    neither peer may emit a member introduced above it.
//!
//! A Field that supports no version core offered emits **no manifest** — a
//! manifest it cannot express correctly is worse than none — writes one
//! actionable line naming both version sets to standard error, and exits with
//! [`crate::ExitCode::Negotiation`]. A Field that answers with a version core
//! did not offer has its manifest rejected rather than partially interpreted.

use core::fmt;

use crate::codes::RejectionCode;

/// The single major protocol version this build implements.
pub const PROTOCOL_VERSION: u16 = 1;

/// The highest additive revision this build understands within
/// [`PROTOCOL_VERSION`].
pub const PROTOCOL_REVISION: u16 = 0;

/// An additive-only revision within a major version.
pub type ProtocolRevision = u16;

/// The largest revision the schema admits.
pub const MAX_PROTOCOL_REVISION: ProtocolRevision = 4095;

/// The largest major version the schema admits in a version list.
pub const MAX_PROTOCOL_VERSION: u16 = 4095;

/// A settled negotiation: the version both peers speak and the revision
/// neither may exceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Negotiation {
    /// The major version the Field selected from what core offered.
    pub version: u16,
    /// The minimum of the two declared revisions.
    pub revision: ProtocolRevision,
}

impl Negotiation {
    /// Whether a member introduced at `revision` may be emitted.
    #[must_use]
    pub fn admits_revision(&self, revision: ProtocolRevision) -> bool {
        revision <= self.revision
    }
}

/// Why negotiation failed, with both version sets so the message can name the
/// concrete remedy rather than reporting a mysterious rejection later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationError {
    /// The two peers share no major version.
    NoSharedVersion {
        /// Every version core offered.
        core_offered: Vec<u16>,
        /// Every version the Field supports.
        field_supports: Vec<u16>,
    },
    /// The Field answered with a version core did not offer.
    SelectedUnofferedVersion {
        /// The version the Field selected.
        selected: u16,
        /// Every version core offered.
        core_offered: Vec<u16>,
    },
    /// The Field selected a version it does not itself list as supported.
    SelectionNotSelfConsistent {
        /// The version the Field selected.
        selected: u16,
        /// Every version the Field claims to support.
        field_supports: Vec<u16>,
    },
    /// A declared revision exceeds what the schema admits.
    RevisionOutOfRange {
        /// The offending revision.
        revision: ProtocolRevision,
    },
}

impl NegotiationError {
    /// The rejection code core reports.
    ///
    /// Every negotiation failure is [`RejectionCode::ProtocolVersionUnsupported`]
    /// because the actionable fact is the version, not a schema-internal
    /// detail.
    #[must_use]
    pub fn code(&self) -> RejectionCode {
        RejectionCode::ProtocolVersionUnsupported
    }
}

fn render(versions: &[u16]) -> String {
    let mut rendered = String::from("[");
    for (index, version) in versions.iter().enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        rendered.push_str(&version.to_string());
    }
    rendered.push(']');
    rendered
}

impl fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NegotiationError::NoSharedVersion {
                core_offered,
                field_supports,
            } => write!(
                f,
                "protocol version mismatch: core offered {}, this build supports {}. \
                 Upgrade Fieldnotes or install the matching Field build.",
                render(core_offered),
                render(field_supports)
            ),
            NegotiationError::SelectedUnofferedVersion {
                selected,
                core_offered,
            } => write!(
                f,
                "the Field answered with protocol version {selected}, which core did not offer \
                 ({}). Install a Field build that speaks a version core supports.",
                render(core_offered)
            ),
            NegotiationError::SelectionNotSelfConsistent {
                selected,
                field_supports,
            } => write!(
                f,
                "the Field selected protocol version {selected} but declares support for {}. \
                 The manifest contradicts itself and is rejected rather than partly interpreted.",
                render(field_supports)
            ),
            NegotiationError::RevisionOutOfRange { revision } => write!(
                f,
                "declared protocol revision {revision} exceeds the {MAX_PROTOCOL_REVISION} the \
                 schema admits."
            ),
        }
    }
}

impl std::error::Error for NegotiationError {}

/// Chooses the version a Field should answer with, from the Field's side.
///
/// Returns the highest version both peers support, so a newer Field paired with
/// an older core speaks the older contract rather than failing. `None` means
/// there is nothing to answer with, and the Field must emit no manifest.
#[must_use]
pub fn select_version(core_offered: &[u16], field_supports: &[u16]) -> Option<u16> {
    core_offered
        .iter()
        .filter(|version| field_supports.contains(version))
        .copied()
        .max()
}

/// Settles negotiation from core's side, given what core offered and what the
/// manifest declared.
pub fn negotiate(
    core_offered: &[u16],
    core_max_revision: ProtocolRevision,
    field_selected: u16,
    field_revision: ProtocolRevision,
    field_supports: &[u16],
) -> Result<Negotiation, NegotiationError> {
    if core_max_revision > MAX_PROTOCOL_REVISION {
        return Err(NegotiationError::RevisionOutOfRange {
            revision: core_max_revision,
        });
    }
    if field_revision > MAX_PROTOCOL_REVISION {
        return Err(NegotiationError::RevisionOutOfRange {
            revision: field_revision,
        });
    }
    if !field_supports.contains(&field_selected) {
        return Err(NegotiationError::SelectionNotSelfConsistent {
            selected: field_selected,
            field_supports: field_supports.to_vec(),
        });
    }
    if !core_offered.contains(&field_selected) {
        // Distinguish "we share nothing" from "you answered with something we
        // did not offer": the remedies differ, and the message must say which.
        if select_version(core_offered, field_supports).is_none() {
            return Err(NegotiationError::NoSharedVersion {
                core_offered: core_offered.to_vec(),
                field_supports: field_supports.to_vec(),
            });
        }
        return Err(NegotiationError::SelectedUnofferedVersion {
            selected: field_selected,
            core_offered: core_offered.to_vec(),
        });
    }
    Ok(Negotiation {
        version: field_selected,
        revision: core_max_revision.min(field_revision),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_negotiated_revision_is_the_minimum_of_the_two() -> Result<(), NegotiationError> {
        let settled = negotiate(&[1], 3, 1, 1, &[1])?;
        assert_eq!(
            settled,
            Negotiation {
                version: 1,
                revision: 1
            }
        );
        assert!(settled.admits_revision(1));
        assert!(!settled.admits_revision(2));
        Ok(())
    }

    #[test]
    fn no_shared_version_names_both_sets() {
        match negotiate(&[1], 0, 2, 0, &[2, 3]) {
            Err(NegotiationError::NoSharedVersion {
                core_offered,
                field_supports,
            }) => {
                assert_eq!(core_offered, vec![1]);
                assert_eq!(field_supports, vec![2, 3]);
            }
            other => panic!("expected a no-shared-version failure, got {other:?}"),
        }
    }

    #[test]
    fn answering_with_an_unoffered_version_is_rejected_not_interpreted() {
        match negotiate(&[1, 2], 0, 3, 0, &[1, 3]) {
            Err(NegotiationError::SelectedUnofferedVersion { selected, .. }) => {
                assert_eq!(selected, 3);
            }
            other => panic!("expected an unoffered-version failure, got {other:?}"),
        }
    }

    #[test]
    fn a_self_contradicting_manifest_is_rejected() {
        assert!(matches!(
            negotiate(&[1], 0, 1, 0, &[2]),
            Err(NegotiationError::SelectionNotSelfConsistent { .. })
        ));
    }

    #[test]
    fn every_negotiation_failure_is_version_unsupported() {
        let error = NegotiationError::NoSharedVersion {
            core_offered: vec![1],
            field_supports: vec![2],
        };
        assert_eq!(error.code(), RejectionCode::ProtocolVersionUnsupported);
    }

    #[test]
    fn the_field_side_picks_the_highest_shared_version() {
        assert_eq!(select_version(&[1, 2], &[2, 3]), Some(2));
        assert_eq!(select_version(&[1], &[2, 3]), None);
    }

    #[test]
    fn the_failure_message_names_both_version_sets() {
        let error = NegotiationError::NoSharedVersion {
            core_offered: vec![1],
            field_supports: vec![2, 3],
        };
        let rendered = error.to_string();
        assert!(rendered.contains("core offered [1]"));
        assert!(rendered.contains("this build supports [2, 3]"));
    }
}
