//! Transport-level well-formedness guards.
//!
//! Every grammar here is a **guard for an untrusted child process, not a
//! definition**. Where a value also appears in a notebook record, A1 remains
//! authoritative and core still performs the full A1 validation after a guard
//! passes. That is why, for example, [`NoteTypeToken`] constrains the 32-byte
//! primary-type grammar but does not enumerate A1's eleven approved types: a
//! protocol-level copy of that vocabulary is a copy that can drift.
//!
//! Guards exist so an untrusted child cannot make core do work on obvious
//! nonsense, and so a hostile string can never become a path component.

use core::fmt;

use serde::de::{self, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use fieldnotes_domain::Datetime;

/// Why a guarded value was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrammarError {
    /// The guard that refused the value.
    pub guard: &'static str,
    /// What was wrong with it.
    pub kind: GrammarErrorKind,
}

/// The shape of a guard failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarErrorKind {
    /// Shorter than the guard's minimum length in bytes.
    TooShort,
    /// Longer than the guard's maximum length in bytes.
    TooLong,
    /// Does not match the guard's character grammar.
    Pattern,
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.kind {
            GrammarErrorKind::TooShort => "is shorter than the protocol guard allows",
            GrammarErrorKind::TooLong => "is longer than the protocol guard allows",
            GrammarErrorKind::Pattern => "does not match the protocol guard grammar",
        };
        write!(f, "{} {reason}", self.guard)
    }
}

impl std::error::Error for GrammarError {}

/// Whether `text` matches `[a-z][a-z0-9]*(?:_[a-z0-9]+)*`.
#[must_use]
pub fn is_underscore_token(text: &str) -> bool {
    segmented(text, b'_')
}

/// Whether `text` matches `[a-z][a-z0-9]*(?:-[a-z0-9]+)*`.
#[must_use]
pub fn is_hyphen_token(text: &str) -> bool {
    segmented(text, b'-')
}

fn segmented(text: &str, separator: u8) -> bool {
    if !text.is_ascii() {
        return false;
    }
    let mut segments = text.split(separator as char);
    let Some(first) = segments.next() else {
        return false;
    };
    let mut bytes = first.bytes();
    match bytes.next() {
        Some(b'a'..=b'z') => {}
        _ => return false,
    }
    if !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()) {
        return false;
    }
    segments.all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

/// Whether `text` matches `[a-z][a-z0-9_]*`, the A1 property-name grammar.
#[must_use]
pub fn is_lower_snake(text: &str) -> bool {
    fieldnotes_domain::property::is_valid_property_name(text)
}

/// Whether `text` is lowercase hexadecimal.
#[must_use]
pub fn is_lower_hex(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether `text` contains no C0 control character and no `U+007F`.
///
/// A control character in a source scope or identity is never meaningful and is
/// exactly what would corrupt a log line, a terminal, or a diagnostic.
#[must_use]
pub fn is_printable(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() && c != '\u{a0}') && !text.contains('\u{7f}')
}

macro_rules! guarded_string {
    (
        $(#[$meta:meta])*
        $name:ident, min = $min:expr, max = $max:expr, check = $check:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// The guard's minimum length in bytes.
            pub const MIN_BYTES: usize = $min;
            /// The guard's maximum length in bytes.
            pub const MAX_BYTES: usize = $max;

            /// Validates `text` against this guard.
            pub fn parse(text: &str) -> Result<Self, GrammarError> {
                if text.len() < Self::MIN_BYTES {
                    return Err(GrammarError {
                        guard: stringify!($name),
                        kind: GrammarErrorKind::TooShort,
                    });
                }
                if text.len() > Self::MAX_BYTES {
                    return Err(GrammarError {
                        guard: stringify!($name),
                        kind: GrammarErrorKind::TooLong,
                    });
                }
                let check: fn(&str) -> bool = $check;
                if !check(text) {
                    return Err(GrammarError {
                        guard: stringify!($name),
                        kind: GrammarErrorKind::Pattern,
                    });
                }
                Ok($name(text.to_owned()))
            }

            /// The validated textual form.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                $name::parse(&text).map_err(de::Error::custom)
            }
        }
    };
}

