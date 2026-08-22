# Proposed v1 invalid notebook fixtures

**Status:** Approved negative corpus at A1 on 2026-08-22; frozen as the IG1
implementation target

Every Note fixture listed below is intentionally invalid and should be rejected
by the proposed v1 validator. Each fixture isolates one primary error; the
conceptual error names below are review labels, not an implemented error-code
API.

Except for the filename-mismatch and timezone-less fixtures, filenames are the
canonical UTC names computed from their frontmatter. IDs, required properties,
offsets, key order, and bodies are otherwise kept minimal so a rejection does
not accidentally depend on an unrelated rule.

| Fixture | Required rejection | Conceptual error |
|---|---|---|
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab01.md` | `title` contains a nested mapping | `frontmatter.nested_mapping` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab02.md` | `participants` is an array of objects | `frontmatter.array_object` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab03.md` | `participants` mixes text and number values | `frontmatter.mixed_list` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab04.md` | `title` is `null` | `frontmatter.null` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab05.md` | `title` appears twice | `frontmatter.duplicate_key` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab06.md` | `title` uses a custom YAML tag | `frontmatter.custom_tag` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab07.md` | `occurred_at` has no UTC offset | `datetime.offset_required` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab08.md` | list-typed `participants` is emitted as a scalar | `property.list_required` |
| `20260822T093614Z_teams_work_message_note_01a028d5-90c0-7248-a74b-c8bc1085ab09.md` | Teams Note uses unknown unprefixed `chat_id` | `property.unknown_unprefixed` |
| `20260822T093614Z_local_work_file_note_01a028d5-90c0-7248-a74b-c8bc1085ab0b.md` | external Note has `source_identity` but no `source_scope` | `source.scope_required` |
| `20260822T093615Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0c.md` | filename UTC second disagrees with valid frontmatter | `filename.mismatch` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0d.md` | frontmatter contains an explicit YAML document-end marker | `frontmatter.document_marker` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0e.md` | `self` Note carries a foreign `teams_` connector-prefixed property | `property.foreign_prefix` |

Tests may report a more specific parse location in addition to the conceptual
error. They must not accept, normalize, or silently discard the invalid value.

## Withdrawn fixtures

`20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0a.md`
(`security.secret_detected`, a registered `source_url` containing an
access-token canary) was withdrawn by approved amendment (ruling 3, 2026-08-22):
Fieldnotes does not scan or reject notebook content for secrets, passwords,
tokens, or credentials. A credential appearing in collected evidence was put
there by a person or upstream system; rejecting it would discard evidence and
be unfixable by the user. The real invariant — Fieldnotes never writes
credentials it holds into notebooks — is enforced elsewhere (the
CredentialProvider boundary, protected secret delivery, log redaction, and
release-gate scanning of Fieldnotes' own output), not by content validation.
