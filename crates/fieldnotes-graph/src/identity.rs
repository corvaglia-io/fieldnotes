//! Deterministic identity normalization: namespaced anchor keys, their declared
//! matching scope, and exactly what the resolver refuses to treat as exact.
//!
//! An identity anchor is a pair of mandatory namespace and normalized value,
//! written flat as `namespace:value` in notebook frontmatter
//! (`email:alice@example.com`, `phone:+41441234567`). Fieldnotes never joins two
//! source-local values merely because their unqualified strings match, so every
//! namespace carries a declared [`ScopeClass`], a [`Strength`], and a versioned
//! [`NormalizationRule`]. A value that cannot be normalized, or whose namespace
//! is not declared, stays unresolved and is reported as a gap rather than being
//! guessed at.
//!
//! # What normalization deliberately does not do
//!
//! - It never lowercases or otherwise folds an opaque source ID, because two
//!   distinct upstream IDs can differ only by case.
//! - It never strips address tags (`alice+news@`) or dots inside a mail local
//!   part. Those rules are provider-specific, and applying them globally would
//!   merge mailboxes that are genuinely different.
//! - It never accepts a phone number without country context: a national-format
//!   number is ambiguous between countries, so it is refused instead of guessed.
//! - It never treats a display name, organization label, subject, or role text
//!   as an identity. Those are weak descriptive values and can only produce a
//!   review candidate.

use core::fmt;
use std::collections::BTreeMap;

use crate::entity::EntityKind;

/// The declared matching scope of an identity namespace.
///
/// The classes mirror the scope classes in `docs/identity-and-graph.md` and the
/// A2 `IdentityScopeClass` a Field declares for the anchors it emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeClass {
    /// The upstream system documents the value as unique across that system.
    SourceGlobal,
    /// Exact only within a declared tenant, site, server, or account.
    AuthorityScoped,
    /// Exact only within a named service or configured namespace.
    NamespaceScoped,
    /// A channel identity matched through a versioned normalization rule.
    NormalizedChannel,
    /// An unverified human description; never exact.
    WeakDescriptive,
}

impl ScopeClass {
    /// Whether an anchor in this class must name the scope it is exact within.
    #[must_use]
    pub fn requires_scope(self) -> bool {
        matches!(
            self,
            ScopeClass::AuthorityScoped | ScopeClass::NamespaceScoped
        )
    }

    /// The stable lowercase label used in explanations.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ScopeClass::SourceGlobal => "source-global",
            ScopeClass::AuthorityScoped => "authority-scoped",
            ScopeClass::NamespaceScoped => "namespace-scoped",
            ScopeClass::NormalizedChannel => "normalized-channel",
            ScopeClass::WeakDescriptive => "weak-descriptive",
        }
    }
}

impl fmt::Display for ScopeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How strong an anchor is, which decides whether a deterministic rule may
/// join automatically or must leave a review candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Strength {
    /// A documented source object, user, or account ID inside its own scope.
    Exact,
    /// A source-authoritative normalized channel identity.
    Strong,
    /// A stable service username or login; needs an additional approved rule.
    Medium,
    /// A display name or other unverified description; candidates only.
    Weak,
}

impl Strength {
    /// Whether two occurrences of the same normalized value may be joined
    /// without human review.
    #[must_use]
    pub fn permits_auto_merge(self) -> bool {
        matches!(self, Strength::Exact | Strength::Strong)
    }

    /// The stable lowercase label used in explanations.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Strength::Exact => "exact",
            Strength::Strong => "strong",
            Strength::Medium => "medium",
            Strength::Weak => "weak",
        }
    }
}

impl fmt::Display for Strength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A versioned deterministic normalization rule for anchor values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NormalizationRule {
    /// `email-normalize-v1`: trim, unwrap one pair of angle brackets, require a
    /// single `@` with a dot-atom local part and a dotted domain, then
    /// ASCII-lowercase both parts. No tag or dot stripping.
    EmailV1,
    /// `phone-e164-v1`: remove ASCII spaces, `-`, `.`, `(`, `)`, and
    /// non-breaking spaces, then require `+` and 7 to 15 digits.
    PhoneE164V1,
    /// `domain-v1`: ASCII-lowercase, drop one trailing dot, require non-empty
    /// dot-separated labels.
    DomainV1,
    /// `sha256-digest-v1`: require exactly 64 lowercase hexadecimal digits.
    Sha256DigestV1,
    /// `opaque-token-v1`: require a printable, whitespace-free value and change
    /// nothing else, because case-folding an opaque source ID can merge two
    /// distinct upstream objects.
    OpaqueTokenV1,
}

