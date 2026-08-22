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

#[cfg(test)]
mod tests {
    use super::canonical_extension;

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
