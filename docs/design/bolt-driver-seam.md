# The bolt-driver seam — what bolted owes an external project

**Status: design input, not a decision.** Records the seam between bolted and
**bolt-driver** (`~/Developer/akesson/bolt-driver`, a sibling repo) so no bolted decision
forecloses it by accident — the same move §9 makes for interaction replay, one repo out.
Nothing here resolves a §9 item; scheduling any obligation below is a design-session call.

## What bolt-driver is

In-process, permission-free remote driving and inspection for native apps: a dev-build-embedded
agent serves the semantics tree, drive verbs (tap / set_text / scroll), and screenshots over a
local socket; the primary client is Claude (CLI today, MCP later). **Proven** by its step 01
(2026-07-23, branch `step-01` @ `a9be22b`): both arms — Compose (semantics owner) and
SwiftUI/iOS (in-process a11y walk + self-enabled `ApplicationAccessibilityEnabled`) — driven
blind end-to-end via the CLI, tree + tap + set_text + screenshots, no a11y service, no TCC
prompt, release builds structurally agent-free. Read there: `docs/VISION.md`,
`docs/design/session-replay.md`, `docs/steps/step-01-report.md`.

## The dependency law (both repos enforce their half)

- bolted **never** depends on bolt-driver — deleting bolt-driver leaves bolted intact.
- bolt-driver's core crates never depend on bolted; exactly one glue crate (`driver-bolted`,
  living in the bolt-driver repo) depends on both. A non-bolted Rust core implementing
  bolt-driver's `DriveTarget` trait is a first-class citizen (their two-implementor rule).

## What bolted owes the seam (unscheduled; each is a future step or design-pass item)

1. **Generated automation ids** — facet-binding codegen stamps stable identifiers
   (`bolted:<facet>.<field>`-shaped) as `testTag` / `accessibilityIdentifier` /
   `AutomationId` / `data-testid`, from the one parsed declaration (D25 road).
2. **The core-track tap** — recording of typed calls entering the core, emitted in the
   generated FFI layer (D22 road: committed generated source, drift-checked).
3. **`bolted-trace`** — the core-track envelope (§9 interaction replay's first artifact;
   D27-style versioned, parse-don't-validate). Owned by bolted; bolt-driver's glass track has
   its own format, and only `driver-bolted` knows both.
4. **A dev-mode host** for the driver agent next to the store (embedded topology), or wire
   attachment in the daemon topology (D30/D31) — dev builds only, same-user socket (D30's
   posture).
5. **`DriveTarget` mapping** (implemented in `driver-bolted`, not here): scopes = facets,
   `read` = snapshot, `version` = store version, `dispatch` = contract verbs.

## Already satisfied by construction (no action, worth knowing)

- bolt-driver's replayability contract (serialized totally-ordered inputs; determinism;
  versioned state) is D35 + the typed verbs behind `bolted-ffi`'s single Mutex + store
  versions — a bolted core passes without new machinery.
- The streaming seam's adopted rulings already treat body chunks as **inputs to the recorded
  input stream** (`docs/design/streaming-seam.md`), so streamed http responses do not
  foreclose replay.
- §9's interaction-replay preconditions (1)–(3) remain as stated there; the glass track
  covers exactly the core-invisible complement (navigation et al.), which is why the one-shot
  effects/navigation session, when it happens, should note: anything it makes core-visible
  shrinks the fragile half of a recorded session (see bolt-driver's
  `docs/design/session-replay.md` for the two-track model).