/// The largest anchor value the resolver accepts, matching the A2 transport
/// bound on an identity-anchor value.
pub const MAX_VALUE_BYTES: usize = 1024;

impl NormalizationRule {
    /// The stable rule name recorded in explanations.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            NormalizationRule::EmailV1 => "email-normalize-v1",
            NormalizationRule::PhoneE164V1 => "phone-e164-v1",
            NormalizationRule::DomainV1 => "domain-v1",
            NormalizationRule::Sha256DigestV1 => "sha256-digest-v1",
            NormalizationRule::OpaqueTokenV1 => "opaque-token-v1",
        }
    }

    /// Applies the rule, or explains why the value stays unresolved.
    pub fn apply(self, raw: &str) -> Result<String, RefusalReason> {
        let trimmed = raw.trim_matches(|c: char| c == ' ' || c == '\t');
        if trimmed.is_empty() {
            return Err(RefusalReason::EmptyValue);
        }
        if trimmed.len() > MAX_VALUE_BYTES {
            return Err(RefusalReason::ValueTooLong);
        }
        match self {
            NormalizationRule::EmailV1 => normalize_email(trimmed),
            NormalizationRule::PhoneE164V1 => normalize_phone(trimmed),
            NormalizationRule::DomainV1 => normalize_domain(trimmed),
            NormalizationRule::Sha256DigestV1 => normalize_digest(trimmed),
            NormalizationRule::OpaqueTokenV1 => normalize_opaque(trimmed),
        }
    }
}

impl fmt::Display for NormalizationRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

fn has_control_or_space(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\u{a0}')
}

/// Whether a mail local part is an unquoted dot-atom with no empty label.
fn is_dot_atom(local: &str) -> bool {
    if local.is_empty() || local.starts_with('.') || local.ends_with('.') {
        return false;
    }
    local.split('.').all(|atom| {
        !atom.is_empty()
            && atom.chars().all(|c| {
                c.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~-".contains(c) || !c.is_ascii()
            })
    })
}

/// Whether a domain is dot-separated with non-empty labels and at least one dot.
fn is_dotted_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || !c.is_ascii())
        })
}

fn normalize_email(value: &str) -> Result<String, RefusalReason> {
    let value = match (value.strip_prefix('<'), value.ends_with('>')) {
        (Some(inner), true) => inner.trim_end_matches('>'),
        _ => value,
    };
    if has_control_or_space(value) {
        return Err(RefusalReason::MalformedEmail);
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(RefusalReason::MalformedEmail);
    };
    if !is_dot_atom(local) {
        return Err(RefusalReason::MalformedEmail);
    }
    let domain = domain.strip_suffix('.').unwrap_or(domain);
    if !is_dotted_domain(domain) {
        return Err(RefusalReason::MalformedEmail);
    }
    Ok(format!(
        "{}@{}",
        local.to_ascii_lowercase(),
        domain.to_ascii_lowercase()
    ))
}

fn normalize_phone(value: &str) -> Result<String, RefusalReason> {
    let mut digits = String::new();
    let mut leading_plus = false;
    for (index, c) in value.chars().enumerate() {
        match c {
            '+' if index == 0 => leading_plus = true,
            ' ' | '\t' | '-' | '.' | '(' | ')' | '\u{a0}' | '/' => {}
            '0'..='9' => digits.push(c),
            _ => return Err(RefusalReason::MalformedPhone),
        }
    }
    if !leading_plus {
        // A national-format number is ambiguous between countries. Guessing a
        // country from a locale or from another Note would be inference.
        return Err(RefusalReason::InsufficientCountryContext);
    }
    if !(7..=15).contains(&digits.len()) {
        return Err(RefusalReason::MalformedPhone);
    }
    Ok(format!("+{digits}"))
}

