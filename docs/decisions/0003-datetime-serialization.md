# ADR 0003: Offset-bearing datetimes and UTC filenames

- **Status:** Proposed for user approval
- **Date:** 2026-08-22

## Context

Timezone-less YAML datetimes rely on a Fieldnotes convention that is invisible
to generic tools and can change the represented instant when clients assume a
local timezone. Always displaying UTC, however, discards useful source-local
context.

## Decision

Datetime frontmatter uses RFC 3339 with an explicit numeric UTC offset and
never a timezone-less value.

```yaml
occurred_at: 2026-08-22T11:36:14+02:00
```

Use a reliable source-local offset when supplied. Otherwise use the configured
Field timezone or the collecting client's local offset at that instant. The
represented instant is preserved in every case. UTC is serialized as
`+00:00`.

Note filenames render the same instant in UTC using
`YYYYMMDDTHHMMSSZ`. Date-only properties remain `YYYY-MM-DD` and do not acquire
a timezone.

## Consequences

- A generic RFC 3339 parser can recover the exact instant.
- Humans retain source/client-local wall-clock context in frontmatter.
- Lexicographic note filenames still form a global UTC timeline.
- Correcting an event time may rename the file but never changes the Note ID.
- Obsidian compatibility must be tested against offset-bearing values rather
  than solved by dropping timezone information.

