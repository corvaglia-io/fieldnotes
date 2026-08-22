//! Content-addressed original-artifact storage.
//!
//! An original lives at `artifacts/<artifact-id>.<canonical-extension>`. The
//! artifact ID is the SHA-256 of the exact bytes, so identity is decided by
//! content and nothing else: the same bytes imported twice reuse the existing
//! file instead of rewriting it, and a source filename never selects the
//! stored extension.

use std::path::{Path, PathBuf};

use fieldnotes_domain::ArtifactId;
use fieldnotes_format::{artifact_id_for_bytes, canonical_extension};

use crate::atomic;
use crate::error::StoreError;
use crate::layout::Notebook;

/// The outcome of storing original bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredArtifact {
    /// The content-addressed artifact ID.
    pub id: ArtifactId,
    /// The absolute path of the stored original.
    pub path: PathBuf,
    /// The notebook-relative path, as referenced from a Note body.
    pub relative_path: String,
    /// Whether the bytes were already present and the existing file was reused.
    pub reused: bool,
}

/// Locates an already-stored artifact by ID.
///
/// The expected canonical filename is checked first. A directory scan follows,
/// because the same bytes may have been stored earlier under a different
/// canonical extension (an import that supplied no detectable media type used
/// `.bin`). Identity is the ID, so any file whose stem is this ID is the
/// artifact and is reused rather than duplicated.
pub fn find_artifact(
    notebook: &Notebook,
    id: &ArtifactId,
    media_type: Option<&str>,
) -> Result<Option<PathBuf>, StoreError> {
    let directory = notebook.artifacts_dir();
    let expected = directory.join(format!("{id}.{}", canonical_extension(media_type)));
    if expected.is_file() {
        return Ok(Some(expected));
    }
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StoreError::io("read directory", &directory, error)),
    };
    let stem = id.to_string();
    let mut matches: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io("read directory", &directory, error))?;
        let path = entry.path();
        if path.file_stem().and_then(|value| value.to_str()) == Some(stem.as_str())
            && path.is_file()
        {
            matches.push(path);
        }
    }
    // Deterministic choice when more than one extension exists for one ID.
    matches.sort();
    Ok(matches.into_iter().next())
}

/// Stores original bytes, reusing an existing file with the same artifact ID.
///
/// `media_type` is the deterministically detected type, if any; the canonical
/// extension registry maps an unknown or absent type to `.bin`.
pub fn store_artifact(
    notebook: &Notebook,
    bytes: &[u8],
    media_type: Option<&str>,
) -> Result<StoredArtifact, StoreError> {
    let id = artifact_id_for_bytes(bytes);
    let directory = notebook.artifacts_dir();
    if let Some(existing) = find_artifact(notebook, &id, media_type)? {
        return Ok(StoredArtifact {
            id,
            relative_path: notebook.relative_display(&existing),
            path: existing,
            reused: true,
        });
    }
    std::fs::create_dir_all(&directory)
        .map_err(|error| StoreError::io("create directory", &directory, error))?;
    let filename = format!("{id}.{}", canonical_extension(media_type));
    let path = atomic::write_atomic(&directory, &filename, bytes)?;
    Ok(StoredArtifact {
        id,
        relative_path: notebook.relative_display(&path),
        path,
        reused: false,
    })
}

/// Verifies that a stored artifact's bytes still hash to its filename ID.
///
/// Returns the byte length on success.
pub fn verify_artifact(path: &Path) -> Result<u64, StoreError> {
    let bytes =
        std::fs::read(path).map_err(|error| StoreError::io("read artifact", path, error))?;
    let corrupt = || StoreError::ArtifactCorrupt {
        path: path.to_path_buf(),
    };
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let expected = ArtifactId::parse(stem).map_err(|_| corrupt())?;
    if artifact_id_for_bytes(&bytes) != expected {
        return Err(corrupt());
    }
    u64::try_from(bytes.len()).map_err(|_| corrupt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_test_support::TempDir;

    fn notebook(label: &str) -> Result<(TempDir, Notebook), StoreError> {
        let temp = TempDir::new(label)
            .map_err(|error| StoreError::io("create temporary directory", ".", error))?;
        let (notebook, _) = Notebook::create(&temp.path().join("notebook"))?;
        Ok((temp, notebook))
    }

    #[test]
    fn identical_bytes_reuse_the_existing_file() -> Result<(), StoreError> {
        let (_temp, notebook) = notebook("artifact-reuse")?;
        let bytes = b"\x89PNG\r\n\x1a\npayload";
        let first = store_artifact(&notebook, bytes, Some("image/png"))?;
        assert!(!first.reused);
        assert!(first.relative_path.starts_with("artifacts/"));
        assert!(first.relative_path.ends_with(".png"));
        let modified = std::fs::metadata(&first.path)
            .and_then(|metadata| metadata.modified())
            .map_err(|error| StoreError::io("read metadata", &first.path, error))?;

        let second = store_artifact(&notebook, bytes, Some("image/png"))?;
        assert!(second.reused);
        assert_eq!(second.path, first.path);
        let modified_again = std::fs::metadata(&second.path)
            .and_then(|metadata| metadata.modified())
            .map_err(|error| StoreError::io("read metadata", &second.path, error))?;
        assert_eq!(modified, modified_again, "the file must not be rewritten");

        // One artifact file exists for these bytes.
        let count = std::fs::read_dir(notebook.artifacts_dir())
            .map_err(|error| StoreError::io("read directory", notebook.root(), error))?
            .count();
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn a_different_extension_claim_reuses_the_same_id() -> Result<(), StoreError> {
        let (_temp, notebook) = notebook("artifact-extension")?;
        let bytes = b"arbitrary bytes";
        let first = store_artifact(&notebook, bytes, None)?;
        assert!(first.relative_path.ends_with(".bin"));
        let second = store_artifact(&notebook, bytes, Some("image/png"))?;
        assert!(second.reused);
        assert_eq!(second.path, first.path);
        Ok(())
    }

    #[test]
    fn verification_detects_tampered_bytes() -> Result<(), StoreError> {
        let (_temp, notebook) = notebook("artifact-verify")?;
        let stored = store_artifact(&notebook, b"%PDF-1.7\n", Some("application/pdf"))?;
        assert_eq!(verify_artifact(&stored.path)?, 9);
        std::fs::write(&stored.path, b"tampered")
            .map_err(|error| StoreError::io("write", &stored.path, error))?;
        assert!(matches!(
            verify_artifact(&stored.path),
            Err(StoreError::ArtifactCorrupt { .. })
        ));
        Ok(())
    }
}
