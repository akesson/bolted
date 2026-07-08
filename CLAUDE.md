# Bolted — project memory

Bolted is a compile-time-verified application framework around BoltFFI (boltffi.dev): one Rust
core, native shells (SwiftUI / Compose / WinUI / Linux), plus a Rust-web target (Leptos/Dioxus,
browser only, zero FFI). Currently **pre-implementation**: design done, validation spike
starting.

## Read order (do this before any work)

1. `VISION.md` — scope, principles, the verification ladder, non-goals.
2. `docs/ARCHITECTURE.md` — the design: observe/command/draft triad, Elm core, `Field`
   validity×sync, live rebase, three-tier validation, invariants (§7), OPEN questions (§9).
3. `docs/ROADMAP.md` — phases, step table with status, working agreement.
4. `docs/steps/step-XX-*.md` — the current step (the one marked **ready**).

## How work is organized

- **Planning sessions (Fable)**: architecture, step authoring, design freeze, resolving OPEN
  questions, updating VISION/ARCHITECTURE/ROADMAP.
- **Implementation sessions (Opus)**: implement exactly one step, as specified by its step
  doc. Scope is the step doc — nothing more.
- **The interface between them is files**: every step ends with
  `docs/steps/step-XX-report.md` (built / deviations / friction log / open questions) and a
  ROADMAP status update. Reports are how findings flow back to planning — write them well.

## Rules for implementation sessions

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