guarded_string! {
    /// Core's identifier for one bounded Field process run.
    ///
    /// Opaque to the Field, and never a notebook record ID.
    RunId, min = 36, max = 36, check = |text| {
        let bytes = text.as_bytes();
        bytes.len() == 36
            && bytes.iter().enumerate().all(|(index, byte)| match index {
                8 | 13 | 18 | 23 => *byte == b'-',
                _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
            })
    }
}

guarded_string! {
    /// A lowercase 64-character SHA-256 hexadecimal digest.
    Sha256Hex, min = 64, max = 64, check = is_lower_hex
}

guarded_string! {
    /// Half of the portable exact-source key: a connector-namespaced,
    /// non-secret, cross-instance-stable upstream authority or account scope.
    SourceScope, min = 1, max = 512, check = is_printable
}

guarded_string! {
    /// Half of the portable exact-source key: object identity stable within its
    /// source scope.
    SourceIdentity, min = 1, max = 1024, check = is_printable
}

guarded_string! {
    /// An opaque source-supplied version token, comparable only through the
    /// ordering rule the manifest declares.
    SourceVersion, min = 1, max = 256, check = is_printable
}

guarded_string! {
    /// An opaque, non-secret, bounded, Field-owned resume token that core never
    /// parses, orders, or interprets.
    ///
    /// Excludes every C0 control character, not just NUL, matching the
    /// treatment [`SourceScope`] already gets: a cursor containing an
    /// unescaped LF is exactly the value that corrupts an NDJSON-shaped state
    /// file or a log line if it is ever written out raw, and a cursor is
    /// stored and logged, not merely compared.
    Cursor, min = 1, max = 4096, check = is_printable
}

guarded_string! {
    /// A declared media type, mapped through the A1 canonical-extension
    /// registry. The media type is never artifact identity.
    MediaType, min = 3, max = 255, check = |text: &str| {
        let Some((kind, subtype)) = text.split_once('/') else {
            return false;
        };
        let token = |part: &str| {
            let mut bytes = part.bytes();
            match bytes.next() {
                Some(byte) if byte.is_ascii_lowercase() || byte.is_ascii_digit() => {}
                _ => return false,
            }
            part.len() <= 127
                && bytes.all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-')
                })
        };
        token(kind) && token(subtype)
    }
}

guarded_string! {
    /// One entry of a media-type retention include set: an exact `type/subtype`
    /// media type, or a subtype wildcard such as `image/*`. Never an extension,
    /// and never derived from a source filename -- A1 section 2's rule that a
    /// source filename never selects the stored extension applies equally to
    /// retention. Matching this grammar says nothing about whether the type has
    /// a canonical extension in A1's separate, frozen extension registry: the
    /// two questions are orthogonal by design.
    MediaTypeMatcher, min = 3, max = 255, check = |text: &str| {
        let Some((kind, subtype)) = text.split_once('/') else {
            return false;
        };
        let token = |part: &str| {
            let mut bytes = part.bytes();
            match bytes.next() {
                Some(byte) if byte.is_ascii_lowercase() || byte.is_ascii_digit() => {}
                _ => return false,
            }
            part.len() <= 127
                && bytes.all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-')
                })
        };
        token(kind) && (subtype == "*" || token(subtype))
    }
}

impl MediaTypeMatcher {
    /// Whether `essence` -- an already parameter-stripped, ASCII-lowercased
    /// `type/subtype` media type -- matches this entry, honoring a subtype
    /// wildcard such as `image/*`.
    #[must_use]
    pub fn matches(&self, essence: &str) -> bool {
        if self.as_str() == essence {
            return true;
        }
        match self.as_str().split_once('/') {
            Some((kind, "*")) => essence
                .split_once('/')
                .is_some_and(|(essence_kind, _)| essence_kind == kind),
            _ => false,
        }
    }
}

guarded_string! {
    /// A stable connector-namespaced upstream attachment reference, following
    /// the same object-kind-namespace convention `SourceIdentity` uses. Carried
    /// only on a `not_retained` artifact reference, and projected by core onto
    /// the shared `skipped_attachments` Note property.
    AttachmentRef, min = 1, max = 1024, check = is_printable
}

guarded_string! {
    /// An identity-anchor namespace.
    IdentityNamespace, min = 1, max = 63, check = |text: &str| {
        let mut bytes = text.bytes();
        match bytes.next() {
            Some(b'a'..=b'z') => {}
            _ => return false,
        }
        bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }
}