fn normalize_domain(value: &str) -> Result<String, RefusalReason> {
    if has_control_or_space(value) {
        return Err(RefusalReason::MalformedDomain);
    }
    let value = value.strip_suffix('.').unwrap_or(value);
    if !is_dotted_domain(value) {
        return Err(RefusalReason::MalformedDomain);
    }
    Ok(value.to_ascii_lowercase())
}

fn normalize_digest(value: &str) -> Result<String, RefusalReason> {
    let valid = value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if valid {
        Ok(value.to_owned())
    } else {
        Err(RefusalReason::MalformedDigest)
    }
}

fn normalize_opaque(value: &str) -> Result<String, RefusalReason> {
    if has_control_or_space(value) {
        return Err(RefusalReason::MalformedToken);
    }
    Ok(value.to_owned())
}

/// Why a candidate identity value stays unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RefusalReason {
    /// The value carries no `namespace:` prefix, and a namespace is mandatory.
    MissingNamespace,
    /// The namespace does not match `[a-z][a-z0-9-]{0,62}`.
    MalformedNamespace,
    /// No declared policy covers the namespace, so its matching scope is
    /// unknown and it cannot be treated as exact.
    UnknownNamespace,
    /// The value is empty after trimming.
    EmptyValue,
    /// The value exceeds [`MAX_VALUE_BYTES`].
    ValueTooLong,
    /// The value is not a single unquoted mail address.
    MalformedEmail,
    /// The value is not a dialable digit sequence.
    MalformedPhone,
    /// The phone number has no `+` country context, so it is ambiguous.
    InsufficientCountryContext,
    /// The value is not a dotted domain name.
    MalformedDomain,
    /// The value is not 64 lowercase hexadecimal digits.
    MalformedDigest,
    /// An opaque token contains whitespace or control characters.
    MalformedToken,
    /// A display name, label, or other unverified description. Candidates only.
    WeakDescriptiveValue,
    /// An authority- or namespace-scoped anchor whose scope is not known, so
    /// matching it would risk a cross-tenant collision.
    ScopeRequired,
}

impl RefusalReason {
    /// The stable lowercase label used in gap reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RefusalReason::MissingNamespace => "missing-namespace",
            RefusalReason::MalformedNamespace => "malformed-namespace",
            RefusalReason::UnknownNamespace => "unknown-namespace",
            RefusalReason::EmptyValue => "empty-value",
            RefusalReason::ValueTooLong => "value-too-long",
            RefusalReason::MalformedEmail => "malformed-email",
            RefusalReason::MalformedPhone => "malformed-phone",
            RefusalReason::InsufficientCountryContext => "insufficient-country-context",
            RefusalReason::MalformedDomain => "malformed-domain",
            RefusalReason::MalformedDigest => "malformed-digest",
            RefusalReason::MalformedToken => "malformed-token",
            RefusalReason::WeakDescriptiveValue => "weak-descriptive-value",
            RefusalReason::ScopeRequired => "scope-required",
        }
    }
}

impl fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            RefusalReason::MissingNamespace => {
                "the value carries no identity namespace, and Fieldnotes never matches an \
                 unqualified value"
            }
            RefusalReason::MalformedNamespace => "the identity namespace is not a lowercase token",
            RefusalReason::UnknownNamespace => {
                "no declared policy covers this identity namespace, so its matching scope is \
                 unknown"
            }
            RefusalReason::EmptyValue => "the identity value is empty",
            RefusalReason::ValueTooLong => "the identity value exceeds the accepted length",
            RefusalReason::MalformedEmail => "the value is not a single unquoted mail address",
            RefusalReason::MalformedPhone => "the value is not a dialable digit sequence",
            RefusalReason::InsufficientCountryContext => {
                "the phone number has no explicit country context, so it is ambiguous"
            }
            RefusalReason::MalformedDomain => "the value is not a dotted domain name",
            RefusalReason::MalformedDigest => "the value is not a 64-digit lowercase hex digest",
            RefusalReason::MalformedToken => {
                "the opaque token contains whitespace or control characters"
            }
            RefusalReason::WeakDescriptiveValue => {
                "the value is an unverified description, which can only produce a candidate"
            }
            RefusalReason::ScopeRequired => {
                "the anchor is exact only inside a scope this Note does not declare"
            }
        };
        f.write_str(text)
    }
}

