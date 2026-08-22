//! The Field-owned incremental resume cursor.
//!
//! # Format
//!
//! ```text
//! local-walk/v1;hw=<epoch-seconds>[;at=<percent-encoded-relative-path>,...|;wide=1]
//! ```
//!
//! `hw` is the high-water mark: the latest last-modified instant, in whole
//! Unix-epoch seconds, among every file this Field has confirmed does not
//! need reporting again. `at` lists, percent-encoded, the relative paths of
//! every file sitting exactly at that second that this Field has already
//! reported, so a second file landing on the same one-second tick as the
//! last reported file is never mistaken for one already seen. When more such
//! files exist than comfortably fit inside the cursor's byte bound, `wide=1`
//! is written instead of a list: it tells a later run to treat *every* file
//! at the watermark second as still due, trading a few safe duplicate
//! re-emissions for never dropping one.
//!
//! # Why it can lag but never lead
//!
//! [`CursorState::advance`] only ever moves the watermark forward from the
//! maximum modification instant this Field actually observed, and only when
//! [`crate::collect`] calls it after a walk that hit **zero** read errors.
//! A run that hit an error anywhere re-offers the previous cursor completely
//! unchanged (see [`CursorState::advance`]'s `Less` branch, driven by
//! `crate::collect` always comparing against the frozen previous state on an
//! incomplete walk): the Field cannot prove it saw everything under the
//! watermark, so it does not claim to have. On the next run, [`is_due`]
//! compares against that same, possibly stale, watermark, so no object already
//! reported is silently promoted to "settled" without this Field actually
//! having re-confirmed it. The only way an object could be missed forever is
//! if its modification time never advances past a watermark set before it
//! existed -- exactly the scenario `snapshot` mode exists to recover from,
//! since a snapshot walk ignores the cursor and re-examines everything in
//! its declared scope regardless of any watermark.
//!
//! [`is_due`]: CursorState::is_due

use std::collections::BTreeSet;

use fieldnotes_field_protocol::grammar::{Cursor as CursorToken, GrammarError};

/// The tag every cursor this Field writes begins with.
const TAG: &str = "local-walk/v1";

/// The most tie-break paths embedded before falling back to `wide=1`.
///
/// Chosen so the encoded cursor comfortably clears the frozen 4 KiB protocol
/// ceiling even for a full set of long relative paths sharing one second.
const MAX_TIE_BREAK_ENTRIES: usize = 32;

/// The decoded state of a cursor this Field previously offered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CursorState {
    /// The high-water mark, in whole Unix-epoch seconds.
    pub(crate) high_water: i64,
    /// Relative paths already reported exactly at `high_water`, when known
    /// precisely.
    pub(crate) at_high_water: BTreeSet<String>,
    /// Whether the tie-break set was too large to encode precisely, so every
    /// file at `high_water` is conservatively treated as still due.
    pub(crate) wide: bool,
}

impl CursorState {
    /// Whether a file last modified at `modified_unix_seconds`, with this
    /// relative path, still needs to be reported.
    #[must_use]
    pub(crate) fn is_due(&self, modified_unix_seconds: i64, relative_path: &str) -> bool {
        if modified_unix_seconds > self.high_water {
            return true;
        }
        if modified_unix_seconds == self.high_water {
            return self.wide || !self.at_high_water.contains(relative_path);
        }
        false
    }

    /// Encodes this state as an opaque cursor token.
    pub(crate) fn encode(&self) -> Result<CursorToken, GrammarError> {
        let mut text = format!("{TAG};hw={}", self.high_water);
        if self.wide {
            text.push_str(";wide=1");
        } else if !self.at_high_water.is_empty() {
            text.push_str(";at=");
            let mut first = true;
            for path in &self.at_high_water {
                if !first {
                    text.push(',');
                }
                first = false;
                text.push_str(&percent_encode(path));
            }
        }
        CursorToken::parse(&text)
    }

    /// Decodes a previously offered cursor.
    ///
    /// `None` means the text is not one of this Field's own cursors -- core
    /// already refuses to replay a cursor written at a different declared
    /// `cursor_format_version` (A2 section 9), so this only fires for a
    /// cursor that is corrupt despite matching this Field's own format
    /// version. Either way, the walk starts unbounded rather than trust a
    /// token it cannot read.
    #[must_use]
    pub(crate) fn decode(token: &CursorToken) -> Option<Self> {
        let text = token.as_str();
        let mut segments = text.split(';');
        if segments.next() != Some(TAG) {
            return None;
        }
        let mut state = CursorState::default();
        let mut saw_high_water = false;
        for segment in segments {
            if let Some(value) = segment.strip_prefix("hw=") {
                state.high_water = value.parse().ok()?;
                saw_high_water = true;
            } else if segment == "wide=1" {
                state.wide = true;
            } else {
                let value = segment.strip_prefix("at=")?;
                if value.is_empty() {
                    continue;
                }
                for encoded in value.split(',') {
                    state.at_high_water.insert(percent_decode(encoded)?);
                }
            }
        }
        if saw_high_water { Some(state) } else { None }
    }

