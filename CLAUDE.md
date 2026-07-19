# Bolted — project memory

Bolted is a compile-time-verified application framework around BoltFFI (boltffi.dev): one Rust
core, native shells (SwiftUI / Compose / WinUI / Linux), plus a Rust-web target (Leptos/Dioxus,
browser only, zero FFI). Phases 1–3 are **done** (validation spike → design freeze → framework
extraction; ARCHITECTURE frozen, currently v1.13); Phase 4 — the verification harness — is in
progress. The C# leg's upstream boltffi fix is merged on main (unreleased; git-pin decided);
status lives in `upstream/boltffi/README.md`.

## Read order (do this before any work)

1. `docs/VISION.md` — scope, principles, the verification ladder, non-goals.
2. `docs/ARCHITECTURE.md` — the design: facets, observe/command/draft triad, store-owned core,
   `Field` validity×sync, live rebase, three-tier validation, invariants (§7), OPEN questions (§9).
3. `docs/ROADMAP.md` — phases, step table with status, working agreement.
4. `docs/steps/step-XX-*.md` — the current step (the one marked **ready**).

Keep `docs/GLOSSARY.md` at hand throughout: it is the project's **ubiquitous language**
(domain-driven-design sense), deliberately small and curated. Terms are admitted only by
Henrik's explicit decision — **propose a term and ask; never add to or extend the glossary
unilaterally**. Definitions there are self-contained (no doc/section references).

## Memory setup

Claude's auto-memory for this project is redirected into the repo at `.claude/memory/` and
**committed** (index: `.claude/memory/MEMORY.md`). The redirect is the `autoMemoryDirectory`
key in `.claude/settings.local.json` (gitignored — absolute path, machine-specific); on a new
machine, recreate it pointing at `<repo>/.claude/memory`. Durable cross-session learnings go
there; project *instructions* stay in this file.

## How work is organized

- **Fable plans AND drives execution** (since 2026-07-19; before that, implementation ran in
  separate fresh Opus sessions): architecture, step authoring, design freeze, resolving OPEN
  questions, updating VISION/ARCHITECTURE/ROADMAP — and then executing each step by
  delegating implementation to **Opus sub-agents** (Agent tool, `model: "opus"`) while Fable
  orchestrates, reviews their output, and writes the report.
- **Sub-agents implement exactly one step (or one coherent slice of it)**, as specified by
  its step doc. Scope is the step doc — nothing more. The step doc is still written to be
  self-sufficient: a sub-agent has no access to the planning conversation.
- **The interface is still files**: every step ends with
  `docs/steps/step-XX-report.md` (built / deviations / friction log / open questions) and a
  ROADMAP status update. Reports are how findings flow back to planning — write them well.

## Rules for implementation work (sub-agents inherit these; Fable enforces them)

- If the step doc omits a decision: smallest reversible choice, record it in the report. If
  the decision is structural (traits, invariants, ARCHITECTURE.md): stop, record the question
  in the report, leave it for a design session. Never resolve ARCHITECTURE §9 OPEN questions
  ad hoc.
- Kill criteria in step docs are real: if hit, stop and report — don't work around.
- Build/test only via `mise run check` / `mise run test`.
- Rust: edition 2024; clippy `-D warnings`; no `unwrap`/`expect`/`panic!` in library code.
- `bolted-core` never depends on boltffi and stays sans-io (no async runtime).
- Shell/UI code must contain no constraint literals (no magic `30`s — they come from the core).

## Key design decisions (rationale in ARCHITECTURE.md §8)

Lean contract, UI orchestrates validation timing; drafts are core-side handles with live
rebase and field-level conflicts (keep-mine/take-theirs ceiling, no text merge); dirty is
value-based; errors are key+params data, never strings; generics carry behavior, macros only
stamp names; submit re-validates everything, always.
