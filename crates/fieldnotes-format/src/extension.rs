//! The frozen A1 canonical media-type-to-extension registry, extended by
//! [ADR 0008](../../../docs/decisions/0008-extend-canonical-extension-registry.md).
//!
//! Media-type parameters are removed and the type/subtype is ASCII-lowercased
//! before exact lookup. Unknown, unavailable, or conflicting detected types
//! use `bin`; a source filename never selects the stored extension. Adding a
//! row is a pure addition: it never changes an already-assigned extension, so
//! it cannot invalidate a previously stored original.

/// The frozen v0.1 mapping from media type to canonical extension (no dot),
/// extended per
/// [ADR 0008](../../../docs/decisions/0008-extend-canonical-extension-registry.md)
/// with the nine default-retention document/image types that had no entry.
const REGISTRY: [(&str, &str); 24] = [
    ("application/json", "json"),
    ("application/pdf", "pdf"),
    ("application/rtf", "rtf"),
    ("application/vnd.oasis.opendocument.presentation", "odp"),
    ("application/vnd.oasis.opendocument.spreadsheet", "ods"),
    ("application/vnd.oasis.opendocument.text", "odt"),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pptx",
    ),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsx",
    ),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "docx",
    ),
    ("application/zip", "zip"),
    ("audio/mp4", "m4a"),
    ("audio/mpeg", "mp3"),
    ("audio/ogg", "ogg"),
    ("audio/wav", "wav"),
    ("image/gif", "gif"),
    ("image/heic", "heic"),
    ("image/jpeg", "jpg"),
    ("image/png", "png"),
    ("image/svg+xml", "svg"),
    ("image/webp", "webp"),
    ("text/csv", "csv"),
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
    // brands are audio/mp4, HEIF/HEIC brands are image/heic, and every other
    // recognized brand is video/mp4.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"M4A " || brand == b"M4B " || brand == b"M4P " {
            return Some("audio/mp4");
        }
        if matches!(
            brand,
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1"
        ) {
            return Some("image/heic");
        }
        return Some("video/mp4");
    }
    if starts(bytes, b"PK\x03\x04") {
        return Some("application/zip");
    }
    // RTF is a plain-text control-word format, but its fixed five-byte header
    // is reliable enough to identify without falling through to the generic
    // text check below (which cannot distinguish RTF from any other text).
    if starts(bytes, b"{\\rtf1") {
        return Some("application/rtf");
    }
    // Text is the last resort: valid UTF-8 with no control characters other
    // than tab, CR, and LF. Markdown, JSON, CSV, and SVG are not distinguished
    // from plain text by content alone, so they stay `text/plain`. The Office
    // Open XML and OpenDocument formats are also indistinguishable from a
    // plain `application/zip` by magic bytes alone: telling them apart
    // requires reading the archive's `[Content_Types].xml` or `mimetype`
    // member, which this function deliberately does not do (see ADR 0008).
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

    /// Every media type ADR 0008 added, and the extension it must resolve to.
    const ADR_0008_ADDITIONS: [(&str, &str); 9] = [
        (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        ),
        (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
        ),
        (
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "pptx",
        ),
        ("application/vnd.oasis.opendocument.text", "odt"),
        ("application/vnd.oasis.opendocument.spreadsheet", "ods"),
        ("application/vnd.oasis.opendocument.presentation", "odp"),
        ("text/csv", "csv"),
        ("application/rtf", "rtf"),
        ("image/heic", "heic"),
    ];

    #[test]
    fn adr_0008_adds_nine_document_and_image_mappings() {
        for (media_type, extension) in ADR_0008_ADDITIONS {
            assert_eq!(
                canonical_extension(Some(media_type)),
                extension,
                "{media_type} must map to .{extension}"
            );
            // The lookup rule (strip parameters, ASCII-lowercase) applies
            // identically to every new row, not only the pre-existing ones.
            let with_params = format!("{media_type}; foo=bar");
            assert_eq!(
                canonical_extension(Some(&with_params)),
                extension,
                "{media_type} must still resolve with parameters present"
            );
            let uppercased = media_type.to_ascii_uppercase();
            assert_eq!(
                canonical_extension(Some(&uppercased)),
                extension,
                "{media_type} must still resolve when uppercased"
            );
        }
    }

    #[test]
    fn an_unlisted_media_type_still_falls_back_to_bin() {
        // A type that is deliberately never registered, so the `.bin`
        // fallback keeps applying to material outside the frozen registry
        // even after ADR 0008's additions.
        assert_eq!(
            canonical_extension(Some("application/x-still-unregistered")),
            "bin"
        );
        // Legacy binary Office formats were deliberately excluded from the
        // ADR 0008 additions (see the ADR's consequences) and must still
        // fall back to `.bin`.
        assert_eq!(canonical_extension(Some("application/msword")), "bin");
    }

    #[test]
    fn heic_is_detected_from_the_ftyp_brand_and_not_confused_with_video() {
        // A real HEIC photo's ftyp box: major brand `heic`.
        assert_eq!(
            detect_media_type(b"\0\0\0\x18ftypheic\0\0\0\0"),
            Some("image/heic")
        );
        // The generic HEIF container brand is also recognized.
        assert_eq!(
            detect_media_type(b"\0\0\0\x18ftypmif1\0\0\0\0"),
            Some("image/heic")
        );
        // An unrelated ftyp brand still falls to the video/mp4 catch-all,
        // proving the new brand check did not widen to match everything.
        assert_eq!(
            detect_media_type(b"\0\0\0\x18ftypisom\0\0\0\0"),
            Some("video/mp4")
        );
    }

    #[test]
    fn rtf_is_detected_by_its_fixed_header() {
        assert_eq!(
            detect_media_type(br"{\rtf1\ansi\deff0 Hello.}"),
            Some("application/rtf")
        );
    }

    #[test]
    fn office_and_opendocument_formats_are_not_distinguishable_from_zip_by_content() {
        // Office Open XML and OpenDocument files are ZIP containers with the
        // same local-file-header magic bytes as a plain .zip; telling them
        // apart requires reading an archive member (`[Content_Types].xml` or
        // `mimetype`), which detection deliberately does not do (ADR 0008).
        // A registry row exists for these types, but content sniffing alone
        // can never select it -- only a Field or importer that declares the
        // media type explicitly can.
        assert_eq!(
            detect_media_type(b"PK\x03\x04\x14\0\0\0\0\0"),
            Some("application/zip")
        );
    }

    #[test]
    fn csv_has_no_reliable_content_signature_and_reads_as_plain_text() {
        // CSV has no magic bytes: it is ordinary text, so content-only
        // detection cannot distinguish it from `text/plain`. The `.csv`
        // registry row therefore only helps a caller that already knows the
        // declared media type; detection alone never produces it.
        assert_eq!(detect_media_type(b"name,age\nAda,36\n"), Some("text/plain"));
    }
}
