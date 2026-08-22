//! The frozen v1 bound ceilings, the per-run effective limits, and the run
//! deadline.
//!
//! Field process output is untrusted input. Protocol v1 freezes a ceiling for
//! every bound; a notebook may configure a value lower, and raising one above
//! its ceiling requires a protocol revision. The effective limits are echoed to
//! the Field in the request so a well-behaved connector can self-police rather
//! than discovering a limit by being killed — but core enforces every one of
//! them regardless of what the Field does.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::grammar::OffsetDatetime;

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
    /// The frozen v1 ceilings, which are also the defaults.
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
    /// The A2 default idle bound.
    pub const DEFAULT_IDLE_SECONDS: u32 = 120;
    /// The A2 default cancellation grace period.
    pub const DEFAULT_CANCEL_GRACE_SECONDS: u32 = 10;
    /// The A2 run wall-clock ceiling, in seconds.
    pub const MAX_RUN_SECONDS: u64 = 3600;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frozen_ceilings_validate() -> Result<(), LimitError> {
        Limits::ceilings().validate()
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
    fn limits_round_trip_and_reject_unknown_members() -> Result<(), serde_json::Error> {
        let text = serde_json::to_string(&Limits::ceilings())?;
        let parsed: Limits = serde_json::from_str(&text)?;
        assert_eq!(parsed, Limits::ceilings());
        let with_extra = text.replace("{\"", "{\"max_surprise\":1,\"");
        assert!(serde_json::from_str::<Limits>(&with_extra).is_err());
        Ok(())
    }
}
