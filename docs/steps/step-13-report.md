# Step 13 — report: the foreign-language emitter, and the seam it made observable

**Status: done. No kill criteria hit.** D28 is real: the Kotlin stash codec and both per-language
contract suites are committed generated source, byte-compared inside `mise run check` with no Gradle,
Xcode, NDK or boltffi CLI in the loop. The suites are generic over a values-only fixture — 33 emitted
tests each, 22 C-IDs (C10 the one exemption) — and they run green on both real tiers. The genericity
golden caught a live leak on its first run; the falsification pass proved every new forbidding test can
actually forbid.

Green sweep this step:

`mise run check` **46** binaries, 0 failures (the three foreign drift checks + the Rust FFI drift + the
two genericity goldens + the manifest accounting test all among them) · `test:android` **80/80** on ART
(ProfileConformanceSuite **33/33**) · `test:apple` **75/75** probe + **20/20** app on XCTest
(ProfileConformanceSuite **33/33**). Web/UI tiers were not re-run — nothing here touches them.

> **The process note, answered.** The step-13 doc was written *after* reading the generator, the emitted
> surface and the probes, and it asked the report to "say where it was wrong anyway." It was — in three
> places, and all three are the planner being **conservative**, not optimistic (the opposite of step
> 12's four). It feared exemptions the surface didn't need, priced Swift as a mirror of Kotlin when only
> its *names* mirror, and framed the genericity golden as a formality that then found a real bug. A
> planner who had not yet emitted a second language would price all three exactly this way. Details under
> "Where the doc was wrong."

## What was built, by milestone

- **M0 — the observability map.** All 23 C-IDs classified in `foreign::BOUNDARY_MAP` and mirrored in
  `docs/CONFORMANCE.md`'s per-language accounting; a four-claim manifest test (`tests/manifest.rs`) ties
  map ↔ document in both directions. **22 emitted, 1 exempt (C10)** — the superseded-token race needs
  two checks in flight, which the atomic single-checker `run_*_check` driver makes unreachable at the
  boundary (exposing raw tokens would be a D18 contract change, not an accessor). KC4 gate (a third
  exempt) cleared with room to spare.
- **M1 — the pipeline, proven on the codec.** `foreign` module in `bolted-ffi-gen`; string-building in
  plain Rust, no template engine; `@generated` banner; `gen:ffi` writes the foreign files; per-feature
  drift tests byte-compare inside `check`. The Kotlin stash codec emitted; `profile-app` moved onto it;
  **`StashCodec.kt` deleted** — step-12 deliverable 5, closed for real. KC1 (drift stays pure) proven:
  text in, text out.
- **M2 — the Kotlin contract suite.** 33 `@Test` methods over 22 C-IDs, generic over the hand-written
  values-only `ProfileConformanceFixture.kt`. Roles assigned from the declaration (checked = the
  `#[check]` field; primary/secondary = the other text fields in order); concrete verb calls emitted, so
  the fixture makes no judgement (KC3 clear). Emulator-verified 80/80. Prerequisite: the
  `delete_canonical` accessor (deliverable 8; commit `3718a47`).
- **M3 — the Swift contract suite.** Same 22 C-IDs, same fixture shape, its own templates (see below).
  75/75 probe + 20/20 app on XCTest.
- **M4 — the genericity golden + the falsification pass.** Deliverable 6 caught a real leak (below);
  deliverable 7's planted-reds proved each new forbidding test bites. Evidence table below.
- **M5 — this report + ROADMAP.**

## Where the doc was wrong (three, all conservative)

1. **Exemptions were over-predicted.** The doc expected "exemptions on the order of C10 and fractions of
   C07/C12." Reality: **only C10**. C07's full precedence clause composes at the boundary
   (`delete_canonical` over a conflict → `Orphaned` outranks `Conflicted`; a conflict plus an invalid
   field → `Conflicted` outranks `Validation`), and C12's create-flow *and* its contrapositive both
   express through the public surface (`rebasing_draft_count == 0`; null a field's stash `base`, restore,
   `== 1`). The surface was more expressive than the planner feared — the one accessor it lacked
   (`delete_canonical`) was added, and then nothing else was missing.

2. **Swift is not a mirror of Kotlin at the body level.** Deliverable 5 said "mirror the Kotlin choices."
   The type and method *names* do mirror — bindgen derives both languages from one Rust `#[export]`
   surface — but the test *bodies* cannot: ARC scope-exit release vs `.use{}`/`close()` (C18/C22 observe
   teardown via counts, not an explicit close), `try`/`XCTAssertThrowsError` vs `try/catch`, `guard case
   … else { XCTFail }` vs `assertTrue(x is …)`, lowerCamel enum cases vs PascalCase sealed subtypes,
   `XCTSkip` vs `Assume`. The Swift templates are their own, ~as much code as the Kotlin ones. What
   mirrored exactly was the *shape*: roles, the values-only fixture, the C-ID coverage, the `RuleFlip`
   data. "Mirror the choices" undersold the body work while correctly predicting the structure.

3. **The genericity golden was priced as a formality; it caught a live bug.** The doc framed deliverable
   6 as defensive ritual ("a suite with one implementor is shaped like it — the memory that named this
   rule is why this deliverable exists"). On its first run it found the Swift C20 template hardcoding
   `ProfileStashFfi` in a comment where the Kotlin emitter correctly used the generic `TextFieldStashFfi`
   (the per-field stash type, a shared DTO). The doc's instinct — that a one-input generator leaks its
   input's shape — was *righter than its framing*: the leak was already there, in exactly the form the
   deliverable exists to catch. The golden is load-bearing, not a rubber stamp.

## The falsification pass (deliverable 7)

Every forbidding test added this step was watched red before being trusted — "a forbidding test can
forbid nothing" (step 10), carried across the FFI boundary.

- **Per-language planted-red (the suites are live on-device, not vacuous).** Flipped c05's
  dirty-after-revert assertion in the *emitter* (`assertFalse`/`XCTAssertFalse` → `…True`), regenerated
  (the committed suite's `git diff` proved the output changed — the step-10 mutation discipline), and ran
  each tier:
  - **Kotlin / ART:** `test:android` → 80 tests, **1 failure**, exactly
    `ProfileConformanceSuite.c05_revertForFree` (`AssertionError: dirty is a function of the data`).
  - **Swift / XCTest:** `test:apple` → ProfileConformanceSuite **32 passed / 1 failed**, exactly
    `testC05RevertForFree` (`XCTAssertTrue failed - dirty is a function of the data`).
  - Both restored to byte-identity with the committed (M2/M3-green) suites; drift green.
- **Positive control per drift check (each reads its own path).** A one-line hand edit to each committed
  foreign file turned *its* drift test red at a **distinct first-differing line** — codec **@85**, Kotlin
  suite **@638**, Swift suite **@390** — and restore returned each to green. Three files, three paths,
  three line numbers: step 10's "a drift check reading a wrong path is green forever," falsified three
  ways. (`include_str!` also fails these tests to *compile* if a path is wrong, so the path is guarded at
  two levels.)
- **Manifest planted-red.** Flipping C11 `emitted → exempt` in `CONFORMANCE.md`'s accounting turned
  `the_accounting_table_matches_the_map_both_directions` red with a precise message (`C11: CONFORMANCE.md
  says exempt, BOUNDARY_MAP says emitted`); restore → 4/4.
- **The genericity golden itself.** Re-introducing the `ProfileStashFfi` leak made
  `a_suite_emitted_for_another_feature_names_no_profile_concept` fail naming `Profile`; its can-fire
  companion (`every_profile_concept_actually_appears_in_the_profile_suite`) proves the eight profile
  needles are non-vacuous — each genuinely appears in the profile suite.

## Deviations from the step doc (smallest-reversible, recorded)

- **The shared drift helper is named `kotlin_drift` but serves all three foreign checks** (Kotlin codec,
  Kotlin suite, Swift suite). Byte-comparison is language-agnostic, so one helper is right; the name is
  now slightly misleading. Its `what` parameter disambiguates every message ("the committed Swift
  contract suite drifted…"), so no message lies. A rename to `foreign_drift` is a trivial future cleanup.
- **`delete_canonical` emits no store-producer snapshot.** There is no canonical left to snapshot after a
  delete, so the store stream is silent and a shell reads `canonical() == None`; the orphaned drafts each
  get one transition snapshot. (Wrapper decision, M2 prerequisite.)
- **The genericity golden reads the *real* `gen-note`/`gen-profile` sources** via `include_str!`, not the
  inline `PLAIN`/`GNARLY` fragments already in `golden.rs`. More honest — it also fails if `gen-note`
  grows a profile-shaped field — and it needs no second lookalike to maintain.

## Accessor gaps from M0 (deliverable 8)

**One**, and it is closed: `Store::delete_canonical` (the FFI verb `deleteCanonical()`), needed by **C07**
(the orphaned-outranks-conflicted precedence arm) and **C11** (deletion orphans). Added to the generated
Rust FFI in `3718a47`; both languages get `deleteCanonical()` at pack time from the one Rust surface. No
other accessor was missing — the "gaps, not chasms" prediction held to a single verb.

## Friction log

1. **`mise run test:android` exits 0 with a failing test.** The planted-red run returned exit code 0
   while the JUnit XML recorded `tests=80 failures=1`. The inherited caution ("counts from the JUnit XML")
   is not a style preference — the tier's exit code *masks* real failures. Always read
   `…/managedDevice/debug/dev34/TEST-*.xml`, never trust the exit code.
2. **The `gen-profile-ffi` crate's package name is `gen_profile_ffi`** (underscore). `cargo test -p
   gen-profile-ffi` errors out (`did not match any packages`) rather than running nothing silently — but
   a `grep`-filtered invocation hides that error and reads as a pass. Cost one nearly-recorded false
   positive in the drift positive controls; caught by reading raw output before trusting it.
3. **`git checkout -- <foreign file>` reverts uncommitted regeneration.** During the drift positive
   controls, restoring a committed suite with `git checkout` reset it to HEAD — which predated the M4
   genericity fix — so the drift test then legitimately failed. The correct restore after an emitter
   change is `mise run gen:ffi`, not `git checkout`. (mtime also bites: `mv`/`git checkout` can leave an
   older mtime than the last build, so cargo reuses a stale binary; `touch` after restoring.)
4. **`cargo fmt --all` after every `gen:ffi`** (step-12's friction 1, unchanged). The Rust half is
   rustfmt-owned; the foreign half is emitter-owned and nothing may reformat it — which is what keeps the
   byte comparison honest.

## Open questions (recorded, not resolved)

- **`kotlin_drift` → `foreign_drift` rename** — cosmetic, deferred to avoid churn mid-step.
- **A second *packed* feature.** This step proved genericity at the text level (the honest scope); C#
  (step 14) becomes the emitter's third fixture and the first genericity test that actually *packs and
  runs* on the platforms. If the emitter has a feature-specific assumption the text golden cannot see, C#
  is where it surfaces.
- **No Apple codec** (deliverable 3, by design): nothing stashes on Apple; an emitted file with no
  consumer is dead code. Revisit when an Apple shell persists an edit session.

## Kill criteria

None hit. **KC1** (drift can't stay pure): the three foreign drift checks run inside `mise run check`
with no Gradle/Xcode/NDK/boltffi — text in, text out, proven at M1 and never walked back. **KC2** (`dist/`
or bindgen internals): the emitted suites consume only the public `#[export]`/`#[data]` surface; the one
missing capability was added as an *accessor* (`delete_canonical`), not by reaching inside a binding.
**KC3** (the fixture needs a judgement): every fixture member is a constant — even C08's tier-2 rule is
`RuleFlip` *data* (name, dirty edits, flipped canonical, pins), no callback, no branching; it held in
both languages. **KC4** (exempt > a third): 1 of 23.

## Handoff to step 14 (C#)

Step 14 inherits a working foreign emitter and three things to lean on or watch: (1) the **role model**
(checked / primary / secondary from the declaration) and the **values-only fixture** contract — C#
writes one ~30-line fixture per feature and gets the suite; (2) the **genericity golden** as the pattern
for a third feature — extend `PROFILE_CONCEPTS`-style needles per language, keep the can-fire companion;
(3) the open **`foreign_drift` rename** and the friction-2 package-name trap. C# is also the first chance
to promote the genericity proof from text-level to *packed-and-run*, which is where any remaining
emitter assumption the text golden cannot see would finally show.