guarded_string! {
    /// A stable connector driver name.
    DriverName, min = 1, max = 63, check = is_hyphen_token
}

guarded_string! {
    /// A connector driver version string.
    DriverVersion, min = 1, max = 63, check = |text: &str| {
        let mut bytes = text.bytes();
        match bytes.next() {
            Some(byte) if byte.is_ascii_alphanumeric() => {}
            _ => return false,
        }
        bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
    }
}

guarded_string! {
    /// A connector-local object-kind token, also used as the object-kind
    /// namespace inside a source identity.
    ObjectKind, min = 1, max = 63, check = is_hyphen_token
}

guarded_string! {
    /// A registered connector property prefix, including its trailing
    /// underscore.
    PropertyPrefix, min = 2, max = 32, check = |text: &str| {
        text.ends_with('_') && is_underscore_token(&text[..text.len() - 1])
    }
}

guarded_string! {
    /// A configured Field ID, as a transport guard for the A1 Field-ID grammar.
    ///
    /// Core validates the value against the A1 registered stem set; `self` does
    /// not use this protocol at all.
    FieldIdToken, min = 1, max = 63, check = is_underscore_token
}

guarded_string! {
    /// A registered Field stem, as a transport guard.
    FieldStemToken, min = 1, max = 31, check = is_underscore_token
}

guarded_string! {
    /// An A1 property name, as a transport guard.
    PropertyNameToken, min = 1, max = 63, check = is_lower_snake
}

guarded_string! {
    /// A primary Note type candidate.
    ///
    /// The closed eleven-value vocabulary is frozen by A1 and deliberately not
    /// enumerated here; core rejects any value the A1 registry does not contain.
    NoteTypeToken, min = 1, max = 32, check = is_lower_snake
}

guarded_string! {
    /// A single-use per-run authorization token for the protected channel.
    ///
    /// Not source credential material: it authorizes nothing outside this run's
    /// channel.
    GrantId, min = 16, max = 128, check = is_lower_hex
}

guarded_string! {
    /// The non-secret name of a configured credential profile. Not the
    /// credential.
    ProfileRef, min = 1, max = 63, check = is_lower_snake
}

guarded_string! {
    /// A named derivation or normalization rule.
    RuleName, min = 1, max = 63, check = is_lower_snake
}

guarded_string! {
    /// A non-secret snapshot scope token core expects a Field to reconcile
    /// completely.
    SnapshotScope, min = 1, max = 512, check = is_printable
}

guarded_string! {
    /// Bounded free text for a human reviewer.
    ShortText, min = 1, max = 512, check = |_| true
}

guarded_string! {
    /// Bounded free text where the schema allows a kilobyte.
    MediumText, min = 1, max = 1024, check = |_| true
}

guarded_string! {
    /// Bounded already-redacted human-readable diagnostic text.
    MessageText, min = 1, max = 4096, check = |_| true
}

/// The major protocol version, fixed at 1 for this proposal.
///
/// Serializes as the JSON number `1` and refuses to deserialize from anything
/// else, which is how a peer that answers with a version this build does not
/// implement fails closed instead of being partially interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ProtocolV1;

impl Serialize for ProtocolV1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(1)
    }
}

impl<'de> Deserialize<'de> for ProtocolV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u64::deserialize(deserializer)?;
        if value == 1 {
            Ok(ProtocolV1)
        } else {
            Err(de::Error::invalid_value(
                Unexpected::Unsigned(value),
                &"protocol major version 1",
            ))
        }
    }
}

/// A manifest member whose only honest value is `false`.
///
/// Encoding it as a constant makes a connector that disagrees fail validation
/// instead of quietly behaving differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ConstFalse;

impl Serialize for ConstFalse {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(false)
    }
}

impl<'de> Deserialize<'de> for ConstFalse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? {
            Err(de::Error::invalid_value(
                Unexpected::Bool(true),
                &"the constant false",
            ))
        } else {
            Ok(ConstFalse)
        }
    }
}

/// A manifest member whose only honest value is `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ConstTrue;

impl Serialize for ConstTrue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for ConstTrue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? {
            Ok(ConstTrue)
        } else {
            Err(de::Error::invalid_value(
                Unexpected::Bool(false),
                &"the constant true",
            ))
        }
    }
}

