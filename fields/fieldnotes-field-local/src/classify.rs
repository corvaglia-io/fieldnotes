//! Mapping a collected file onto a capability slice, and content-only media
//! type detection.
//!
//! # The media-type question this Field does not decide
//!
//! A1 section 2 states that a source filename never selects the stored
//! extension, and content detection alone cannot distinguish the Office and
//! OpenDocument formats -- or CSV -- from a plain ZIP or arbitrary text
//! (ADR 0008). Declaring `local_media_type` (or an artifact's `media_type`)
//! from a filename would let core's extension registry select the stored
//! extension by filename indirection, which is exactly what A1 forbids.
//!
//! **This Field resolves that tension conservatively: it declares a media
//! type only when magic bytes determine it unambiguously, and leaves it
//! absent otherwise.** This is most visible for `.docx`, `.xlsx`, `.pptx`,
//! and every OpenDocument format, whose shared ZIP-family magic bytes this
//! Field deliberately declines to interpret further -- so a collected
//! `.docx` file gets no declared media type at all from this release, and
//! core falls back to the `.bin` extension per A1 section 2 rather than
//! `.docx`. See the crate's final report for the coordinator question this
//! leaves open and the alternatives available.

/// The capability slice one collected file belongs to: its connector-local
/// object kind and the A1 primary Note type its manifest slice declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Capability {
    /// The connector-local object-kind token.
    pub(crate) object_kind: &'static str,
    /// The primary Note type this slice maps to.
    pub(crate) note_type: &'static str,
}

/// Extensions (lowercase, without the leading dot) of file formats whose own
/// document identity is primary: office documents and PDFs. Everything else
/// maps to the generic `file` slice.
const DOCUMENT_EXTENSIONS: [&str; 11] = [
    "pdf", "doc", "docx", "odt", "rtf", "ppt", "pptx", "odp", "xls", "xlsx", "ods",
];

/// Classifies a collected file by its extension alone.
///
/// This is a Field-local vendor-mapping decision -- which capability slice a
/// vendor object belongs to -- and is unrelated to A1 section 2's stored-
/// extension rule: no notebook path, filename, or extension is ever derived
/// from this classification. Core's canonical extension always comes from
/// its own media-type registry, never from this function's input or output.
#[must_use]
pub(crate) fn classify(relative_path: &str) -> Capability {
    let extension = std::path::Path::new(relative_path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some(extension) if DOCUMENT_EXTENSIONS.contains(&extension) => Capability {
            object_kind: crate::constants::OBJECT_KIND_DOCUMENT,
            note_type: crate::constants::OBJECT_KIND_DOCUMENT,
        },
        _ => Capability {
            object_kind: crate::constants::OBJECT_KIND_FILE,
            note_type: crate::constants::OBJECT_KIND_FILE,
        },
    }
}

/// Detects a media type from file bytes alone, never from a filename.
///
/// Returns `None` rather than guess whenever the bytes are ambiguous: most
/// notably the ZIP-family magic bytes shared by `.docx`, `.xlsx`, `.pptx`,
/// every OpenDocument format, and a plain `.zip`. See the module
/// documentation for why.
#[must_use]
pub(crate) fn sniff_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // The ZIP family: `.docx`, `.xlsx`, `.pptx`, every OpenDocument format,
    // and a plain `.zip` all share this magic. Content alone cannot tell
    // them apart (ADR 0008), so no media type is declared for it.
    let is_zip_family = bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04])
        || bytes.starts_with(&[0x50, 0x4B, 0x05, 0x06])
        || bytes.starts_with(&[0x50, 0x4B, 0x07, 0x08]);
    if is_zip_family {
        return None;
    }
    if is_plain_text(bytes) {
        return Some("text/plain");
    }
    None
}

/// Whether `bytes` is valid UTF-8 with no NUL byte -- the only content-only
/// signal this Field trusts to call something "text". It cannot tell
/// Markdown from plain text without the filename, so it never claims
/// `text/markdown`.
fn is_plain_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{classify, sniff_media_type};

    #[test]
    fn classification_is_extension_based_and_defaults_to_file() {
        assert_eq!(classify("readme.md").object_kind, "file");
        assert_eq!(classify("report.PDF").object_kind, "document");
        assert_eq!(classify("no-extension").object_kind, "file");
        assert_eq!(classify("archive.docx").object_kind, "document");
    }

    #[test]
    fn classification_pairs_object_kind_with_a_matching_note_type() {
        let file = classify("notes.txt");
        assert_eq!(file.object_kind, file.note_type);
        let document = classify("spec.pdf");
        assert_eq!(document.object_kind, document.note_type);
    }

    #[test]
    fn media_type_sniffing_never_guesses_the_zip_family() {
        assert_eq!(sniff_media_type(b"%PDF-1.4"), Some("application/pdf"));
        assert_eq!(sniff_media_type(b"hello world"), Some("text/plain"));
        assert_eq!(sniff_media_type(&[0x50, 0x4B, 0x03, 0x04, 0, 0]), None);
        assert_eq!(sniff_media_type(&[0x00, 0x01, 0x02]), None);
    }

    #[test]
    fn media_type_sniffing_recognizes_common_image_magic_bytes() {
        assert_eq!(
            sniff_media_type(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]),
            Some("image/png")
        );
        assert_eq!(
            sniff_media_type(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(sniff_media_type(b"GIF89a"), Some("image/gif"));
    }
}
