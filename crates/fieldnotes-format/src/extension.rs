//! The frozen A1 canonical media-type-to-extension registry.
//!
//! Media-type parameters are removed and the type/subtype is ASCII-lowercased
//! before exact lookup. Unknown, unavailable, or conflicting detected types
//! use `bin`; a source filename never selects the stored extension.

/// The frozen v0.1 mapping from media type to canonical extension (no dot).
const REGISTRY: [(&str, &str); 15] = [
    ("application/json", "json"),
    ("application/pdf", "pdf"),
    ("application/zip", "zip"),
    ("audio/mp4", "m4a"),
    ("audio/mpeg", "mp3"),
    ("audio/ogg", "ogg"),
    ("audio/wav", "wav"),
    ("image/gif", "gif"),
    ("image/jpeg", "jpg"),
    ("image/png", "png"),
    ("image/svg+xml", "svg"),
    ("image/webp", "webp"),
    ("text/markdown", "md"),
    ("text/plain", "txt"),
    ("video/mp4", "mp4"),
];

/// The canonical extension (without a leading dot) for a detected media type.
///
/// `None` (unavailable) or any unlisted type selects `bin`.
#[must_use]
pub fn canonical_extension(media_type: Option<&str>) -> &'static str {
    let Some(media_type) = media_type else {
        return "bin";
    };
    // Strip parameters, trim surrounding whitespace, ASCII-lowercase.
    let essence = media_type.split(';').next().unwrap_or("").trim();
    let lowered = essence.to_ascii_lowercase();
    REGISTRY
        .iter()
        .find(|(name, _)| *name == lowered)
        .map_or("bin", |(_, extension)| extension)
}

/// Deterministically detects the media type of original bytes from their
/// content alone.
///
/// A source filename never participates: A1 forbids a filename from selecting
/// the stored extension, so detection reads magic numbers only, plus a final
/// `text/plain` classification for byte sequences that are valid UTF-8 without
/// control characters. `None` means "no reliable type", which
/// [`canonical_extension`] maps to `bin`.
#[must_use]
pub fn detect_media_type(bytes: &[u8]) -> Option<&'static str> {
    /// Whether `bytes` starts with `prefix`.
    fn starts(bytes: &[u8], prefix: &[u8]) -> bool {
        bytes.len() >= prefix.len() && &bytes[..prefix.len()] == prefix
    }
    /// Whether a RIFF container declares the given four-byte form type.
    fn riff_form(bytes: &[u8], form: &[u8; 4]) -> bool {
        starts(bytes, b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == form
    }

    if starts(bytes, b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if starts(bytes, b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    if starts(bytes, b"GIF87a") || starts(bytes, b"GIF89a") {
        return Some("image/gif");
    }
    if riff_form(bytes, b"WEBP") {
        return Some("image/webp");
    }
    if riff_form(bytes, b"WAVE") {
        return Some("audio/wav");
    }
    if starts(bytes, b"%PDF-") {
        return Some("application/pdf");
    }
    if starts(bytes, b"OggS") {
        return Some("audio/ogg");
    }
    if starts(bytes, b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0) {
        return Some("audio/mpeg");
    }
    // ISO base media: `ftyp` at offset 4, then a four-byte brand. Audio-only
    // brands are audio/mp4; every other recognized brand is video/mp4.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"M4A " || brand == b"M4B " || brand == b"M4P " {
            return Some("audio/mp4");
        }
        return Some("video/mp4");
    }
    if starts(bytes, b"PK\x03\x04") {
        return Some("application/zip");
    }
    // Text is the last resort: valid UTF-8 with no control characters other
    // than tab, CR, and LF. Markdown, JSON, and SVG are not distinguished from
    // plain text by content alone, so they stay `text/plain`.
    let text = core::str::from_utf8(bytes).ok()?;
    if text.is_empty() {
        return None;
    }
    if text
        .chars()
        .all(|c| !c.is_control() || matches!(c, '\t' | '\r' | '\n'))
    {
        return Some("text/plain");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{canonical_extension, detect_media_type};

    #[test]
    fn detects_types_from_content_only() {
        assert_eq!(detect_media_type(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(
            detect_media_type(b"\xff\xd8\xff\xe0rest"),
            Some("image/jpeg")
        );
        assert_eq!(detect_media_type(b"%PDF-1.7\n"), Some("application/pdf"));
        assert_eq!(detect_media_type(b"OggS\0\x02"), Some("audio/ogg"));
        assert_eq!(detect_media_type(b"ID3\x03\0\0"), Some("audio/mpeg"));
        assert_eq!(
            detect_media_type(b"RIFF\x24\0\0\0WAVEfmt "),
            Some("audio/wav")
        );
        assert_eq!(
            detect_media_type(b"\0\0\0\x18ftypM4A \0\0\0\0"),
            Some("audio/mp4")
        );
        assert_eq!(
            detect_media_type(b"\0\0\0\x18ftypisom\0\0\0\0"),
            Some("video/mp4")
        );
        assert_eq!(
            detect_media_type("plain text\n".as_bytes()),
            Some("text/plain")
        );
        assert_eq!(detect_media_type("Grüsse\n".as_bytes()), Some("text/plain"));
        // No reliable type: arbitrary binary and empty input both fall to bin.
        assert_eq!(detect_media_type(&[0x00, 0x01, 0x02, 0x03]), None);
        assert_eq!(detect_media_type(b""), None);
        assert_eq!(canonical_extension(detect_media_type(b"")), "bin");
    }

    #[test]
    fn lookup_strips_parameters_and_lowercases() {
        assert_eq!(canonical_extension(Some("image/jpeg")), "jpg");
        assert_eq!(canonical_extension(Some("Image/JPEG; q=0.5")), "jpg");
        assert_eq!(
            canonical_extension(Some("text/plain; charset=utf-8")),
            "txt"
        );
        assert_eq!(canonical_extension(Some("image/svg+xml")), "svg");
        assert_eq!(canonical_extension(Some("application/x-unknown")), "bin");
        assert_eq!(canonical_extension(None), "bin");
    }
}
