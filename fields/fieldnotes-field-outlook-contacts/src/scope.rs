//! Deriving the portable exact-source key's scope half, and one contact's
//! identity half.
//!
//! Matching the frozen `outlook_contacts_work` fixture
//! (`tests/fixtures/notebooks/proposed-v1/notes/`), the scope is the tenant
//! -- not the mailbox -- because a Graph contact ID is already unique within
//! its own mailbox and Graph never reuses one across mailboxes, so scoping
//! at the tenant is sufficient to keep two different mailboxes' contacts
//! distinct while remaining stable if a mailbox is ever renamed or aliased.

use fieldnotes_field_protocol::grammar::{SourceIdentity, SourceScope};

/// Why a scope or identity value could not be built.
#[derive(Debug)]
pub(crate) struct ScopeError(pub(crate) String);

impl std::fmt::Display for ScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScopeError {}

/// Computes the portable exact-source scope for a tenant.
pub(crate) fn compute(tenant_id: &str) -> Result<SourceScope, ScopeError> {
    SourceScope::parse(&format!("microsoft-graph:tenant/{tenant_id}"))
        .map_err(|error| ScopeError(format!("source scope guard: {error}")))
}

/// Computes one contact's identity within its scope: `contact/<graph-id>`,
/// embedding the object kind per A2 section 7's
/// `identity_includes_object_kind: true` constant.
pub(crate) fn identity_of(graph_contact_id: &str) -> Result<SourceIdentity, ScopeError> {
    SourceIdentity::parse(&format!(
        "{}/{graph_contact_id}",
        crate::constants::OBJECT_KIND_CONTACT
    ))
    .map_err(|error| ScopeError(format!("source identity guard: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{compute, identity_of};

    #[test]
    fn scope_matches_the_frozen_fixture_shape() {
        let scope = compute("8d820000-0000-7000-8000-000000000001")
            .unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(
            scope.as_str(),
            "microsoft-graph:tenant/8d820000-0000-7000-8000-000000000001"
        );
    }

    #[test]
    fn identity_embeds_the_object_kind() {
        let identity =
            identity_of("AAMkAGI2CONTACT01").unwrap_or_else(|error| panic!("must build: {error}"));
        assert_eq!(identity.as_str(), "contact/AAMkAGI2CONTACT01");
    }
}
