//! The frozen v1 bound ceilings, the per-run effective limits, and the run
//! deadline.
//!
//! Field process output is untrusted input. Protocol v1 freezes a **ceiling**
//! for every bound: an absolute technical bound that protects core against a
//! hostile or buggy child, which no configuration may exceed and which only a
//! protocol revision can raise. The effective limits are echoed to the Field
//! in the request so a well-behaved connector can self-police rather than
//! discovering a limit by being killed — but core enforces every one of them
//! regardless of what the Field does.
//!
//! # A ceiling is not the same thing as a default
//!
//! Most bounds have no reason to differ from their ceiling: there is no
//! product reason a notebook would want fewer than 256 property candidates
//! per record, say. Two bounds are different: [`Limits::max_artifact_bytes`]
//! and the run wall clock (expressed as [`Deadline::not_after`], an absolute
//! instant rather than a duration field). For both, [`Limits::defaults()`]
//! states the value core requests absent configuration, which is well below
//! the frozen ceiling, and a configured value may move anywhere from the
//! product's own minimum up to that ceiling — never above it, because the
//! ceiling is what bounds an untrusted process, not what bounds a user's
//! preference. Configuring **up** toward the ceiling is exactly as legal as
//! configuring down from it; only crossing the ceiling itself requires a
//! protocol revision. Reading and applying that configuration is a `sync`
//! concern for `0.1.1` and later; this crate only states the default and the
//! ceiling a configured value is checked against.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::grammar::{GrammarError, MediaTypeMatcher, OffsetDatetime};

/// Why an effective limit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitError {
    /// The limit member that was out of range.
    pub member: &'static str,
    /// The value that was refused.
    pub value: u64,
    /// The A2 ceiling, or the schema minimum when the value was too small.
    pub bound: u64,
    /// Whether the value exceeded the ceiling or fell below the minimum.
    pub above_ceiling: bool,
}

impl fmt::Display for LimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.above_ceiling {
            write!(
                f,
                "{} is {} but protocol v1 freezes its ceiling at {}; raising it requires a protocol revision",
                self.member, self.value, self.bound
            )
        } else {
            write!(
                f,
                "{} is {} but the protocol requires at least {}",
                self.member, self.value, self.bound
            )
        }
    }
}

impl std::error::Error for LimitError {}

/// The effective bounds for one run.
///
/// Every member is required on the wire: a Field must never have to guess a
/// bound, and an absent bound would be an unbounded bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// One frame, including its terminating LF.
    pub max_frame_bytes: u64,
    /// A record's `body.text`.
    pub max_body_bytes: u64,
    /// Property candidates per record.
    pub max_properties_per_record: u64,
    /// One property value.
    pub max_property_value_bytes: u64,
    /// Members in one list value.
    pub max_list_members: u64,
    /// Artifact references per record.
    pub max_artifacts_per_record: u64,
    /// One staged artifact.
    pub max_artifact_bytes: u64,
    /// Staged bytes per run.
    pub max_run_artifact_bytes: u64,
    /// Standard output per run.
    pub max_run_stdout_bytes: u64,
    /// Records per run.
    pub max_run_records: u64,
    /// Diagnostics per run.
    pub max_run_diagnostics: u64,
    /// Captured standard error per run, ring-buffered.
    pub max_stderr_bytes: u64,
    /// A cursor.
    pub max_cursor_bytes: u64,
}

/// One entry of the frozen ceiling table: the member name, its schema minimum,
/// and its A2 ceiling.
struct Ceiling {
    member: &'static str,
    minimum: u64,
    ceiling: u64,
    read: fn(&Limits) -> u64,
}

