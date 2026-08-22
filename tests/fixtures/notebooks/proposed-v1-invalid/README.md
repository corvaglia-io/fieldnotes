# Proposed v1 invalid notebook fixtures

**Status:** Candidate negative corpus for A1 approval

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
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0a.md` | registered `source_url` contains an access-token canary | `security.secret_detected` |
| `20260822T093614Z_local_work_file_note_01a028d5-90c0-7248-a74b-c8bc1085ab0b.md` | external Note has `source_identity` but no `source_scope` | `source.scope_required` |
| `20260822T093615Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0c.md` | filename UTC second disagrees with valid frontmatter | `filename.mismatch` |
| `20260822T093614Z_self_text_note_01a028d5-90c0-7248-a74b-c8bc1085ab0d.md` | frontmatter contains an explicit YAML document-end marker | `frontmatter.document_marker` |

Tests may report a more specific parse location in addition to the conceptual
error. They must not accept, normalize, or silently discard the invalid value.