impl std::error::Error for RefusalReason {}

/// A refused value together with the rule that refused it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Refusal {
    /// Why the value stays unresolved.
    pub reason: RefusalReason,
    /// The exact input value, retained so a gap report can quote it.
    pub raw: String,
    /// The normalization rule that rejected the value, when one applied.
    pub rule: Option<NormalizationRule>,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.rule {
            Some(rule) => write!(f, "{} ({}: {})", self.raw, rule.name(), self.reason),
            None => write!(f, "{} ({})", self.raw, self.reason),
        }
    }
}

impl std::error::Error for Refusal {}

/// The declared policy for one identity namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacePolicy {
    /// The namespace token, without the `:` separator.
    pub namespace: String,
    /// The scope the namespace's values are exact within.
    pub scope_class: ScopeClass,
    /// How strongly two equal normalized values may be treated as one thing.
    pub strength: Strength,
    /// The versioned rule applied to values in this namespace.
    pub normalization: NormalizationRule,
    /// The kind of entity an anchor in this namespace projects.
    pub entity_kind: EntityKind,
}

/// Whether `text` matches the A2 identity-namespace grammar
/// `[a-z][a-z0-9-]{0,62}`.
#[must_use]
pub fn is_valid_namespace(text: &str) -> bool {
    let mut bytes = text.bytes();
    if !text.is_ascii() || text.is_empty() || text.len() > 63 {
        return false;
    }
    match bytes.next() {
        Some(b'a'..=b'z') => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The declared identity namespaces the resolver knows.
///
/// v0.1 declares only namespaces whose scope and normalization A1's frozen
/// corpus demonstrates: `email`, `phone`, `domain`, and the content-addressed
/// `artifact-sha256`. A connector that emits an authority-scoped anchor
/// namespace supplies its policy through [`NamespaceRegistry::with_policies`],
/// because scope is connector metadata or durable configuration, never a
/// convention hidden in graph code. An undeclared namespace is refused, so a
/// value whose matching scope is unknown never becomes an exact match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceRegistry {
    policies: BTreeMap<String, NamespacePolicy>,
}

/// The namespace of the content-addressed artifact anchor the graph derives
/// from a Note's `artifacts` list.
pub const ARTIFACT_NAMESPACE: &str = "artifact-sha256";

impl NamespaceRegistry {
    /// The v0.1 declared namespaces.
    #[must_use]
    pub fn v1() -> Self {
        let declared = [
            NamespacePolicy {
                namespace: "email".to_owned(),
                scope_class: ScopeClass::NormalizedChannel,
                strength: Strength::Strong,
                normalization: NormalizationRule::EmailV1,
                entity_kind: EntityKind::Person,
            },
            NamespacePolicy {
                namespace: "phone".to_owned(),
                scope_class: ScopeClass::NormalizedChannel,
                strength: Strength::Strong,
                normalization: NormalizationRule::PhoneE164V1,
                entity_kind: EntityKind::Person,
            },
            NamespacePolicy {
                namespace: "domain".to_owned(),
                scope_class: ScopeClass::SourceGlobal,
                strength: Strength::Exact,
                normalization: NormalizationRule::DomainV1,
                entity_kind: EntityKind::Organization,
            },
            NamespacePolicy {
                namespace: ARTIFACT_NAMESPACE.to_owned(),
                scope_class: ScopeClass::SourceGlobal,
                strength: Strength::Exact,
                normalization: NormalizationRule::Sha256DigestV1,
                entity_kind: EntityKind::Artifact,
            },
        ];
        NamespaceRegistry {
            policies: declared
                .into_iter()
                .map(|policy| (policy.namespace.clone(), policy))
                .collect(),
        }
    }

    /// Adds connector- or configuration-declared namespaces to the v0.1 set.
    ///
    /// A later-declared policy replaces an earlier one with the same namespace,
    /// so the caller's declaration order is the only thing that decides the
    /// outcome; iteration order never is.
    pub fn with_policies<I>(policies: I) -> Result<Self, RefusalReason>
    where
        I: IntoIterator<Item = NamespacePolicy>,
    {
        let mut registry = NamespaceRegistry::v1();
        for policy in policies {
            if !is_valid_namespace(&policy.namespace) {
                return Err(RefusalReason::MalformedNamespace);
            }
            registry.policies.insert(policy.namespace.clone(), policy);
        }
        Ok(registry)
    }

    /// The declared policy for one namespace.
    #[must_use]
    pub fn policy(&self, namespace: &str) -> Option<&NamespacePolicy> {
        self.policies.get(namespace)
    }

    /// The declared namespaces in ascending order.
    pub fn namespaces(&self) -> impl Iterator<Item = &str> {
        self.policies.keys().map(String::as_str)
    }
}

impl Default for NamespaceRegistry {
    fn default() -> Self {
        NamespaceRegistry::v1()
    }
}

/// A normalized identity anchor: mandatory namespace, the scope it is exact
/// within when its namespace needs one, and the normalized value.
///
/// Ordering is by namespace, then scope, then value, which gives every derived
/// output a total order that does not depend on input order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdentityKey {
    namespace: String,
    scope: Option<String>,
    value: String,
}