const CEILINGS: [Ceiling; 13] = [
    Ceiling {
        member: "max_frame_bytes",
        minimum: 4096,
        ceiling: 1_048_576,
        read: |limits| limits.max_frame_bytes,
    },
    Ceiling {
        member: "max_body_bytes",
        minimum: 1024,
        ceiling: 1_048_576,
        read: |limits| limits.max_body_bytes,
    },
    Ceiling {
        member: "max_properties_per_record",
        minimum: 1,
        ceiling: 256,
        read: |limits| limits.max_properties_per_record,
    },
    Ceiling {
        member: "max_property_value_bytes",
        minimum: 64,
        ceiling: 65_536,
        read: |limits| limits.max_property_value_bytes,
    },
    Ceiling {
        member: "max_list_members",
        minimum: 1,
        ceiling: 1024,
        read: |limits| limits.max_list_members,
    },
    Ceiling {
        member: "max_artifacts_per_record",
        minimum: 0,
        ceiling: 64,
        read: |limits| limits.max_artifacts_per_record,
    },
    Ceiling {
        member: "max_artifact_bytes",
        minimum: 0,
        ceiling: 536_870_912,
        read: |limits| limits.max_artifact_bytes,
    },
    Ceiling {
        member: "max_run_artifact_bytes",
        minimum: 0,
        ceiling: 8_589_934_592,
        read: |limits| limits.max_run_artifact_bytes,
    },
    Ceiling {
        member: "max_run_stdout_bytes",
        minimum: 4096,
        ceiling: 1_073_741_824,
        read: |limits| limits.max_run_stdout_bytes,
    },
    Ceiling {
        member: "max_run_records",
        minimum: 0,
        ceiling: 1_000_000,
        read: |limits| limits.max_run_records,
    },
    Ceiling {
        member: "max_run_diagnostics",
        minimum: 0,
        ceiling: 10_000,
        read: |limits| limits.max_run_diagnostics,
    },
    Ceiling {
        member: "max_stderr_bytes",
        minimum: 0,
        ceiling: 262_144,
        read: |limits| limits.max_stderr_bytes,
    },
    Ceiling {
        member: "max_cursor_bytes",
        minimum: 1,
        ceiling: 4096,
        read: |limits| limits.max_cursor_bytes,
    },
];

impl Limits {
    /// The default single-artifact retention threshold: 25 MiB, spelled out
    /// as an exact byte count so there is no decimal-versus-binary ambiguity:
    /// 25 × 1024 × 1024 = 26,214,400 bytes.
    ///
    /// A notebook is disposable working material, not a system of record:
    /// the default keeps what is useful for work and context, and larger
    /// original material stays at its source rather than being copied by
    /// default. This is a default, not the ceiling — see
    /// [`Limits::max_artifact_bytes`]'s ceiling of 536,870,912 bytes (512
    /// MiB), which a notebook may configure this value up to.
    pub const DEFAULT_ARTIFACT_BYTES: u64 = 26_214_400;

    /// The frozen v1 ceilings: the absolute technical bound for every member,
    /// which no configuration may exceed.
    ///
    /// For most members the ceiling is also the sensible default, so
    /// [`Limits::defaults()`] reuses this constructor's values apart from
    /// [`Limits::max_artifact_bytes`], where it does not.
    #[must_use]
    pub fn ceilings() -> Self {
        Limits {
            max_frame_bytes: 1_048_576,
            max_body_bytes: 1_048_576,
            max_properties_per_record: 256,
            max_property_value_bytes: 65_536,
            max_list_members: 1024,
            max_artifacts_per_record: 64,
            max_artifact_bytes: 536_870_912,
            max_run_artifact_bytes: 8_589_934_592,
            max_run_stdout_bytes: 1_073_741_824,
            max_run_records: 1_000_000,
            max_run_diagnostics: 10_000,
            max_stderr_bytes: 262_144,
            max_cursor_bytes: 4096,
        }
    }

    /// The limits core requests absent any configuration.
    ///
    /// Identical to [`Limits::ceilings()`] except
    /// [`Limits::max_artifact_bytes`], which defaults to
    /// [`Limits::DEFAULT_ARTIFACT_BYTES`] rather than the frozen ceiling.
    /// Settings may raise `max_artifact_bytes` from this default up to the
    /// ceiling — never above it — because the ceiling protects core against a
    /// hostile or buggy child and the default merely reflects what is useful
    /// to keep.
    #[must_use]
    pub fn defaults() -> Self {
        Limits {
            max_artifact_bytes: Self::DEFAULT_ARTIFACT_BYTES,
            ..Self::ceilings()
        }
    }

