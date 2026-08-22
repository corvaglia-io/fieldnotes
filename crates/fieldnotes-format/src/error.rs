//! The typed rejection model for public notebook files.
//!
//! Variants map onto the conceptual error names used by the approved invalid
//! corpus (`frontmatter.nested_mapping`, `datetime.offset_required`,
//! `security.secret_detected`, `filename.mismatch`, ...) via
//! [`ValidationError::conceptual_label`].

use core::fmt;

use fieldnotes_domain::ScalarKind;

/// Why a public notebook file was rejected.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ValidationError {
    /// The file is not valid UTF-8.
    InvalidUtf8,
    /// The file does not begin with an opening `---` frontmatter delimiter.
    MissingOpeningDelimiter,
    /// No closing `---` frontmatter delimiter was found.
    MissingClosingDelimiter,
    /// The required single blank line between frontmatter and body is missing.
    MissingBodySeparator,
    /// More than one blank line separates the frontmatter from the body, so the
    /// body would begin with a blank line the canonical grammar does not allow.
    ExtraBodySeparator,
    /// A frontmatter value contains a nested mapping or inline object.
    NestedMapping {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A list item is a mapping/object instead of a scalar.
    ArrayObject {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A list mixes more than one scalar type.
    MixedList {
        /// The offending property name.
        key: String,
    },
    /// A property value is `null`; missing values must be omitted.
    NullValue {
        /// The offending property name.
        key: String,
    },
    /// A property name appears more than once.
    DuplicateKey {
        /// The duplicated property name.
        key: String,
    },
    /// A value carries a YAML tag.
    CustomTag {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A value uses a YAML anchor.
    Anchor {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A value uses a YAML alias.
    Alias {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// An explicit YAML document marker (`...` or an extra `---`) appears.
    DocumentMarker {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A value uses YAML flow-sequence syntax instead of block style.
    FlowSequence {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A value uses a YAML block scalar (`|` or `>`).
    BlockScalar {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A value uses single-quoted style; canonical text is plain or double-quoted.
    SingleQuoted {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A YAML comment appears; the canonical serializer emits none.
    Comment {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A blank line appears inside the frontmatter block.
    BlankLine {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// Indentation does not match the canonical flat two-space list form.
    BadIndentation {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A line has trailing whitespace or is otherwise not `key: value` shaped.
    MalformedLine {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A double-quoted scalar is not a valid RFC 8785 JSON string.
    InvalidString {
        /// One-based frontmatter line number.
        line: usize,
    },
    /// A property name violates `[a-z][a-z0-9_]*` or the 63-byte limit.
    InvalidPropertyName {
        /// The offending property name.
        key: String,
    },
    /// An unregistered property without a registered connector prefix.
    UnknownUnprefixed {
        /// The offending property name.
        key: String,
    },
    /// A registered list-typed property was emitted as a scalar.
    ListRequired {
        /// The offending property name.
        key: String,
    },
    /// A registered scalar-typed property was emitted as a list.
    ScalarRequired {
        /// The offending property name.
        key: String,
    },
    /// A scalar does not have the registered type for its property.
    TypeMismatch {
        /// The offending property name.
        key: String,
        /// The registered scalar type.
        expected: ScalarKind,
    },
    /// A datetime has no explicit numeric UTC offset.
    OffsetRequired {
        /// The offending property name.
        key: String,
    },
    /// A datetime uses the invalid `-00:00` offset.
    NegativeZeroOffset {
        /// The offending property name.
        key: String,
    },
    /// A datetime or date is malformed or out of calendar/clock range.
    InvalidDatetime {
        /// The offending property name.
        key: String,
    },
    /// A number is not a valid finite JSON-grammar binary64 literal.
    InvalidNumber {
        /// The offending property name.
        key: String,
    },
    /// An integer literal is outside the exactly representable binary64 range.
    IntegerOutOfRange {
        /// The offending property name.
        key: String,
    },
    /// A non-finite number cannot be serialized.
    NonFiniteNumber {
        /// The offending property name.
        key: String,
    },
    /// A required property is missing.
    MissingRequired {
        /// The missing property name.
        key: String,
    },
    /// A record or referenced ID is malformed.
    InvalidId {
        /// The property carrying the ID.
        key: String,
    },
    /// An ID has the wrong record-kind prefix for its position.
    WrongIdKind {
        /// The property carrying the ID.
        key: String,
    },
    /// The `field_id` is not `self` or a registered stem plus valid label.
    InvalidFieldId {
        /// The rejected value.
        value: String,
    },
    /// The Note `type` is not one of the eleven approved primary types.
    UnknownNoteType {
        /// The rejected value.
        value: String,
    },
    /// A non-Note record `type` violates the lowercase type grammar.
    InvalidRecordType {
        /// The rejected value.
        value: String,
    },
    /// `source_identity` is present without `source_scope`.
    ScopeRequired,
    /// `source_scope` is present without `source_identity`.
    IdentityRequired,
    /// An `attachments` member does not also appear in `artifacts`.
    AttachmentNotInArtifacts {
        /// The offending artifact ID.
        value: String,
    },
    /// A `content_hash` value is not `fn-content-v1-sha256:<64-lowercase-hex>`.
    InvalidContentHash,
    /// A frontmatter text value contains a secret indicator.
    SecretDetected {
        /// The property carrying the secret.
        key: String,
    },
    /// The actual filename disagrees with the name computed from frontmatter.
    FilenameMismatch {
        /// The expected canonical filename.
        expected: String,
        /// The actual filename.
        actual: String,
    },
    /// A proposal `binding_status` is outside `bound`/`unresolved`/`ambiguous`
    /// or disagrees with the presence of `entity_id`.
    BindingStatusViolation,
    /// A proposal `status` is outside the approved public vocabulary.
    UnknownProposalStatus {
        /// The rejected value.
        value: String,
    },
    /// The instance metadata file violates the exact three-key schema.
    InvalidInstanceMetadata {
        /// A short reason.
        reason: &'static str,
    },
}

impl ValidationError {
    /// The conceptual error name used by the approved invalid-corpus table.
    #[must_use]
    pub fn conceptual_label(&self) -> &'static str {
        match self {
            ValidationError::InvalidUtf8 => "encoding.invalid_utf8",
            ValidationError::MissingOpeningDelimiter
            | ValidationError::MissingClosingDelimiter
            | ValidationError::MissingBodySeparator
            | ValidationError::ExtraBodySeparator => "frontmatter.envelope",
            ValidationError::NestedMapping { .. } => "frontmatter.nested_mapping",
            ValidationError::ArrayObject { .. } => "frontmatter.array_object",
            ValidationError::MixedList { .. } => "frontmatter.mixed_list",
            ValidationError::NullValue { .. } => "frontmatter.null",
            ValidationError::DuplicateKey { .. } => "frontmatter.duplicate_key",
            ValidationError::CustomTag { .. } => "frontmatter.custom_tag",
            ValidationError::Anchor { .. } => "frontmatter.anchor",
            ValidationError::Alias { .. } => "frontmatter.alias",
            ValidationError::DocumentMarker { .. } => "frontmatter.document_marker",
            ValidationError::FlowSequence { .. } => "frontmatter.flow_sequence",
            ValidationError::BlockScalar { .. } => "frontmatter.block_scalar",
            ValidationError::SingleQuoted { .. } => "frontmatter.single_quoted",
            ValidationError::Comment { .. } => "frontmatter.comment",
            ValidationError::BlankLine { .. } => "frontmatter.blank_line",
            ValidationError::BadIndentation { .. } => "frontmatter.indentation",
            ValidationError::MalformedLine { .. } => "frontmatter.malformed_line",
            ValidationError::InvalidString { .. } => "frontmatter.invalid_string",
            ValidationError::InvalidPropertyName { .. } => "property.invalid_name",
            ValidationError::UnknownUnprefixed { .. } => "property.unknown_unprefixed",
            ValidationError::ListRequired { .. } => "property.list_required",
            ValidationError::ScalarRequired { .. } => "property.scalar_required",
            ValidationError::TypeMismatch { .. } => "property.type_mismatch",
            ValidationError::OffsetRequired { .. } => "datetime.offset_required",
            ValidationError::NegativeZeroOffset { .. } => "datetime.negative_zero_offset",
            ValidationError::InvalidDatetime { .. } => "datetime.invalid",
            ValidationError::InvalidNumber { .. } => "number.invalid",
            ValidationError::IntegerOutOfRange { .. } => "number.integer_out_of_range",
            ValidationError::NonFiniteNumber { .. } => "number.non_finite",
            ValidationError::MissingRequired { .. } => "record.missing_required",
            ValidationError::InvalidId { .. } => "record.invalid_id",
            ValidationError::WrongIdKind { .. } => "record.wrong_id_kind",
            ValidationError::InvalidFieldId { .. } => "record.invalid_field_id",
            ValidationError::UnknownNoteType { .. } => "record.unknown_note_type",
            ValidationError::InvalidRecordType { .. } => "record.invalid_record_type",
            ValidationError::ScopeRequired => "source.scope_required",
            ValidationError::IdentityRequired => "source.identity_required",
            ValidationError::AttachmentNotInArtifacts { .. } => {
                "record.attachment_not_in_artifacts"
            }
            ValidationError::InvalidContentHash => "record.invalid_content_hash",
            ValidationError::SecretDetected { .. } => "security.secret_detected",
            ValidationError::FilenameMismatch { .. } => "filename.mismatch",
            ValidationError::BindingStatusViolation => "proposal.binding_status",
            ValidationError::UnknownProposalStatus { .. } => "proposal.unknown_status",
            ValidationError::InvalidInstanceMetadata { .. } => "instance.invalid_metadata",
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::InvalidUtf8 => write!(f, "file is not valid UTF-8"),
            ValidationError::MissingOpeningDelimiter => {
                write!(f, "missing opening frontmatter delimiter")
            }
            ValidationError::MissingClosingDelimiter => {
                write!(f, "missing closing frontmatter delimiter")
            }
            ValidationError::MissingBodySeparator => {
                write!(f, "missing blank line between frontmatter and body")
            }
            ValidationError::ExtraBodySeparator => {
                write!(f, "more than one blank line between frontmatter and body")
            }
            ValidationError::NestedMapping { line } => {
                write!(f, "nested mapping at frontmatter line {line}")
            }
            ValidationError::ArrayObject { line } => {
                write!(f, "list item is an object at frontmatter line {line}")
            }
            ValidationError::MixedList { key } => {
                write!(f, "list property `{key}` mixes scalar types")
            }
            ValidationError::NullValue { key } => {
                write!(f, "property `{key}` is null; omit missing values")
            }
            ValidationError::DuplicateKey { key } => write!(f, "duplicate property `{key}`"),
            ValidationError::CustomTag { line } => write!(f, "YAML tag at frontmatter line {line}"),
            ValidationError::Anchor { line } => write!(f, "YAML anchor at frontmatter line {line}"),
            ValidationError::Alias { line } => write!(f, "YAML alias at frontmatter line {line}"),
            ValidationError::DocumentMarker { line } => {
                write!(
                    f,
                    "explicit YAML document marker at frontmatter line {line}"
                )
            }
            ValidationError::FlowSequence { line } => {
                write!(f, "flow-style sequence at frontmatter line {line}")
            }
            ValidationError::BlockScalar { line } => {
                write!(f, "block scalar at frontmatter line {line}")
            }
            ValidationError::SingleQuoted { line } => {
                write!(f, "single-quoted scalar at frontmatter line {line}")
            }
            ValidationError::Comment { line } => {
                write!(f, "YAML comment at frontmatter line {line}")
            }
            ValidationError::BlankLine { line } => {
                write!(f, "blank line inside frontmatter at line {line}")
            }
            ValidationError::BadIndentation { line } => {
                write!(f, "bad indentation at frontmatter line {line}")
            }
            ValidationError::MalformedLine { line } => {
                write!(f, "malformed frontmatter line {line}")
            }
            ValidationError::InvalidString { line } => {
                write!(f, "invalid double-quoted string at frontmatter line {line}")
            }
            ValidationError::InvalidPropertyName { key } => {
                write!(f, "invalid property name `{key}`")
            }
            ValidationError::UnknownUnprefixed { key } => {
                write!(f, "unknown unprefixed property `{key}`")
            }
            ValidationError::ListRequired { key } => write!(f, "property `{key}` must be a list"),
            ValidationError::ScalarRequired { key } => {
                write!(f, "property `{key}` must be a scalar")
            }
            ValidationError::TypeMismatch { key, expected } => {
                write!(f, "property `{key}` must be {}", expected.as_str())
            }
            ValidationError::OffsetRequired { key } => {
                write!(
                    f,
                    "datetime `{key}` requires an explicit numeric UTC offset"
                )
            }
            ValidationError::NegativeZeroOffset { key } => {
                write!(f, "datetime `{key}` uses the invalid -00:00 offset")
            }
            ValidationError::InvalidDatetime { key } => {
                write!(f, "invalid datetime or date in `{key}`")
            }
            ValidationError::InvalidNumber { key } => write!(f, "invalid number in `{key}`"),
            ValidationError::IntegerOutOfRange { key } => {
                write!(f, "integer in `{key}` exceeds the exact binary64 range")
            }
            ValidationError::NonFiniteNumber { key } => write!(f, "non-finite number in `{key}`"),
            ValidationError::MissingRequired { key } => {
                write!(f, "missing required property `{key}`")
            }
            ValidationError::InvalidId { key } => write!(f, "invalid record ID in `{key}`"),
            ValidationError::WrongIdKind { key } => {
                write!(f, "wrong record-kind prefix in `{key}`")
            }
            ValidationError::InvalidFieldId { value } => write!(f, "invalid field ID `{value}`"),
            ValidationError::UnknownNoteType { value } => {
                write!(f, "unknown primary Note type `{value}`")
            }
            ValidationError::InvalidRecordType { value } => {
                write!(f, "invalid record type `{value}`")
            }
            ValidationError::ScopeRequired => {
                write!(f, "source_identity requires source_scope")
            }
            ValidationError::IdentityRequired => {
                write!(f, "source_scope requires source_identity")
            }
            ValidationError::AttachmentNotInArtifacts { value } => {
                write!(f, "attachment `{value}` does not appear in artifacts")
            }
            ValidationError::InvalidContentHash => write!(f, "invalid content_hash value form"),
            ValidationError::SecretDetected { key } => {
                write!(f, "secret indicator detected in `{key}`")
            }
            ValidationError::FilenameMismatch { expected, actual } => {
                write!(
                    f,
                    "filename `{actual}` does not match computed `{expected}`"
                )
            }
            ValidationError::BindingStatusViolation => {
                write!(
                    f,
                    "binding_status disagrees with entity_id presence or vocabulary"
                )
            }
            ValidationError::UnknownProposalStatus { value } => {
                write!(f, "unknown proposal status `{value}`")
            }
            ValidationError::InvalidInstanceMetadata { reason } => {
                write!(f, "invalid instance metadata: {reason}")
            }
        }
    }
}

impl std::error::Error for ValidationError {}
