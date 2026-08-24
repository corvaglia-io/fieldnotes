# Prompt for a new Grok session — A3 plan, then implement

Copy everything below the line into a new Grok session in this repo.

---

You are taking over Fieldnotes after A3 was written. Do **not** re-argue A3. Do **not** implement until I say go.

## Goal

1. Get comfortable in the repo with **minimal tokens**.
2. Produce a **subagent-driven implementation plan** for A3 as settled.
3. Present the plan and **stop**.
4. When I say go: implement on a **new branch** off current `main` (suggested: `a3-signals-notes`). Do not merge. Do not check A3 approval boxes.

A3 is still labeled “not approved” in the docs. Treat the **Settled by the owner** list and **§13 planner’s brief** as the implementation brief. Owner direction: implement on a branch after the plan is accepted.

## Token and speed rules (mandatory)

- Read **once**, in this order, then stop reading A3:
  1. `docs/approvals/A3-signals-and-notes.md` — front matter + **Settled by the owner** (points 1–19) + **§13** (13.1–13.8, including 13.7 spike evidence).
  2. `AGENTS.md`
  3. `docs/approvals/A3-signals-and-notes.md` §13.4 crate table only if you skipped it in (1).
- Do **not** read the rest of A3 (P1–P7 debate, alternatives) unless a specific contradiction blocks a plan item.
- Do **not** dump the whole crate tree into context. Use the crate table in A3 §13.4 as the map. Grep/read only files a workstream will touch.
- Subagent prompts must include the relevant settled points as **copy-paste bullets**, not “see A3.” Cap each child at the files it may write.
- Parallelize independent workstreams; serialize anything that edits `fieldnotes-format` emit/filename, `fieldnotes-store` layout, or shared protocol schemas.
- No live Microsoft Graph in ordinary work. Fixtures only.
- No model downloads in CI. Linux CI is the **fold** path.

## What to implement (in)

Follow A3 §13.5 order, compressed into workstreams:

| WS | What | Notes |
|---|---|---|
| W1 | Domain + format: `origin`, `signals` list, `title_slug`, `sig_` 96-bit ids, title-first note emit | Tests first. Do not change `fn-content-v1` / `fn-record-v1` vectors except path/id prefix on signals. |
| W2 | Store: `.fieldnotes/signals/<field-id>/`, cache index (files always win; lose cache = rescan, never wrong merge), skip unless `--force` | `--force` reruns extraction + Field observations + caption; **does not** rerun slug. |
| W3 | Collect pipeline: deterministic note → masked extraction → observations | First emit only. |
| W4 | Mail scaffold + authored notes + A3 review corpus (mail, authored, skip/`--force`, authored-safe rebuild, cache rebuild) | No contact-note fixtures. |
| W5 | Contacts: stop public notes; vCard 4 subset under `.fieldnotes/fields/<id>/contacts/`; graph reads UID/EMAIL/TEL | |
| W6 | Additive `describe.note_sections` (extraction / observation / signals). Mail declares sections. | A2 record/checkpoint frames unchanged. |
| W7 | macOS Foundation Models adapter: detect availability; JSON in/out; core **slug observation** then **one-line image caption** | Spike already ran on owner Mac — see A3 §13.7. Do not use `/usr/bin/fm` (license). Do not use Private Cloud Compute. Caption eval C1–C7 in §13.6 E. |
| W8 | Apply A3 §12 replacement wording to roadmap invariant 1, ADR 0001 class 1, `product.md`, R0, R8. | Do not silent-rename every “Note” in historical ADRs. |

## What not to implement (out)

- Type-partitioned `notes/`
- Public `signals/` directory
- llama.cpp / GGUF pin (Win/Linux stay on **fold**)
- PII / Presidio (rejected: Python)
- BYO providers
- Eleven templates
- Nested JSON signals / A2 record nesting
- Change tracking / versioned signals
- In-place notebook migrator (re-sync)
- Using the vision model as document OCR (MarkItDown vs Firecrawl AnyDoc is a **later eval**, not this branch’s ship; mail body stays Field-mapped text/HTML)
- Scoring Field observation use cases beyond wiring `describe` prompts (ask/FYI wait)

## Plan shape I want back

A short DAG:

- Workstream id, files it may write, tests it must add, what it waits on
- Which streams can run as **parallel worktree subagents**
- A coordinator-only integration step (workspace manifests, golden fixtures)
- Explicit “first green” milestone: `cargo test` on Linux fold path + Mac adapter compile-gated

Then **stop and wait**.

When I say go: create the branch, execute the DAG, keep commits small and by workstream, match existing crate style (`Agents.md`, no `unwrap` in libraries, injected clocks). If contract prose and fixture bytes disagree, record a finding — do not guess.

## Repo orientation (do not rediscover)

- Rust workspace, Fields are child processes (A2).
- Core is the only durable notebook writer.
- `fieldnotes-format` owns A1 bytes; `fieldnotes-store` owns layout; `fieldnotes-app` owns collect.
- Outlook Mail/Calendar/Contacts already exist; Contacts’ **product role** changes (matching vCard, not notes).
- Owner machine spike: Apple M5 Pro, macOS 27, `SystemLanguageModel` available; slug ~0.4–0.8s; JSON observations; `Attachment(NSImage)` captions ~1.2–2s. Command Line Tools cannot load `FoundationModelsMacros` — use JSON, not `@Generable`, unless Xcode is on the build.
