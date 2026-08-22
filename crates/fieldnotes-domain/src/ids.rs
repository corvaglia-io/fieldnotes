//! Kind-prefixed logical record IDs and content-addressed artifact IDs.
//!
//! Logical records use lowercase, hyphenated UUIDv7 values with a readable
//! kind prefix. Immutable original artifacts are the deliberate exception and
//! use `artifact_sha256_<64-lowercase-hex>` over their exact bytes.

use core::fmt;

/// Errors produced while parsing or generating identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdError {
    /// The value does not start with a registered record-kind prefix.
    UnknownPrefix,
    /// The UUID portion is not a lowercase, hyphenated UUID.
    MalformedUuid,
    /// The UUID version nibble is not `7`.
    WrongUuidVersion,
    /// The UUID variant bits are not the RFC 9562 `10` variant.
    WrongUuidVariant,
    /// The artifact digest is not exactly 64 lowercase hexadecimal digits.
    MalformedDigest,
    /// The millisecond timestamp does not fit the 48-bit UUIDv7 field.
    TimestampOutOfRange,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdError::UnknownPrefix => write!(f, "unknown record-kind prefix"),
            IdError::MalformedUuid => write!(f, "malformed lowercase hyphenated UUID"),
            IdError::WrongUuidVersion => write!(f, "UUID version nibble is not 7"),
            IdError::WrongUuidVariant => {
                write!(f, "UUID variant bits are not the RFC 9562 variant")
            }
            IdError::MalformedDigest => write!(f, "artifact digest is not 64 lowercase hex digits"),
            IdError::TimestampOutOfRange => {
                write!(f, "timestamp does not fit 48 bits of milliseconds")
            }
        }
    }
}

impl std::error::Error for IdError {}

/// The kind of a logical Fieldnotes record, determined by its ID prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    /// A Fieldnotes instance (`fn_`).
    Instance,
    /// A Note (`note_`).
    Note,
    /// An Extraction (`ext_`).
    Extraction,
    /// An Observation (`obs_`).
    Observation,
    /// An entity projection (`ent_`).
    Entity,
    /// A relationship projection (`rel_`).
    Relationship,
    /// A proposal (`prop_`).
    Proposal,
    /// A handback package (`pkg_`).
    Package,
    /// A reconciliation conflict bundle (`conf_`).
    Conflict,
}

impl RecordKind {
    /// Every registered logical record kind.
    pub const ALL: [RecordKind; 9] = [
        RecordKind::Instance,
        RecordKind::Note,
        RecordKind::Extraction,
        RecordKind::Observation,
        RecordKind::Entity,
        RecordKind::Relationship,
        RecordKind::Proposal,
        RecordKind::Package,
        RecordKind::Conflict,
    ];

    /// The approved lowercase ID prefix for this kind, including the trailing underscore.
    #[must_use]
    pub fn prefix(self) -> &'static str {
        match self {
            RecordKind::Instance => "fn_",
            RecordKind::Note => "note_",
            RecordKind::Extraction => "ext_",
            RecordKind::Observation => "obs_",
            RecordKind::Entity => "ent_",
            RecordKind::Relationship => "rel_",
            RecordKind::Proposal => "prop_",
            RecordKind::Package => "pkg_",
            RecordKind::Conflict => "conf_",
        }
    }
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn push_hex(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(char::from(HEX[usize::from(byte >> 4)]));
    out.push(char::from(HEX[usize::from(byte & 0x0f)]));
}

/// A lowercase, hyphenated UUIDv7 value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid7 {
    bytes: [u8; 16],
}

