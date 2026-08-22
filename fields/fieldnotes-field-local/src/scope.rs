//! Deriving the portable exact-source scope from the configured root.

use std::path::Path;

use sha2::{Digest, Sha256};

/// Computes the non-secret, per-root-stable portable exact-source scope.
///
/// Hashing the canonical root path, rather than embedding it verbatim, keeps
/// the scope free of any user-identifying path segment (a home-directory user
/// name, for instance) while remaining perfectly stable across repeated runs
/// against the same configured root. It is not portable across a root that
/// is later moved or renamed -- refetching under a fresh scope after that is
/// the expected recovery, exactly as for any other Field whose scope
/// derivation depends on a stable upstream identifier.
#[must_use]
pub(crate) fn compute(canonical_root: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_root.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    format!("local-root:{}", crate::hexutil::to_hex(&digest))
}

#[cfg(test)]
mod tests {
    use super::compute;
    use std::path::Path;

    #[test]
    fn the_same_root_always_computes_the_same_scope() {
        let root = Path::new("/tmp/example-root");
        assert_eq!(compute(root), compute(root));
    }

    #[test]
    fn different_roots_compute_different_scopes() {
        let first = compute(Path::new("/tmp/root-one"));
        let second = compute(Path::new("/tmp/root-two"));
        assert_ne!(first, second);
    }

    #[test]
    fn the_scope_never_embeds_the_raw_path() {
        let root = Path::new("/home/jgcorvaglia/reference-library");
        let scope = compute(root);
        assert!(!scope.contains("jgcorvaglia"));
        assert!(scope.starts_with("local-root:"));
    }
}