    /// Checks every member against its schema minimum and its A2 ceiling.
    ///
    /// A notebook may configure a value lower than the ceiling. A value above
    /// it is refused rather than clamped, because clamping would let a
    /// configuration silently mean something other than what it says.
    pub fn validate(&self) -> Result<(), LimitError> {
        for entry in &CEILINGS {
            let value = (entry.read)(self);
            if value < entry.minimum {
                return Err(LimitError {
                    member: entry.member,
                    value,
                    bound: entry.minimum,
                    above_ceiling: false,
                });
            }
            if value > entry.ceiling {
                return Err(LimitError {
                    member: entry.member,
                    value,
                    bound: entry.ceiling,
                    above_ceiling: true,
                });
            }
        }
        Ok(())
    }

    /// The same limits with `max_frame_bytes` lowered, for a test that needs a
    /// small frame ceiling without hand-writing the whole table.
    #[must_use]
    pub fn with_max_frame_bytes(mut self, bytes: u64) -> Self {
        self.max_frame_bytes = bytes;
        self
    }
}

/// The run's wall-clock, idle, and cancellation-grace bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deadline {
    /// The instant after which the run has exceeded its wall clock.
    pub not_after: OffsetDatetime,
    /// Seconds without a frame or artifact progress before the run is idle.
    pub idle_seconds: u32,
    /// Seconds a cancelled Field has to exit before core terminates it.
    pub cancel_grace_seconds: u32,
}

impl Deadline {
    /// The frozen v1 ceilings for the two duration members.
    pub const MAX_IDLE_SECONDS: u32 = 3600;
    /// The frozen v1 ceiling for the cancellation grace period.
    pub const MAX_CANCEL_GRACE_SECONDS: u32 = 120;
    /// The A2 default idle bound. Stays sensibly proportioned to
    /// [`Deadline::DEFAULT_RUN_SECONDS`] regardless of the run-length default.
    pub const DEFAULT_IDLE_SECONDS: u32 = 120;
    /// The A2 default cancellation grace period.
    pub const DEFAULT_CANCEL_GRACE_SECONDS: u32 = 10;
    /// The absolute run wall-clock ceiling, in seconds: the technical bound
    /// past which no configuration may push the run, because it protects
    /// core against a hostile or buggy child that never stops.
    pub const MAX_RUN_SECONDS: u64 = 3600;
    /// The run wall-clock default, in seconds: 10 minutes, not the 3600-second
    /// ceiling.
    ///
    /// A first full sync that would need longer than this is expected to be
    /// handled by windowed, resumable runs — the cursor and checkpoint
    /// machinery already exists for exactly that — rather than by one long
    /// running process, because a crash late in a long run discards far more
    /// durable-but-uncommitted work than a crash late in a short one. This is
    /// a default core computes `not_after` from absent configuration; a
    /// configured value may lengthen the run up to
    /// [`Deadline::MAX_RUN_SECONDS`], never past it.
    pub const DEFAULT_RUN_SECONDS: u64 = 600;

    /// Checks the two duration members against their ceilings.
    pub fn validate(&self) -> Result<(), LimitError> {
        if self.idle_seconds == 0
            || u64::from(self.idle_seconds) > u64::from(Self::MAX_IDLE_SECONDS)
        {
            return Err(LimitError {
                member: "idle_seconds",
                value: u64::from(self.idle_seconds),
                bound: u64::from(Self::MAX_IDLE_SECONDS),
                above_ceiling: self.idle_seconds != 0,
            });
        }
        if self.cancel_grace_seconds == 0
            || self.cancel_grace_seconds > Self::MAX_CANCEL_GRACE_SECONDS
        {
            return Err(LimitError {
                member: "cancel_grace_seconds",
                value: u64::from(self.cancel_grace_seconds),
                bound: u64::from(Self::MAX_CANCEL_GRACE_SECONDS),
                above_ceiling: self.cancel_grace_seconds != 0,
            });
        }
        Ok(())
    }
}

/// The most entries `CollectRequest.artifact_media_types` admits.
///
/// A technical bound on the list length, not a policy ceiling: unlike
/// [`Limits::max_artifact_bytes`], there is no absolute number of media types
/// that protects core from a hostile child, only a sane upper bound on how
/// long the list a well-behaved caller sends can be.
pub const MAX_ARTIFACT_MEDIA_TYPES: usize = 128;