impl Uuid7 {
    /// Parses the canonical lowercase hyphenated textual form and verifies the
    /// version and variant bits.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        let raw = text.as_bytes();
        if raw.len() != 36 {
            return Err(IdError::MalformedUuid);
        }
        let mut bytes = [0u8; 16];
        let mut nibble_index = 0usize;
        for (pos, byte) in raw.iter().enumerate() {
            if matches!(pos, 8 | 13 | 18 | 23) {
                if *byte != b'-' {
                    return Err(IdError::MalformedUuid);
                }
                continue;
            }
            let value = hex_val(*byte).ok_or(IdError::MalformedUuid)?;
            if nibble_index.is_multiple_of(2) {
                bytes[nibble_index / 2] = value << 4;
            } else {
                bytes[nibble_index / 2] |= value;
            }
            nibble_index += 1;
        }
        if bytes[6] >> 4 != 7 {
            return Err(IdError::WrongUuidVersion);
        }
        if bytes[8] >> 6 != 0b10 {
            return Err(IdError::WrongUuidVariant);
        }
        Ok(Uuid7 { bytes })
    }

    /// Builds a UUIDv7 from a Unix-epoch millisecond timestamp and ten random bytes.
    ///
    /// The timestamp is the ID creation instant, never `occurred_at`.
    pub fn from_parts(unix_millis: u64, random: [u8; 10]) -> Result<Self, IdError> {
        if unix_millis >= 1u64 << 48 {
            return Err(IdError::TimestampOutOfRange);
        }
        let ms = unix_millis.to_be_bytes();
        let mut bytes = [0u8; 16];
        bytes[..6].copy_from_slice(&ms[2..8]);
        bytes[6] = 0x70 | (random[0] & 0x0f);
        bytes[7] = random[1];
        bytes[8] = 0x80 | (random[2] & 0x3f);
        bytes[9..16].copy_from_slice(&random[3..10]);
        Ok(Uuid7 { bytes })
    }

    /// The embedded Unix-epoch millisecond creation timestamp.
    ///
    /// This is ID bookkeeping only; it must not be decoded for business behavior.
    #[must_use]
    pub fn unix_millis(&self) -> u64 {
        let mut ms = [0u8; 8];
        ms[2..8].copy_from_slice(&self.bytes[..6]);
        u64::from_be_bytes(ms)
    }

    /// The raw sixteen UUID bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.bytes
    }
}

impl fmt::Display for Uuid7 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::with_capacity(36);
        for (index, byte) in self.bytes.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                out.push('-');
            }
            push_hex(&mut out, *byte);
        }
        f.write_str(&out)
    }
}

/// A kind-prefixed logical record ID such as
/// `note_01a028d5-90c0-7248-a74b-c8bc1085ab19`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordId {
    kind: RecordKind,
    uuid: Uuid7,
}

impl RecordId {
    /// Combines a record kind and a UUIDv7 value.
    #[must_use]
    pub fn new(kind: RecordKind, uuid: Uuid7) -> Self {
        RecordId { kind, uuid }
    }

    /// Parses a kind-prefixed lowercase hyphenated UUIDv7 record ID.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        for kind in RecordKind::ALL {
            if let Some(rest) = text.strip_prefix(kind.prefix()) {
                return Ok(RecordId {
                    kind,
                    uuid: Uuid7::parse(rest)?,
                });
            }
        }
        Err(IdError::UnknownPrefix)
    }

    /// The record kind encoded by the prefix.
    #[must_use]
    pub fn kind(&self) -> RecordKind {
        self.kind
    }

    /// The UUIDv7 value portion.
    #[must_use]
    pub fn uuid(&self) -> &Uuid7 {
        &self.uuid
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.kind.prefix(), self.uuid)
    }
}

/// A content-addressed original-artifact ID:
/// `artifact_sha256_<64-lowercase-hex>` over the exact original bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactId {
    digest: [u8; 32],
}

impl ArtifactId {
    /// The fixed artifact-ID prefix.
    pub const PREFIX: &'static str = "artifact_sha256_";

    /// Wraps an exact-byte SHA-256 digest.
    #[must_use]
    pub fn from_digest(digest: [u8; 32]) -> Self {
        ArtifactId { digest }
    }

    /// Parses `artifact_sha256_<64-lowercase-hex>`.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        let hex = text
            .strip_prefix(Self::PREFIX)
            .ok_or(IdError::UnknownPrefix)?;
        let raw = hex.as_bytes();
        if raw.len() != 64 {
            return Err(IdError::MalformedDigest);
        }
        let mut digest = [0u8; 32];
        for (index, pair) in raw.as_chunks::<2>().0.iter().enumerate() {
            let hi = hex_val(pair[0]).ok_or(IdError::MalformedDigest)?;
            let lo = hex_val(pair[1]).ok_or(IdError::MalformedDigest)?;
            digest[index] = (hi << 4) | lo;
        }
        Ok(ArtifactId { digest })
    }

    /// The exact-byte SHA-256 digest.
    #[must_use]
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::with_capacity(Self::PREFIX.len() + 64);
        out.push_str(Self::PREFIX);
        for byte in self.digest {
            push_hex(&mut out, byte);
        }
        f.write_str(&out)
    }
}