/// An RFC 3339 datetime with an explicit numeric offset.
///
/// `Z` and `-00:00` are both refused, exactly as the A1 datetime contract
/// requires. The value is parsed with [`fieldnotes_domain::Datetime`] rather
/// than a protocol-local copy of the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OffsetDatetime(Datetime);

impl OffsetDatetime {
    /// The longest accepted spelling, matching the schema's `maxLength`.
    pub const MAX_BYTES: usize = 40;

    /// Parses an explicit-offset RFC 3339 datetime.
    pub fn parse(text: &str) -> Result<Self, GrammarError> {
        if text.len() > Self::MAX_BYTES {
            return Err(GrammarError {
                guard: "OffsetDatetime",
                kind: GrammarErrorKind::TooLong,
            });
        }
        Datetime::parse(text)
            .map(OffsetDatetime)
            .map_err(|_| GrammarError {
                guard: "OffsetDatetime",
                kind: GrammarErrorKind::Pattern,
            })
    }

    /// The parsed domain value.
    #[must_use]
    pub fn datetime(&self) -> Datetime {
        self.0
    }
}

impl fmt::Display for OffsetDatetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for OffsetDatetime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for OffsetDatetime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        OffsetDatetime::parse(&text).map_err(de::Error::custom)
    }
}

/// Generates a unit type that serializes as one fixed `type` discriminator and
/// refuses every other spelling.
macro_rules! type_tag {
    ($(#[$meta:meta])* $name:ident, $wire:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name;

        impl $name {
            /// The wire spelling of this discriminator.
            pub const WIRE: &'static str = $wire;
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str($wire)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                if text == $wire {
                    Ok($name)
                } else {
                    Err(de::Error::invalid_value(
                        Unexpected::Str(&text),
                        &$wire,
                    ))
                }
            }
        }
    };
}