/// The default v1 artifact media-type retention include set: what a run
/// retains by default, in addition to clearing the size threshold
/// [`Limits::max_artifact_bytes`] already governs.
///
/// Approved by `docs/decisions/0007-attachment-retention-policy.md`.
/// Documents and text, images, and audio are included; video, archives, disk
/// images, and installers/executables are excluded, because voice is a
/// first-class Note type but a first-class Note type does not need every
/// container format a source system might send.
///
/// This is a **default**, not a ceiling: a notebook may configure a
/// different include set entirely, in either direction, exactly like
/// [`Limits::max_artifact_bytes`]'s configurable default. ADR 0008 gave all
/// twenty of these media types a canonical extension, so a retained original
/// is named correctly whenever its media type is known. Content detection
/// alone still cannot identify seven of them — the Office and OpenDocument
/// formats share ZIP magic bytes, and CSV has no signature at all — so those
/// resolve to `.bin` until a Field supplies the media type from upstream
/// metadata. Retention policy and extension naming remain orthogonal.
///
/// # Panics
///
/// Never, in practice: every literal below is a fixed, reviewed constant.
/// Should one ever be malformed, this function panics rather than silently
/// shipping a shorter default set than the one actually approved.
#[must_use]
pub fn default_artifact_media_types() -> Vec<MediaTypeMatcher> {
    const DEFAULT: [&str; 20] = [
        // Documents and text.
        "application/pdf",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "application/vnd.oasis.opendocument.text",
        "application/vnd.oasis.opendocument.spreadsheet",
        "application/vnd.oasis.opendocument.presentation",
        "text/plain",
        "text/markdown",
        "text/csv",
        "application/rtf",
        // Images.
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/heic",
        // Audio: voice is a first-class Note type.
        "audio/mp4",
        "audio/mpeg",
        "audio/wav",
        "audio/ogg",
    ];
    DEFAULT
        .iter()
        .map(|text| {
            MediaTypeMatcher::parse(text)
                .unwrap_or_else(|error| panic_on_default_media_type(text, error))
        })
        .collect()
}

/// Isolated so the `unwrap_or_else` closure above stays a one-line call, which
/// keeps the panic message assembly out of the hot path and out of the
/// closure clippy would otherwise flag as doing too much.
fn panic_on_default_media_type(text: &str, error: GrammarError) -> MediaTypeMatcher {
    panic!("the default artifact media type {text:?} must be a valid matcher: {error}")
}

/// Whether `essence` -- the artifact's declared media type, already
/// parameter-stripped and ASCII-lowercased -- is included by `policy`,
/// honoring a subtype wildcard such as `image/*`.
#[must_use]
pub fn artifact_media_type_included(policy: &[MediaTypeMatcher], essence: &str) -> bool {
    policy.iter().any(|matcher| matcher.matches(essence))
}