    /// Builds the state a run that completed without an error should offer,
    /// from the maximum instant it actually observed across its whole scope
    /// (not merely the files it re-reported).
    ///
    /// `observed` is `None` for a scope with nothing in it at all, in which
    /// case the watermark cannot move and `previous` is returned unchanged.
    #[must_use]
    pub(crate) fn advance(
        previous: &CursorState,
        observed: Option<(i64, BTreeSet<String>)>,
    ) -> Self {
        let Some((max_seconds, at_max)) = observed else {
            return previous.clone();
        };
        match max_seconds.cmp(&previous.high_water) {
            std::cmp::Ordering::Less => previous.clone(),
            std::cmp::Ordering::Equal => {
                let mut merged = previous.at_high_water.clone();
                merged.extend(at_max);
                widen_if_needed(CursorState {
                    high_water: previous.high_water,
                    at_high_water: merged,
                    wide: previous.wide,
                })
            }
            std::cmp::Ordering::Greater => widen_if_needed(CursorState {
                high_water: max_seconds,
                at_high_water: at_max,
                wide: false,
            }),
        }
    }
}

fn widen_if_needed(mut state: CursorState) -> CursorState {
    if !state.wide && state.at_high_water.len() > MAX_TIE_BREAK_ENTRIES {
        state.wide = true;
        state.at_high_water.clear();
    }
    state
}

/// Percent-encodes every byte outside a small safe set, byte by byte so a
/// multi-byte UTF-8 sequence is never split incorrectly.
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        let safe = byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'-');
        if safe {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::CursorState;
    use std::collections::BTreeSet;

    #[test]
    fn a_fresh_state_treats_everything_as_due() {
        let state = CursorState::default();
        assert!(state.is_due(0, "a.txt"));
        assert!(state.is_due(1_700_000_000, "z/deep.txt"));
    }

    #[test]
    fn a_file_strictly_above_the_watermark_is_due() {
        let state = CursorState {
            high_water: 100,
            ..CursorState::default()
        };
        assert!(state.is_due(101, "a.txt"));
        assert!(!state.is_due(99, "a.txt"));
    }

    #[test]
    fn a_file_exactly_at_the_watermark_is_due_only_if_not_already_recorded() {
        let mut at_high_water = BTreeSet::new();
        at_high_water.insert("seen.txt".to_owned());
        let state = CursorState {
            high_water: 100,
            at_high_water,
            wide: false,
        };
        assert!(!state.is_due(100, "seen.txt"));
        assert!(state.is_due(100, "new-at-same-second.txt"));
    }

    #[test]
    fn a_wide_state_always_treats_the_watermark_second_as_due() {
        let state = CursorState {
            high_water: 100,
            at_high_water: BTreeSet::new(),
            wide: true,
        };
        assert!(state.is_due(100, "anything.txt"));
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let mut at_high_water = BTreeSet::new();
        at_high_water.insert("dir/with space, comma.txt".to_owned());
        at_high_water.insert("plain.txt".to_owned());
        let state = CursorState {
            high_water: 1_755_000_000,
            at_high_water,
            wide: false,
        };
        let token = state
            .encode()
            .unwrap_or_else(|error| panic!("must encode: {error}"));
        let decoded = CursorState::decode(&token).unwrap_or_else(|| panic!("must decode: {token}"));
        assert_eq!(decoded, state);
    }

    #[test]
    fn a_foreign_cursor_text_decodes_to_none() {
        let foreign = fieldnotes_field_protocol::grammar::Cursor::parse("graph-delta:v1:token")
            .unwrap_or_else(|error| panic!("must parse as a cursor at all: {error}"));
        assert_eq!(CursorState::decode(&foreign), None);
    }

    #[test]
    fn advance_never_moves_the_watermark_backwards() {
        let previous = CursorState {
            high_water: 500,
            at_high_water: BTreeSet::new(),
            wide: false,
        };
        let advanced = CursorState::advance(&previous, Some((100, BTreeSet::new())));
        assert_eq!(
            advanced.high_water, 500,
            "a lower observation must not regress the watermark"
        );
    }

    #[test]
    fn advance_widens_when_too_many_files_share_one_second() {
        let previous = CursorState::default();
        let mut many = BTreeSet::new();
        for index in 0..100 {
            many.insert(format!("file-{index}.txt"));
        }
        let advanced = CursorState::advance(&previous, Some((100, many)));
        assert!(advanced.wide);
        assert!(advanced.at_high_water.is_empty());
    }

    #[test]
    fn advance_with_no_observation_leaves_the_previous_state_unchanged() {
        let previous = CursorState {
            high_water: 42,
            at_high_water: BTreeSet::new(),
            wide: false,
        };
        assert_eq!(CursorState::advance(&previous, None), previous);
    }
}
