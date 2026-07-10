# Step 12 — report: the generated layer is safe to hold, and the doc met the code

**Status: done. No kill criteria hit.** The lifecycle bug step 11's controls found is fixed and
watched red on three layers; leak-freedom is a per-language contract test that bites; the stash is a
versioned, parse-don't-validate envelope (D27) with C23 to keep it honest; the ergonomics and l10n
items landed where the toolchain allowed and converted — loudly, recorded — where it did not. The
full sweep is green:

`check` 42 · `test:apple` 42 + 20 · `test:apple:gen` 7 · `test:android` 47 · `test:android:app` 36 ·
`test:android:hazard` 3 · `test:android:gen` 6 · `test:web` 8 · `test:apple:ui` 9 — all zero failures.

> **The process note, answered first.** This is the first step doc authored in a planning session for
> a separate implementation session, and it asked the report to "say where this doc was wrong in ways
> an implementer-author would have known." It was wrong in **four** places, all of them the same shape:
> the doc priced work the toolchain cannot do as if it could. An implementer-author, having just felt
> BoltFFI's edges in steps 05/10/11, would have priced them differently. The split earned its keep
> *specifically here* — a planner's optimism about the FFI seam is exactly what an implementer catches,
> and the milestones that converted (M4) or changed mechanism (M3, M5) are the evidence. Details below,
> under "Where the doc was wrong."

## What was built, by milestone

- **M0 — passthrough probe.** No annotation passthrough in BoltFFI 0.27.3 (`@Parcelize`/`Codable`
  cannot be stamped); the DTO wire ser/de exists but is `internal`. Recorded, and it re-priced M4/M5.
  (`artifacts/step-12-m0-passthrough.md`.)
- **M1 — the D23 ordering fix.** `bolted-ffi-gen`'s check driver now resolves the draft's liveness
  *before* the no-checker short-circuit, so `run_username_check()` on a released draft refuses (typed)
  whether or not a checker is installed. Regenerated; drift green. Controls on **three** layers, each
  watched red against the unfixed generator and restored: the Rust `tests/wrapper.rs` three-cell test,
  and both the Swift and Kotlin probes' no-checker controls (the typed throw crosses the binding).
- **M2 — leak-freedom as D26's backstop.** Both platforms already had the test D26 asks for; M2
  elevated them from an incidental `== 0` to the named "teardown → baseline" contract and tied each to
  D26. Planted-red: removing `draft.close()` from `onCleared()` fails the Kotlin test
  ("expected:0 but was:1") — the backstop bites the exact leak a `Cleaner` would have hidden until GC.
