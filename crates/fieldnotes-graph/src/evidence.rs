//! Evidence origins, rule identifiers, and the explanation record every
//! derived entity and relationship carries.
//!
//! A derivation that cannot say which Notes and which normalized identities
//! produced it is a defect, so an [`Explanation`] is built at the same time as
//! the projection it explains rather than reconstructed afterwards. It names the
//! claim, its origin class, the rule and generator version, the identity
//! namespaces and their declared scope, the cited Note IDs, the counts and time
//! range needed to reproduce the conclusion, and any competing evidence.

use core::fmt;

use fieldnotes_domain::{Datetime, RecordId};

use crate::identity::{IdentityKey, NormalizationRule, ScopeClass, Strength};

/// The deterministic entity-resolver generator contract this crate implements.
pub const ENTITY_GENERATOR: &str = "fieldnotes-entity-resolver-v1";

/// The deterministic relationship-builder generator contract this crate
/// implements.
pub const RELATIONSHIP_GENERATOR: &str = "fieldnotes-relationship-builder-v1";

/// Where a projected claim came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// The source directly supplied the relationship or fact.
    Explicit,
    /// An approved deterministic rule established it.
    Matched,
    /// An optional Extraction recovered literal evidence from one Note.
    ///
    /// No deterministic rule in this crate produces this origin; Extractions
    /// arrive with the optional enhancement gate.
    Extracted,
    /// An optional Observation synthesized cited evidence.
    ///
    /// As with [`Origin::Extracted`], nothing here produces it.
    Observed,
}

impl Origin {
    /// The stable lowercase label used in records and explanations.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Explicit => "explicit",
            Origin::Matched => "matched",
            Origin::Extracted => "extracted",
            Origin::Observed => "observed",
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A versioned generator and the rule inside it that reached a conclusion.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleId {
    /// The generator contract, such as `fieldnotes-entity-resolver-v1`.
    pub generator: String,
    /// The rule inside that generator, such as `email-exact-v1`.
    pub rule: String,
}

impl RuleId {
    /// Names a rule inside the entity resolver.
    #[must_use]
    pub fn entity(rule: impl Into<String>) -> Self {
        RuleId {
            generator: ENTITY_GENERATOR.to_owned(),
            rule: rule.into(),
        }
    }

    /// Names a rule inside the relationship builder.
    #[must_use]
    pub fn relationship(rule: impl Into<String>) -> Self {
        RuleId {
            generator: RELATIONSHIP_GENERATOR.to_owned(),
            rule: rule.into(),
        }
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.generator, self.rule)
    }
}

/// The rule name for an entity that rests on one directly supplied anchor in
/// `namespace`, with no join.
#[must_use]
pub fn anchor_rule(namespace: &str) -> String {
    format!("{namespace}-anchor-v1")
}

/// The rule name for an entity whose anchor in `namespace` recurred across
/// several current Notes.
#[must_use]
pub fn exact_rule(namespace: &str) -> String {
    format!("{namespace}-exact-v1")
}

/// The rule that treats the person-class anchors printed on one source contact
/// record as anchors of one person, because the source itself states that.
pub const CONTACT_RECORD_RULE: &str = "contact-record-anchors-v1";

/// The rule that records two entities appearing in the same current source
/// object.
pub const CO_PARTICIPANT_RULE: &str = "co-participant-v1";

/// The rule that reuses a prior projection ID when exactly one prior entity
/// record shares an anchor with a newly derived entity.
pub const ID_REUSE_RULE: &str = "entity-id-reuse-v1";

/// One normalized anchor an entity rests on, with everything needed to
/// reproduce why it matched.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolvedIdentity {
    /// The normalized, namespaced key.
    pub key: IdentityKey,
    /// The declared matching scope class of its namespace.
    pub scope_class: ScopeClass,
    /// The declared strength of its namespace.
    pub strength: Strength,
    /// The versioned normalization rule applied to the raw value.
    pub normalization: NormalizationRule,
    /// The Notes that supplied this anchor, in ascending ID order.
    pub evidence: Vec<RecordId>,
}

/// One deterministic join between two different anchors of the same entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IdentityJoin {
    /// The rule that established the join.
    pub rule: RuleId,
    /// The lower-ordered key.
    pub left: IdentityKey,
    /// The higher-ordered key.
    pub right: IdentityKey,
    /// The Note whose content stated both anchors.
    pub evidence: RecordId,
}

/// Evidence that competes with a projected claim, retained instead of resolved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CompetingEvidence {
    /// What the competing evidence claims.
    pub claim: String,
    /// The Notes that supplied it, in ascending ID order.
    pub evidence: Vec<RecordId>,
}