impl IdentityKey {
    /// Assembles a key from already-normalized parts.
    #[must_use]
    pub fn new(
        namespace: impl Into<String>,
        scope: Option<String>,
        value: impl Into<String>,
    ) -> Self {
        IdentityKey {
            namespace: namespace.into(),
            scope,
            value: value.into(),
        }
    }

    /// The identity namespace.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The declared scope the value is exact within, when its namespace needs
    /// one. Two keys with the same namespace and value but different scopes are
    /// different identities and are never joined.
    #[must_use]
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    /// The normalized value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether this key may be written into a public `identities` list.
    ///
    /// A1 froze the flat `namespace:value` anchor form. It did not freeze a
    /// public spelling that also carries an authority scope, so a scoped anchor
    /// stays in explanations and is never emitted in a way a reader could
    /// mistake for a globally exact value.
    #[must_use]
    pub fn is_publishable(&self) -> bool {
        self.scope.is_none()
    }

    /// The flat `namespace:value` anchor form used in public records.
    #[must_use]
    pub fn anchor_text(&self) -> String {
        format!("{}:{}", self.namespace, self.value)
    }
}

impl fmt::Display for IdentityKey {
    /// `namespace:value`, with ` in scope <scope>` appended for a scoped anchor
    /// so an explanation can never be read as claiming global exactness.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.value)?;
        match &self.scope {
            Some(scope) => write!(f, " in scope {scope}"),
            None => Ok(()),
        }
    }
}

/// The rule name used when a bare channel value from a role property such as
/// `from`, `to`, or `participants` is normalized into an anchor.
pub const PARTICIPANT_RULE: &str = "participant-channel-v1";

/// Parses one flat `namespace:value` anchor from a Note's `identities` list.
///
/// `scope` is the Note's `source_scope`, used only when the namespace's declared
/// class requires one. An anchor whose namespace needs a scope the Note does not
/// supply is refused rather than matched loosely.
pub fn parse_anchor(
    raw: &str,
    registry: &NamespaceRegistry,
    scope: Option<&str>,
) -> Result<IdentityKey, Refusal> {
    let refusal = |reason: RefusalReason, rule: Option<NormalizationRule>| Refusal {
        reason,
        raw: raw.to_owned(),
        rule,
    };
    let Some((namespace, value)) = raw.split_once(':') else {
        return Err(refusal(RefusalReason::MissingNamespace, None));
    };
    if !is_valid_namespace(namespace) {
        return Err(refusal(RefusalReason::MalformedNamespace, None));
    }
    let Some(policy) = registry.policy(namespace) else {
        return Err(refusal(RefusalReason::UnknownNamespace, None));
    };
    let normalized = policy
        .normalization
        .apply(value)
        .map_err(|reason| refusal(reason, Some(policy.normalization)))?;
    let scope = if policy.scope_class.requires_scope() {
        match scope {
            Some(scope) if !scope.is_empty() => Some(scope.to_owned()),
            _ => return Err(refusal(RefusalReason::ScopeRequired, None)),
        }
    } else {
        None
    };
    Ok(IdentityKey::new(namespace, scope, normalized))
}