type_tag! {
    /// The `describe_request` discriminator.
    DescribeRequestTag, "describe_request"
}
type_tag! {
    /// The `manifest` discriminator.
    ManifestTag, "manifest"
}
type_tag! {
    /// The `collect_request` discriminator.
    CollectRequestTag, "collect_request"
}
type_tag! {
    /// The `cancel` discriminator.
    CancelTag, "cancel"
}
type_tag! {
    /// The `record` discriminator.
    RecordTag, "record"
}
type_tag! {
    /// The `checkpoint` discriminator.
    CheckpointTag, "checkpoint"
}
type_tag! {
    /// The `diagnostic` discriminator.
    DiagnosticTag, "diagnostic"
}
type_tag! {
    /// The `credential_request` discriminator.
    CredentialRequestTag, "credential_request"
}
type_tag! {
    /// The `credential_response` discriminator.
    CredentialResponseTag, "credential_response"
}
type_tag! {
    /// The `markdown` body-format discriminator. A record body is source text,
    /// and the protocol admits exactly one format for it.
    MarkdownTag, "markdown"
}
type_tag! {
    /// The `tombstone` deletion-authority discriminator.
    TombstoneTag, "tombstone"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_guard_accepts_only_the_hyphenated_lowercase_form() {
        assert!(RunId::parse("1a4c9f2e-0000-4000-8000-000000000001").is_ok());
        assert!(RunId::parse("1A4C9F2E-0000-4000-8000-000000000001").is_err());
        assert!(RunId::parse("1a4c9f2e00004000800000000000001").is_err());
        assert!(RunId::parse("").is_err());
    }

    #[test]
    fn media_type_guard_requires_two_tokens() {
        assert!(MediaType::parse("text/markdown").is_ok());
        assert!(MediaType::parse("application/octet-stream").is_ok());
        assert!(MediaType::parse("text").is_err());
        assert!(MediaType::parse("Text/Markdown").is_err());
        assert!(MediaType::parse("text/").is_err());
    }

    #[test]
    fn media_type_matcher_admits_an_exact_type_or_a_subtype_wildcard() {
        assert!(MediaTypeMatcher::parse("application/pdf").is_ok());
        assert!(MediaTypeMatcher::parse("image/*").is_ok());
        assert!(MediaTypeMatcher::parse("image/").is_err());
        assert!(MediaTypeMatcher::parse("Image/*").is_err());
        assert!(MediaTypeMatcher::parse("image").is_err());
    }

    #[test]
    fn media_type_matcher_matching_honours_the_wildcard_and_nothing_else() {
        let exact = MediaTypeMatcher::parse("application/pdf")
            .unwrap_or_else(|error| panic!("must parse: {error}"));
        assert!(exact.matches("application/pdf"));
        assert!(!exact.matches("application/zip"));

        let wildcard = MediaTypeMatcher::parse("image/*")
            .unwrap_or_else(|error| panic!("must parse: {error}"));
        assert!(wildcard.matches("image/png"));
        assert!(wildcard.matches("image/heic"));
        assert!(!wildcard.matches("video/mp4"));
        assert!(!wildcard.matches("image"));
    }

    #[test]
    fn attachment_ref_guard_matches_source_identity_style_printable_text() {
        assert!(AttachmentRef::parse("mail-attachment/AAMkAGI2TQABAAACattach02").is_ok());
        assert!(AttachmentRef::parse("").is_err());
        assert!(AttachmentRef::parse("with\nnewline").is_err());
    }

    #[test]
    fn property_prefix_guard_requires_a_trailing_underscore() {
        assert!(PropertyPrefix::parse("local_").is_ok());
        assert!(PropertyPrefix::parse("outlook_mail_").is_ok());
        assert!(PropertyPrefix::parse("local").is_err());
        assert!(PropertyPrefix::parse("_").is_err());
    }

    #[test]
    fn source_scope_guard_refuses_control_characters() {
        assert!(SourceScope::parse("local-root:reference-library-v1").is_ok());
        assert!(SourceScope::parse("scope\nwith-newline").is_err());
        assert!(SourceScope::parse("scope\u{7f}").is_err());
    }

    #[test]
    fn cursor_guard_excludes_every_c0_control_character_not_only_nul() {
        assert!(Cursor::parse("walk:v1:seq=2;mtime=2026-08-22T09:45:00Z").is_ok());
        assert!(Cursor::parse("walk:v1:with\0nul").is_err());
        assert!(
            Cursor::parse("walk:v1:with\nlinefeed").is_err(),
            "a cursor containing LF is exactly the value that corrupts an NDJSON-shaped state \
             file or a log line, and it was previously legal"
        );
        assert!(Cursor::parse("walk:v1:with\ttab").is_err());
        assert!(Cursor::parse("walk:v1:with\u{7f}del").is_err());
    }

    #[test]
    fn offset_datetime_guard_matches_the_a1_datetime_rule() {
        assert!(OffsetDatetime::parse("2026-08-22T09:45:00+02:00").is_ok());
        assert!(OffsetDatetime::parse("2026-08-22T20:00:00-05:00").is_ok());
        assert!(OffsetDatetime::parse("2026-08-22T09:45:00Z").is_err());
        assert!(OffsetDatetime::parse("2026-08-22T09:45:00-00:00").is_err());
        assert!(OffsetDatetime::parse("2026-08-22").is_err());
    }

    #[test]
    fn constants_refuse_the_other_value() -> Result<(), serde_json::Error> {
        assert!(serde_json::from_str::<ConstFalse>("false").is_ok());
        assert!(serde_json::from_str::<ConstFalse>("true").is_err());
        assert!(serde_json::from_str::<ConstTrue>("true").is_ok());
        assert!(serde_json::from_str::<ConstTrue>("false").is_err());
        assert_eq!(serde_json::to_string(&ConstFalse)?, "false");
        Ok(())
    }

    #[test]
    fn protocol_version_is_a_constant() -> Result<(), serde_json::Error> {
        assert_eq!(serde_json::to_string(&ProtocolV1)?, "1");
        assert!(serde_json::from_str::<ProtocolV1>("1").is_ok());
        assert!(serde_json::from_str::<ProtocolV1>("2").is_err());
        Ok(())
    }

    #[test]
    fn type_tags_refuse_a_different_discriminator() {
        assert!(serde_json::from_str::<ManifestTag>("\"manifest\"").is_ok());
        assert!(serde_json::from_str::<ManifestTag>("\"record\"").is_err());
        assert_eq!(ManifestTag::WIRE, "manifest");
    }
}
