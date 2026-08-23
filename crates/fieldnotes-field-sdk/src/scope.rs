//! Deriving a stable, non-secret scope from a local identifier.
//!
//! A Field's portable exact-source scope must be stable across runs against
//! the same upstream root or account, and free of anything user-identifying
//! -- a home-directory path segment, an account's own display name. Hashing
//! the identifier, rather than embedding it verbatim, satisfies both: the
//! identifier never appears in the returned scope, and the same identifier
//! always derives the same scope.
//!
//! This is the pattern `fields/fieldnotes-field-local` uses for its
//! `local-root:<sha256>` scope; the prefix and the identifier bytes are
//! supplied by the caller, so any Field can reuse the derivation without
//! adopting `local`'s specific prefix or identifier shape.

use sha2::{Digest, Sha256};

/// Computes `<prefix>:<sha256-hex-of-identifier>`.
///
/// `prefix` names the derivation, written verbatim followed by a colon, so
/// two Fields -- or two derivations within one Field -- cannot collide even
/// if `identifier` happens to coincide. `identifier` is hashed, never
/// embedded, so the derived scope carries no part of it: for example, a
/// canonical filesystem path's user-identifying segments never appear in the
/// scope this returns.
#[must_use]
pub fn derive(prefix: &str, identifier: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(identifier);
    let digest = hasher.finalize();
    format!("{prefix}:{}", crate::hex::to_lower_hex(&digest))
}

#[cfg(test)]
mod tests {
    use super::derive;

    #[test]
    fn the_same_identifier_always_derives_the_same_scope() {
        assert_eq!(
            derive("local-root", b"/tmp/example-root"),
            derive("local-root", b"/tmp/example-root")
        );
    }

    #[test]
    fn different_identifiers_derive_different_scopes() {
        let first = derive("local-root", b"/tmp/root-one");
        let second = derive("local-root", b"/tmp/root-two");
        assert_ne!(first, second);
    }

    #[test]
    fn different_prefixes_derive_different_scopes_for_the_same_identifier() {
        assert_ne!(
            derive("local-root", b"same-identifier"),
            derive("other-root", b"same-identifier")
        );
    }

    #[test]
    fn the_scope_never_embeds_the_raw_identifier() {
        let scope = derive("local-root", b"/home/samkeller/reference-library");
        assert!(!scope.contains("samkeller"));
        assert!(scope.starts_with("local-root:"));
    }
}
