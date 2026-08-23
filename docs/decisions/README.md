# Architecture decision records

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-current-state-and-state-classes.md) | Current-state notebook and state classes | Accepted for v0.1 documentation |
| [0002](0002-source-identity-updates-and-merge.md) | Portable source identity, updates, and merge | Accepted direction; merge metadata name reviewable |
| [0003](0003-datetime-serialization.md) | Offset-bearing datetimes and UTC filenames | Proposed for user approval |
| [0004](0004-record-ids-and-hash-domains.md) | UUIDv7 record IDs and domain-separated hashes | Proposed for user approval |
| [0005](0005-field-process-boundary.md) | Minimal trusted Field process boundary | Proposed; exact protocol is a later gate |
| [0006](0006-a1-implementation-rulings.md) | A1 implementation rulings from IG1 findings | Accepted |
| [0007](0007-attachment-retention-policy.md) | Skipped attachments, link semantics, re-collection, and media-type retention | Accepted |
| [0008](0008-extend-canonical-extension-registry.md) | Extend the canonical extension registry for the default retention set | Accepted |
| [0009](0009-field-sdk-extraction.md) | A Field-authoring SDK, extracted from two working Fields | Accepted |
| [0010](0010-property-registry-relocation.md) | Move the property registry from `fieldnotes-format` to `fieldnotes-domain` | Accepted |
| [0011](0011-neutralize-illustrative-personal-identifiers.md) | Neutralize illustrative personal identifiers before first publication | Accepted |
| [0012](0012-graph-implementation-rulings.md) | Graph implementation rulings from IG4 findings | Accepted |

ADRs record decisions with architectural consequences. Proposed ADRs do not become implementation contracts until their roadmap approval gate is complete.