/// The complete explanation of one derived entity or relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// The projection this explains.
    pub subject: RecordId,
    /// The claim in one line.
    pub claim: String,
    /// The origin class of the claim.
    pub origin: Origin,
    /// The generator and rule that reached it.
    pub rule: RuleId,
    /// The normalized anchors involved, in ascending key order.
    pub identities: Vec<ResolvedIdentity>,
    /// Every deterministic join applied, in ascending order.
    pub joins: Vec<IdentityJoin>,
    /// The cited Notes, in ascending ID order.
    pub evidence: Vec<RecordId>,
    /// The number of distinct current Notes supporting the claim.
    ///
    /// This is the full count even when [`Explanation::evidence`] is a bounded
    /// representative list.
    pub evidence_count: usize,
    /// The earliest supporting event instant.
    pub first_seen: Option<Datetime>,
    /// The latest supporting event instant.
    pub last_seen: Option<Datetime>,
    /// Contradictions and ambiguity that were preserved, not resolved.
    pub competing: Vec<CompetingEvidence>,
}

impl fmt::Display for Explanation {
    /// A deterministic plain-text rendering, line-ordered exactly as the
    /// explainability contract lists its required parts.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "subject: {}", self.subject)?;
        writeln!(f, "claim: {}", self.claim)?;
        writeln!(f, "origin: {}", self.origin)?;
        writeln!(f, "rule: {}", self.rule)?;
        for identity in &self.identities {
            writeln!(
                f,
                "identity: {} [{} / {} / {}]",
                identity.key, identity.scope_class, identity.strength, identity.normalization
            )?;
            for note in &identity.evidence {
                writeln!(f, "  from: {note}")?;
            }
        }
        for join in &self.joins {
            writeln!(
                f,
                "join: {} + {} by {} from {}",
                join.left, join.right, join.rule, join.evidence
            )?;
        }
        for note in &self.evidence {
            writeln!(f, "evidence: {note}")?;
        }
        writeln!(f, "evidence_count: {}", self.evidence_count)?;
        if let Some(first) = &self.first_seen {
            writeln!(f, "first_seen: {first}")?;
        }
        if let Some(last) = &self.last_seen {
            writeln!(f, "last_seen: {last}")?;
        }
        for competing in &self.competing {
            writeln!(f, "competing: {}", competing.claim)?;
            for note in &competing.evidence {
                writeln!(f, "  from: {note}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldnotes_domain::IdError;

    #[test]
    fn rule_ids_render_generator_and_rule() {
        assert_eq!(
            RuleId::entity(exact_rule("email")).to_string(),
            "fieldnotes-entity-resolver-v1/email-exact-v1"
        );
        assert_eq!(
            RuleId::relationship(CO_PARTICIPANT_RULE).to_string(),
            "fieldnotes-relationship-builder-v1/co-participant-v1"
        );
        assert_eq!(anchor_rule("phone"), "phone-anchor-v1");
    }

    #[test]
    fn an_explanation_renders_every_required_part() -> Result<(), IdError> {
        let subject = RecordId::parse("ent_01a028f2-dcc0-7000-8000-000000000001")?;
        let note = RecordId::parse("note_01a0287d-acc0-7000-8000-000000000005")?;
        let key = IdentityKey::new("email", None, "alice@example.com");
        let explanation = Explanation {
            subject,
            claim: "one person".to_owned(),
            origin: Origin::Matched,
            rule: RuleId::entity(exact_rule("email")),
            identities: vec![ResolvedIdentity {
                key,
                scope_class: ScopeClass::NormalizedChannel,
                strength: Strength::Strong,
                normalization: NormalizationRule::EmailV1,
                evidence: vec![note],
            }],
            joins: Vec::new(),
            evidence: vec![note],
            evidence_count: 1,
            first_seen: Some(
                Datetime::parse("2026-08-22T10:00:00+02:00").map_err(|_| IdError::MalformedUuid)?,
            ),
            last_seen: None,
            competing: vec![CompetingEvidence {
                claim: "a second contact record spells the name differently".to_owned(),
                evidence: vec![note],
            }],
        };
        let rendered = explanation.to_string();
        for expected in [
            "subject: ent_01a028f2-dcc0-7000-8000-000000000001",
            "origin: matched",
            "rule: fieldnotes-entity-resolver-v1/email-exact-v1",
            "identity: email:alice@example.com [normalized-channel / strong / email-normalize-v1]",
            "evidence: note_01a0287d-acc0-7000-8000-000000000005",
            "evidence_count: 1",
            "first_seen: 2026-08-22T10:00:00+02:00",
            "competing: a second contact record spells the name differently",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected} in\n{rendered}"
            );
        }
        Ok(())
    }
}