- **M3 — C23 + the D27 versioned envelope.** C23 (the KC3 gate) tested first and held across all four
  value fixtures: a degraded ancestor restores dirty-from-unset and conflicts on rebase. D27: the
  schema version rides the generated DTO; `accept_stash` is a typed parse gate returning a
  `StashAcceptedFfi` token; `restore` takes only the token (parse-don't-validate in the type). Core
  signatures unmoved (KC2 held). Verified on Rust + both probes + a Kotlin app test of the
  `stashWasRefused` observable.
- **M4 — codec deletion: converted.** See "Where the doc was wrong."
- **M5 — l10n coverage + ergonomics.** Swift got its first localization coverage test (the real gap),
  watched red against a planted missing template. Name-collision refusal added as a generator tripwire
  with a golden test. 6a → filing; 6b → not-needed-yet. (`artifacts/step-12-m5-ergonomics-l10n.md`.)
- **M6 — Compose rule, filings, report, sweep.** The Compose parameter rule is now stated in
  `ProfileForm.kt`'s header (where a Compose author first meets the file) and recorded as a
  `bolted-check` Phase-4 candidate; the app audit found **no regressions** (every state-reading helper
  already threads `snapshot`). Five upstream filing drafts under `artifacts/step-12-upstream/`.

## Where the doc was wrong (the four)

1. **Deliverable 5 (M4) — "delete the hand-written codec" is not a modest task; it needs step 13.**
   The doc's second branch ("the codec becomes a generated file beside the bindings") reads like an M4
   chore, but `bolted-ffi-gen` emits only Rust — generating a *Kotlin* codec is the foreign-language
   emitter step 13 is chartered to build, with unresolved infra (where generated Kotlin lives, how it
   builds, how it drift-checks). So deliverable 5 **converted** (one conversion; KC1's budget is >2 and
   its actual trigger — patching `dist/` — was never touched). What the deliverable was *for* is done
   anyway: M3 moved the version authority off the codec, so `StashCodec` is now pure structural
   marshalling. (`artifacts/step-12-m4-codec.md`.)

2. **Deliverable 3 (M3) — D27's `restore` cannot be fallible-returning-a-handle.** The natural D27
   shape (`restore(stash) -> Result<Draft, StashRefused>`) does not compile: BoltFFI cannot return a
   class handle from a throwing method (`Result<Handle, E>` is not `WireEncode`). The design became a
   `accept_stash → token → restore` two-step — which is *stronger* (parse-don't-validate in the type),
   so the constraint improved the design, but it is a workaround, filed as upstream draft 05.

3. **Deliverable 7 (M5) — the "generator-emitted declared key list" cannot be complete.** Rule error
   keys are runtime strings inside `#[bolted::rules]` impl bodies (the generator never sees
   `corporate_email_domain`), `required` is a `bolted-core` constant, and `draft_orphaned` is
   shell-supplied. A declared list would silently omit them. Drive-the-core (Kotlin's design, which
   step 11 validated) sees every key the core emits and is therefore *complete* — so Swift got a
   drive-the-core test, not a declared-list one. A declaration-driven list would need rule keys hoisted
   to declared attributes: an ARCHITECTURE change, left for planning.

4. **Deliverable 2 (M2) — the leak-freedom tests already existed.** The doc said "if `onCleared()`
   already closes, the test pins it." It does, and a test already pinned it on both platforms. M2's
   real work was elevation and the planted-red, not construction.

A fifth, smaller one: **deliverable 6c's collision refusal is a tripwire, not a live guard** — every
declaration-derived name is already suffixed, so no real declaration can produce a bare `Date`/`Error`.
It guards future generator changes; the live collision surface (hand-written composites) is not named
by the generator (a design decision, recorded).

## The pattern under the conversions

M4's codec, 6a's checker helper, and 6b's Sendable extension all funnel to **one** root cause:
`bolted-ffi-gen` emits only Rust, and each needs a *foreign* file. That is one coherent finding —
step 13's foreign-language-emission charter — not three shortfalls, and it is the "generator seam"
signal KC1 gestures at. No `dist/` was patched for any of it.

## Deviations from the step doc

- **M4:** `StashCodec.kt` not deleted (converted; see above). Exit-checklist item adjusted.
- **M3:** `restore` is a token-consuming pair, not a fallible single call (toolchain constraint).
- **M5/7:** Swift l10n test is drive-the-core, not declared-list (completeness; see above).
- **M5/6b:** Swift `@unchecked Sendable` not added (no need under Swift 5.9 non-strict concurrency).
- The version's *value* is a fixed generated constant (`STASH_SCHEMA_VERSION = 1`); deriving it from
  the declaration's constraints is D27's Phase-4 `bolted-check` work. The mechanism does not wait.

## Friction log

1. **`cargo fmt` is a required step after `gen:ffi`.** `gen:ffi` writes `prettyplease` output; the
   committed file must be `rustfmt`'s, and `check` runs `cargo fmt --all --check`. Regenerate → fmt →
   commit. (Cost me one red `check` per regeneration until internalised.)
2. **The GMD task rejects `--tests`.** `mise run test:android:app -- --tests '…'` fails with "Unknown
   command-line option"; the managed-device task takes no test filter. Read counts from the JUnit XML
   and run the whole tier.
3. **`test:apple:ui` did not flake this run** (step 11 flagged the first-post-repoint run as
   flake-prone). 9/9 on the first attempt, ~66 s. Noted, not relied upon.

## Open questions (recorded, not resolved)

- **Rule error keys are not declared** (they live in the rule impl body). If a declaration-driven
  l10n-coverage or key-manifest is wanted, rule keys need hoisting to a declared attribute — a §-level
  question (it changes how rules name their errors, adjacent to constraint.rs's "does `Required` belong
  on the value or the field").
- **The stash schema version's derivation** — constant vs. constraint-hash vs. `bolted-check` semver —
  is deferred to Phase 4 (D27). The refusal mechanism is built and does not depend on the choice.
- **Custom composite type naming** is outside the generator (hand-written `custom`), so the collision
  refusal cannot cover it without bringing composite naming into the generator.

## Kill criteria

None hit. **KC1** (no `dist/` patching): never patched; the items that could not be done are recorded
as filings/step-13 handoffs, and its ">2 conversions" counter is about `dist/`-patching, of which
there were zero. **KC2** (D27 requiring a core-signature change): the core is untouched; the version
lives on the DTO. **KC3** (the degradation claim false): C23 held across all fixtures — tested before
the gate was built. **KC4** (generator change altering an existing C-ID's observable semantics): the
only generator behaviour change is the D23 ordering fix, which *restores* stated semantics.

## Handoff to step 13

Step 13 (per-language contract tests from the C-IDs) inherits a concrete foundation problem, not a
blank sheet: it must build the **foreign-language emitter** M4/6a want; M0's finding that BoltFFI's
wire ser/de is `internal` (so step 13's tests cannot lean on it to round-trip DTOs either — same
blocker, same two paths in filing 04); and the `StashAcceptedFfi`/`StashRefusedFfi` surface D27 added.
The Compose parameter rule and the l10n drive-the-core shape are `bolted-check` lint candidates for
Phase 4.