/// Strips media-type parameters and lowercases, matching A1's extension-
/// registry lookup rule, so a declared `Text/Plain; charset=utf-8` compares
/// equal to the registered `text/plain`.
#[must_use]
pub fn media_type_essence(declared: &str) -> String {
    declared
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frozen_ceilings_validate() -> Result<(), LimitError> {
        Limits::ceilings().validate()
    }

    #[test]
    fn the_default_artifact_bytes_is_25_mib_below_the_512_mib_ceiling() -> Result<(), LimitError> {
        assert_eq!(Limits::DEFAULT_ARTIFACT_BYTES, 25 * 1024 * 1024);
        let defaults = Limits::defaults();
        assert_eq!(defaults.max_artifact_bytes, 26_214_400);
        assert!(defaults.max_artifact_bytes < Limits::ceilings().max_artifact_bytes);
        // A default must itself be a legal configuration.
        defaults.validate()?;
        // Every other member is unaffected: only the artifact retention
        // threshold has a default distinct from its ceiling.
        assert_eq!(defaults.max_frame_bytes, Limits::ceilings().max_frame_bytes);
        assert_eq!(defaults.max_run_records, Limits::ceilings().max_run_records);
        Ok(())
    }

    #[test]
    fn a_configured_artifact_ceiling_may_rise_from_the_default_toward_the_ceiling_but_not_past_it()
    {
        let raised = Limits {
            max_artifact_bytes: 100 * 1024 * 1024,
            ..Limits::defaults()
        };
        assert!(
            raised.validate().is_ok(),
            "settings may increase the retention threshold up to the frozen ceiling"
        );
        let past_ceiling = Limits {
            max_artifact_bytes: Limits::ceilings().max_artifact_bytes + 1,
            ..Limits::defaults()
        };
        assert!(
            past_ceiling.validate().is_err(),
            "the ceiling protects core against a hostile or buggy child; no configuration may \
             cross it"
        );
    }

    #[test]
    #[allow(
        clippy::assertions_on_constants,
        reason = "the invariant is between two named constants on purpose: a future edit that \
                  breaks it should fail this test, not just look odd in a diff"
    )]
    fn the_default_run_seconds_is_ten_minutes_well_under_the_hour_ceiling() {
        assert_eq!(Deadline::DEFAULT_RUN_SECONDS, 600);
        assert!(Deadline::DEFAULT_RUN_SECONDS < Deadline::MAX_RUN_SECONDS);
        // The idle and grace defaults stay proportioned to the shorter run
        // default rather than to the ceiling.
        assert!(u64::from(Deadline::DEFAULT_IDLE_SECONDS) < Deadline::DEFAULT_RUN_SECONDS);
    }

    #[test]
    fn a_lower_configured_value_is_allowed_and_a_higher_one_is_refused() {
        let lowered = Limits {
            max_frame_bytes: 65_536,
            ..Limits::ceilings()
        };
        assert!(lowered.validate().is_ok());

        let raised = Limits {
            max_artifact_bytes: 536_870_913,
            ..Limits::ceilings()
        };
        match raised.validate() {
            Err(error) => {
                assert_eq!(error.member, "max_artifact_bytes");
                assert!(error.above_ceiling);
            }
            Ok(()) => panic!("a value above the ceiling must be refused"),
        }
    }

    #[test]
    fn a_value_below_the_schema_minimum_is_refused() {
        let tiny = Limits {
            max_frame_bytes: 4095,
            ..Limits::ceilings()
        };
        match tiny.validate() {
            Err(error) => {
                assert_eq!(error.member, "max_frame_bytes");
                assert!(!error.above_ceiling);
            }
            Ok(()) => panic!("a value below the minimum must be refused"),
        }
    }

    #[test]
    fn deadline_bounds_are_checked() -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Deadline {
            not_after: OffsetDatetime::parse("2026-08-22T12:30:00+02:00")?,
            idle_seconds: Deadline::DEFAULT_IDLE_SECONDS,
            cancel_grace_seconds: Deadline::DEFAULT_CANCEL_GRACE_SECONDS,
        };
        deadline.validate()?;

        let too_long = Deadline {
            cancel_grace_seconds: 121,
            ..deadline
        };
        assert!(too_long.validate().is_err());
        let zero = Deadline {
            idle_seconds: 0,
            ..deadline
        };
        assert!(zero.validate().is_err());
        Ok(())
    }

    #[test]
    fn the_default_media_types_parse_and_include_the_approved_twenty() {
        let defaults = default_artifact_media_types();
        assert_eq!(defaults.len(), 20);
        assert!(defaults.len() <= MAX_ARTIFACT_MEDIA_TYPES);
        for included in [
            "application/pdf",
            "text/plain",
            "text/markdown",
            "text/csv",
            "application/rtf",
            "image/png",
            "image/heic",
            "audio/mp4",
            "audio/ogg",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.oasis.opendocument.text",
        ] {
            assert!(
                artifact_media_type_included(&defaults, included),
                "{included} must be in the default include set"
            );
        }
        for excluded in [
            "video/mp4",
            "application/zip",
            "application/x-msdownload",
            "application/x-iso9660-image",
        ] {
            assert!(
                !artifact_media_type_included(&defaults, excluded),
                "{excluded} must not be in the default include set"
            );
        }
    }

    #[test]
    fn media_type_essence_strips_parameters_and_lowercases() {
        assert_eq!(
            media_type_essence("Text/Plain; charset=utf-8"),
            "text/plain"
        );
        assert_eq!(media_type_essence("image/PNG"), "image/png");
    }

    #[test]
    fn limits_round_trip_and_reject_unknown_members() -> Result<(), serde_json::Error> {
        let text = serde_json::to_string(&Limits::ceilings())?;
        let parsed: Limits = serde_json::from_str(&text)?;
        assert_eq!(parsed, Limits::ceilings());
        let with_extra = text.replace("{\"", "{\"max_surprise\":1,\"");
        assert!(serde_json::from_str::<Limits>(&with_extra).is_err());
        Ok(())
    }
}