/// An injectable source of the current Unix-epoch time in milliseconds.
///
/// Library behavior never reads the wall clock directly; production callers
/// supply an OS-backed implementation and tests supply a fixed one.
pub trait Clock {
    /// Current Unix-epoch time in milliseconds.
    fn unix_millis(&self) -> u64;
}

/// An injectable source of random bytes for ID generation.
///
/// Production callers must supply a cryptographically secure source; tests may
/// supply a deterministic one.
pub trait RandomSource {
    /// Fills `buffer` with random bytes.
    fn fill_bytes(&mut self, buffer: &mut [u8]);
}

/// Generates kind-prefixed UUIDv7 record IDs from an injected clock and
/// randomness source.
#[derive(Debug)]
pub struct RecordIdGenerator<C, R> {
    clock: C,
    random: R,
}

impl<C: Clock, R: RandomSource> RecordIdGenerator<C, R> {
    /// Combines a clock and a random source into a generator.
    pub fn new(clock: C, random: R) -> Self {
        RecordIdGenerator { clock, random }
    }

    /// Generates a new record ID whose UUIDv7 timestamp is the ID creation time.
    pub fn generate(&mut self, kind: RecordKind) -> Result<RecordId, IdError> {
        let mut random = [0u8; 10];
        self.random.fill_bytes(&mut random);
        let uuid = Uuid7::from_parts(self.clock.unix_millis(), random)?;
        Ok(RecordId::new(kind, uuid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays_note_id() -> Result<(), IdError> {
        let text = "note_01a028d5-90c0-7248-a74b-c8bc1085ab19";
        let id = RecordId::parse(text)?;
        assert_eq!(id.kind(), RecordKind::Note);
        assert_eq!(id.to_string(), text);
        Ok(())
    }

    #[test]
    fn rejects_uppercase_and_wrong_version() {
        assert_eq!(
            RecordId::parse("note_01A028D5-90C0-7248-A74B-C8BC1085AB19"),
            Err(IdError::MalformedUuid)
        );
        assert_eq!(
            RecordId::parse("note_01a028d5-90c0-4248-a74b-c8bc1085ab19"),
            Err(IdError::WrongUuidVersion)
        );
        assert_eq!(
            RecordId::parse("note_01a028d5-90c0-7248-c74b-c8bc1085ab19"),
            Err(IdError::WrongUuidVariant)
        );
        assert_eq!(
            RecordId::parse("thing_01a028d5-90c0-7248-a74b-c8bc1085ab19"),
            Err(IdError::UnknownPrefix)
        );
    }

    #[test]
    fn uuid_v7_round_trips_creation_millis() -> Result<(), IdError> {
        let uuid = Uuid7::from_parts(1_787_381_100_000, [0u8; 10])?;
        assert_eq!(uuid.unix_millis(), 1_787_381_100_000);
        assert_eq!(uuid.to_string(), "01a02837-2de0-7000-8000-000000000000");
        Ok(())
    }

    #[test]
    fn instance_fixture_uuid_timestamp_matches_created_at() -> Result<(), IdError> {
        let id = RecordId::parse("fn_01a02837-2de0-7a2b-8c41-f2481851192a")?;
        assert_eq!(id.uuid().unix_millis(), 1_787_381_100_000);
        Ok(())
    }

    #[test]
    fn artifact_id_round_trips() -> Result<(), IdError> {
        let text =
            "artifact_sha256_449d6bf49ec2725f12047f2db40baea3e2eb1112dbfd851aa0ecc558b91aab17";
        let id = ArtifactId::parse(text)?;
        assert_eq!(id.to_string(), text);
        assert_eq!(
            ArtifactId::parse("artifact_sha256_449d"),
            Err(IdError::MalformedDigest)
        );
        Ok(())
    }

    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn unix_millis(&self) -> u64 {
            self.0
        }
    }
    struct FixedRandom([u8; 10]);
    impl RandomSource for FixedRandom {
        fn fill_bytes(&mut self, buffer: &mut [u8]) {
            for (slot, value) in buffer.iter_mut().zip(self.0.iter().cycle()) {
                *slot = *value;
            }
        }
    }

    #[test]
    fn generator_is_deterministic_with_injected_inputs() -> Result<(), IdError> {
        let mut generator =
            RecordIdGenerator::new(FixedClock(1_787_381_100_000), FixedRandom([0xff; 10]));
        let id = generator.generate(RecordKind::Note)?;
        assert_eq!(id.to_string(), "note_01a02837-2de0-7fff-bfff-ffffffffffff");
        Ok(())
    }
}