/// Normalizes a bare channel value from a role property into an anchor.
///
/// `from`, `to`, `cc`, `bcc`, `organizer`, and `participants` carry values in
/// the source's own spelling with no namespace. A value that normalizes as a
/// mail address becomes an `email:` anchor and a value that normalizes as an
/// international phone number becomes a `phone:` anchor; anything else — a
/// display name, an opaque source handle, a room label — is a weak descriptive
/// value and is refused, because guessing its namespace would be exactly the
/// unqualified match the contract forbids.
pub fn normalize_channel_value(
    raw: &str,
    registry: &NamespaceRegistry,
) -> Result<IdentityKey, Refusal> {
    for namespace in ["email", "phone"] {
        let Some(policy) = registry.policy(namespace) else {
            continue;
        };
        let plausible = match policy.normalization {
            NormalizationRule::EmailV1 => raw.contains('@'),
            NormalizationRule::PhoneE164V1 => raw.trim_start().starts_with('+'),
            _ => false,
        };
        if !plausible {
            continue;
        }
        return match policy.normalization.apply(raw) {
            Ok(value) => Ok(IdentityKey::new(namespace, None, value)),
            Err(reason) => Err(Refusal {
                reason,
                raw: raw.to_owned(),
                rule: Some(policy.normalization),
            }),
        };
    }
    if looks_like_national_number(raw) {
        // A digits-and-punctuation value is plainly meant as a phone number, so
        // the informative refusal is the missing country context rather than a
        // generic "not an identity".
        return Err(Refusal {
            reason: RefusalReason::InsufficientCountryContext,
            raw: raw.to_owned(),
            rule: Some(NormalizationRule::PhoneE164V1),
        });
    }
    Err(Refusal {
        reason: RefusalReason::WeakDescriptiveValue,
        raw: raw.to_owned(),
        rule: None,
    })
}

/// Whether a value is digits and dialing punctuation with no country context.
fn looks_like_national_number(raw: &str) -> bool {
    let digits = raw.chars().filter(char::is_ascii_digit).count();
    digits >= 7
        && raw
            .chars()
            .all(|c| c.is_ascii_digit() || " \t-.()/\u{a0}".contains(c))
}

