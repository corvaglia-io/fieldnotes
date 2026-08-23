//! Deriving the portable exact-source scope for one mailbox.
//!
//! The shape is `microsoft-graph:tenant/<tenant-id>`, which is exactly what
//! the frozen `outlook_mail_work` fixtures in
//! `tests/fixtures/notebooks/proposed-v1/notes/` already show, and what the
//! sibling Microsoft Fields' fixtures share.
//!
//! # Why this is portable, non-secret, and label-independent
//!
//! A tenant identifier is a public directory identifier: it appears in
//! sign-in URLs and in every token audience, it is not a credential, and it
//! identifies an authority rather than a person. It does not depend on the
//! user's local Field label, so two Fieldnotes instances configured against
//! the same mailbox derive the identical scope and their Notes deduplicate
//! exactly, which is the whole point of A2 section 7's
//! `scope_depends_on_field_label: false`.
//!
//! # Why the scope is the tenant rather than the mailbox
//!
//! Graph mail identifiers are opaque per-mailbox item identifiers: the same
//! message delivered to two mailboxes in one tenant has two different
//! identifiers, and is genuinely two source objects. Scoping by tenant
//! therefore cannot collide, while scoping by mailbox would need an identifier
//! a `Mail.Read`-only token cannot read (see [`crate::config`]). It also keeps
//! this Field's scope identical in shape to the Calendar, Contacts, and Teams
//! Fields, which read the same authority.

/// Builds the portable exact-source scope for `tenant_id`.
///
/// `tenant_id` is already validated and lowercased by [`crate::config`].
#[must_use]
pub(crate) fn compute(tenant_id: &str) -> String {
    format!("{}/{tenant_id}", crate::constants::SCOPE_NAMESPACE)
}

#[cfg(test)]
mod tests {
    use super::compute;

    const TENANT: &str = "8d820000-0000-7000-8000-000000000001";

    #[test]
    fn the_scope_matches_the_frozen_fixture_shape() {
        assert_eq!(
            compute(TENANT),
            "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001"
        );
    }

    #[test]
    fn the_same_tenant_always_computes_the_same_scope() {
        assert_eq!(compute(TENANT), compute(TENANT));
    }

    #[test]
    fn different_tenants_compute_different_scopes() {
        assert_ne!(
            compute(TENANT),
            compute("8d820000-0000-7000-8000-000000000002")
        );
    }

    #[test]
    fn the_computed_scope_satisfies_the_transport_guard() {
        assert!(fieldnotes_field_protocol::grammar::SourceScope::parse(&compute(TENANT)).is_ok());
    }
}