/// The comparison key for a display name.
///
/// Trims, collapses internal ASCII whitespace runs to one space, and
/// ASCII-lowercases. Unicode case folding and accent folding are deliberately
/// not applied: this key exists only to *generate a candidate*, never to merge,
/// so a wider fold would only manufacture more candidates. Returns `None` for a
/// value with no visible characters.
#[must_use]
pub fn normalized_display_name(raw: &str) -> Option<String> {
    let collapsed: String = raw
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .to_ascii_lowercase();
    (!collapsed.is_empty()).then_some(collapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_normalization_lowercases_but_never_strips_tags_or_dots() {
        let registry = NamespaceRegistry::v1();
        let key = parse_anchor("email:Alice.Mueller+News@Example.COM.", &registry, None);
        assert_eq!(
            key.map(|key| key.anchor_text()),
            Ok("email:alice.mueller+news@example.com".to_owned())
        );
    }

    #[test]
    fn email_normalization_refuses_shapes_it_cannot_prove() {
        let registry = NamespaceRegistry::v1();
        for value in [
            "email:alice",
            "email:alice@@example.com",
            "email:alice@example",
            "email:.alice@example.com",
            "email:alice example@example.com",
            "email:email:alice@example.com",
        ] {
            let refused = parse_anchor(value, &registry, None);
            assert!(
                matches!(
                    refused,
                    Err(Refusal {
                        reason: RefusalReason::MalformedEmail,
                        ..
                    })
                ),
                "{value} must be refused, got {refused:?}"
            );
        }
    }

    #[test]
    fn phone_normalization_requires_explicit_country_context() {
        let registry = NamespaceRegistry::v1();
        assert_eq!(
            parse_anchor("phone:+41 44 123 45 67", &registry, None).map(|key| key.anchor_text()),
            Ok("phone:+41441234567".to_owned())
        );
        let national = parse_anchor("phone:044 123 45 67", &registry, None);
        assert!(matches!(
            national,
            Err(Refusal {
                reason: RefusalReason::InsufficientCountryContext,
                ..
            })
        ));
    }

    #[test]
    fn an_unqualified_or_undeclared_value_is_never_matched() {
        let registry = NamespaceRegistry::v1();
        assert!(matches!(
            parse_anchor("alice@example.com", &registry, None),
            Err(Refusal {
                reason: RefusalReason::MissingNamespace,
                ..
            })
        ));
        assert!(matches!(
            parse_anchor("github-login:alice", &registry, None),
            Err(Refusal {
                reason: RefusalReason::UnknownNamespace,
                ..
            })
        ));
    }

    #[test]
    fn an_authority_scoped_namespace_needs_its_scope() -> Result<(), RefusalReason> {
        let registry = NamespaceRegistry::with_policies([NamespacePolicy {
            namespace: "graph-user-id".to_owned(),
            scope_class: ScopeClass::AuthorityScoped,
            strength: Strength::Exact,
            normalization: NormalizationRule::OpaqueTokenV1,
            entity_kind: EntityKind::Person,
        }])?;
        assert!(matches!(
            parse_anchor("graph-user-id:AB12", &registry, None),
            Err(Refusal {
                reason: RefusalReason::ScopeRequired,
                ..
            })
        ));
        let one = parse_anchor("graph-user-id:AB12", &registry, Some("tenant/a"))
            .map_err(|refusal| refusal.reason)?;
        let two = parse_anchor("graph-user-id:AB12", &registry, Some("tenant/b"))
            .map_err(|refusal| refusal.reason)?;
        // Same unqualified value, different authority: never the same identity,
        // and never publishable as a flat globally exact anchor.
        assert_ne!(one, two);
        assert!(!one.is_publishable());
        // An opaque source ID keeps its exact case.
        assert_eq!(one.value(), "AB12");
        Ok(())
    }

    #[test]
    fn bare_channel_values_normalize_only_when_provable() {
        let registry = NamespaceRegistry::v1();
        assert_eq!(
            normalize_channel_value("Bob@Example.NET", &registry).map(|key| key.anchor_text()),
            Ok("email:bob@example.net".to_owned())
        );
        assert!(matches!(
            normalize_channel_value("Alice Müller", &registry),
            Err(Refusal {
                reason: RefusalReason::WeakDescriptiveValue,
                ..
            })
        ));
        assert!(matches!(
            normalize_channel_value("Conference Room 4", &registry),
            Err(Refusal {
                reason: RefusalReason::WeakDescriptiveValue,
                ..
            })
        ));
    }

    #[test]
    fn display_name_keys_collapse_whitespace_without_folding_accents() {
        assert_eq!(
            normalized_display_name("  Alice   Müller\t"),
            Some("alice müller".to_owned())
        );
        assert_eq!(normalized_display_name("   "), None);
        // Accent folding is deliberately absent: these stay distinct names.
        assert_ne!(
            normalized_display_name("Alice Müller"),
            normalized_display_name("Alice Mueller")
        );
    }

    #[test]
    fn declared_policies_expose_scope_and_strength_for_explanations() {
        let registry = NamespaceRegistry::v1();
        let policy = registry.policy("email");
        assert_eq!(
            policy.map(|policy| (policy.scope_class, policy.strength)),
            Some((ScopeClass::NormalizedChannel, Strength::Strong))
        );
        assert!(
            registry
                .policy("email")
                .is_some_and(|p| p.strength.permits_auto_merge())
        );
        assert_eq!(
            registry.namespaces().collect::<Vec<&str>>(),
            ["artifact-sha256", "domain", "email", "phone"]
        );
    }
}
