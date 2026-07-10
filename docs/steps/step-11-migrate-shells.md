# Step 11 — migrate the shells onto the generated FFI

**Phase 3 — Framework extraction. Status: done — see [the report](step-11-report.md).**

Step 10 built the generator and left the shells where they were. `mise run pack:apple` and
`pack:android` still build the **hand-written** `spike-profile-ffi`; the four Swift and Kotlin shells
have never linked a generated binding. This step repoints them, and stops there.

> **Process note** (carried from step 10, and getting worse). `CLAUDE.md` splits planning (Fable) from
> implementation (Opus). This step doc was authored by the implementer, for the **sixth** time — steps
> 06 through 11. This round the owner explicitly asked for it, which makes it a decision rather than a
> drift, but the rule in `CLAUDE.md` still describes a process that has never once happened. Either a
> planning session authors step 12, or `CLAUDE.md` should be rewritten to describe what actually
> occurs. Flagged, not resolved.

---

## Scope: this step is the migration, and only the migration

ROADMAP's step-11 sketch bundled three things: the shell migration, a nine-item FFI hardening list,
and three upstream bug filings. That is not one step, and two of the hardening items —
the `Cleaner` backstop and stash schema evolution — are **ARCHITECTURE §9 OPEN questions**, which
`CLAUDE.md` forbids an implementation session from resolving.

The list is therefore split:

- **Step 11 (this one)** — prove the generated crate packs for Android, migrate the four shells, retire
  the hand-written FFI from the build.
- **Step 12 — FFI hardening.** The nine-item list, the two §9 questions, the upstream filings. Every
  item on it is a *generator* change whose only test is a shell that consumes generated bindings — so
  it cannot start until this step lands. Its doc gets authored after a design pass on the two §9
  questions.
- **Step 13** — C# port + generator (was 12).

## What step 10 handed over

`docs/steps/artifacts/step-10-surface-delta.md` measured the migration rather than guessing at it:
**62 declarations hand-written, 57 generated, 42 identical.** Every one of the 20 removals maps to an
addition, and no behaviour differs. The five classes of change:

1. **D24 renames** — `UsernameValidity`/`PersonNameValidity`/`EmailValidity` → `TextValidity`, and the
   two sibling families; `PlainDateRange` → `AvailabilityRaw`; `DateRangeFieldStashFfi` →
   `AvailabilityStash`.
2. **D23's typed refusal** — `resolveKeepMine`, `resolveTakeTheirs` and `runUsernameCheck` now throw
   `DraftClosedFfi`.
3. **The checker capability** — `UniquenessChecker { checkUnique(username:) -> UniquenessVerdictFfi }`
   → `UsernameChecker { check(value:) -> CheckVerdictFfi }`, and the verdict no longer carries the
   error: `.fail` plus a declared `#[check(failed_key = "username_taken")]`, so **no shell names a
   localisation key**.
4. **One arity change** — `trySetAvailability(start:end:)` → `trySetAvailability(raw:)`. D20's shadow,
   recorded for the third time. Not a contract change: the same two dates cross, in the same order,
   with the same validation and the same typed error.
5. **The module and package names** — `SpikeProfileFfi` → `GenProfileFfi`,
   `com.example.spike_profile_ffi` → `com.example.gen_profile_ffi`.

Measured blast radius: **25 source files and 5 build files, ~217 grep hits.** The single densest file
is `android/profile-app/.../ProfileViewModel.kt` (548 lines, 37 rename hits).

Two things checked before this doc was written, so they need not be feared during it:

- **The stash wire format does not move.** `StashCodec` encodes hand-chosen JSON keys (`"username"`,
  `"availability"`) behind a `FORMAT_VERSION` guard, not Kotlin type names. The D24 renames are
  invisible to a persisted stash, and §9's stash-schema-evolution question stays out of this step.
- **`gen-profile-ffi/src/lib.rs` carries `pub use generated::*;` and `pub use custom::*;`**, which is
  what step 10 found the pack-time metadata blob requires. Necessary for M0, and not sufficient — see
  the kill criterion.

## The trap, named before it is walked into

Four of the five classes are renames: `sed`, essentially. The fifth is not, and neither is D23.

**A migration that compiles and passes is not evidence that the refusal ever fired.** `try?` in Swift
and `runCatching {}` in Kotlin will each swallow a `DraftClosedFfi` and hand back exactly the silent
no-op D23 exists to abolish — and every test in the suite will stay green, because none of them ever
calls a mutating verb on a draft the store already released.

This is `a-forbidding-test-can-forbid-nothing` applied *before* the fact. Each shell gets a **positive
control**: check out a draft, `submit` it (C17 releases it store-side), then call `resolveKeepMine` on
the dead handle and assert the typed refusal reaches the shell. Their natural homes exist already —
`apple/profile-probe/.../FreezeContractTests.swift` and
`android/profile-probe/.../FreezeContractProbe.kt`, both of which already test C17's freeze and both of
which today assert nothing about what a mutating verb does afterwards.

Before believing the control, **plant the swallow** — wrap one call site in `try?`, watch the control
go red, put it back. A control that has never failed is a needle that has never fired.

## Deliverables

1. `mise run pack:android:gen` — a new verb, and the proof that a *generated* crate packs for Android.
2. A Kotlin smoke suite against the generated bindings, mirroring `apple/gen-profile-smoke`: the `.so`
   loads on ART, a draft checks out, a keystroke validates, `close()` releases.
3. `apple/profile-probe` and `apple/profile-app` linking `GenProfileFfi`.
4. `android/profile-probe` and `android/profile-app` linking `com.example.gen_profile_ffi`.
5. A D23 positive control in each of the two probes, each verified to fail when the refusal is swallowed.
6. `pack:apple` / `pack:android` repointed at `gen-profile-ffi`; `pack:apple:gen` / `test:apple:gen`
   folded away or kept as the smoke tier.
7. **`crates/spike-profile-ffi` is not deleted.** It stays in the workspace, still built and tested by
   `mise run check`, as the reference the generated crate is read against. A step that deletes its own
   reference proves nothing.
8. `docs/steps/step-11-report.md` + ROADMAP status.

## Milestones

**M0 — the Android pack gate. Nothing else starts until this is green.**
`boltffi pack android` has never been run on a generated crate. `pack:android` hardcodes
`dir = "crates/spike-profile-ffi"` and constructs the `BOLTFFI_BINDING_EXPANSION_*` environment from
`$PWD` to work around step 05's upstream bug — an environment that, per step 10, is exactly what
triggers the whole-crate metadata blob that resolves every exported type from the crate root. Write
`pack:android:gen`, run it, load the `.so` on ART. Also *observe, without deciding*: does the generated
Kotlin emit `AutoCloseable`, and does any generated method consult `__boltffi_closed`? That evidence
is step 12's `Cleaner` input; record it, do not act on it.

**M1 — `apple/profile-probe`.** The lowest-level Swift consumer; it asserts the FFI contract directly,
so it fails legibly. Includes the D23 positive control in `FreezeContractTests.swift`.

**M2 — `apple/profile-app`.** `ProfileViewModel.swift` (465), `ProfileForm.swift` (300),
`ProfileViewModelTests.swift` (312), `Localization.swift`, `project.yml`, `Package.swift`. The
localisation file loses its hardcoded `username_taken`; the key now arrives from the declaration.

**M3 — `android/profile-probe`.** Depends on M0. `LifecycleProbe.kt` (264) and `StreamProbe.kt` (234)
are the two that exercise the parts Apple's ARC hides. D23 positive control in `FreezeContractProbe.kt`.

**M4 — `android/profile-app`.** `ProfileViewModel.kt` (548), `ProfileForm.kt` (180), `StashCodec.kt`,
and the four instrumented tests. `LocalizationCoverageTest.kt` (164) is the interesting one: it
currently proves every error key has a string, using a hand-maintained list that includes
`username_taken`. With `failed_key` declared, that list should come from the bindings — and if it
cannot yet, say so, and leave it for step 12's l10n item rather than quietly weakening the test.

**M5 — retire the verbs, and the sweep.** `pack:apple`/`pack:android` point at `gen-profile-ffi`.
Every count read out of JUnit XML, one verb at a time (step 10, friction 5: `test:android` and
`test:android:hazard` write to the *same* XML file, and the results directory is
`build/outputs/androidTest-results`, not `test-results` — deleting the obvious path clears nothing).
Force `--rerun-tasks`: `test:android:app` can print `BUILD SUCCESSFUL` having run no test.

**M6 — report, ROADMAP, ARCHITECTURE §5 crate-layout note if anything moved.**

## Kill criteria (real; if hit, stop and report)

1. **`boltffi pack android` cannot produce loadable bindings for `gen-profile-ffi`**, and the cause is
   upstream rather than in our tree. Then the Android half of this step does not happen, the Swift half
   ships alone, and the upstream filing that step 10 owed becomes step 11's first deliverable. Do not
   hand-patch generated Kotlin to get a green suite.
2. **A shell needs a change outside the surface delta's five classes** — i.e. the generated surface is
   missing a capability the shell requires. Then the extraction changed the contract without saying so,
   which is KC4 of step 10 arriving late, and the delta document was wrong.
3. **The D23 positive control cannot be written** — a shell cannot observe the typed refusal. Then D23
   is unobservable from the platform it was designed for, and the decision returns to a design session.
4. **A shell forces a change to `bolted-ffi-gen`'s output *semantics*** (not a name, not formatting).
   The generator is supposed to be finished. If migrating reveals otherwise, the smoke test proved less
   than it claimed.
5. **The per-keystroke round-trip regresses past the 1.0 ms bar.** Nothing in this step touches the hot
   path — but that is the sentence step 09 found false when it checked, so it is measured, not asserted.

## Owed verification, and the window that is closing

- **`mise run bench:android:device` — the "before" half is now captured.** Run 2026-07-10 on a
  physical Pixel 8a (API 36) over USB, against the hand-written bindings at code state `55b7faf`:
  **0.0363 ms p50 / 0.0466 ms p95** per keystroke round-trip, n=2000 — a ~27× margin under KC5's
  1.0 ms bar. Full table in `artifacts/step-11-bench-before.md`. What remains owed is the **"after"**
  run on the generated bindings, same device, once M4 is green — the comparison goes in the report.
  The verb refuses an emulator by design (step 07, KC4); if the device is unavailable at that point,
  the report says so rather than substituting an emulator figure.
- **`mise run test:apple:ui` — first run 2026-07-10, green: 9 tests, 0 failures** (ProfileUITests ×8
  + SmokeUITest ×1, 66 s) on the hand-written bindings at `332fe58`. `ProfileForm.swift` is 300 lines
  and this XCUITest suite is the only automated check on the form's behaviour; it now constitutes a
  green baseline, so a red after M2 is attributable to the migration. What remains owed is the
  re-run on generated bindings after M2. Needs Xcode plus a logged-in GUI session holding
  Accessibility permission.

If the device or the GUI session is unavailable when the "after" measurements are due, the migration
still proceeds — and the report says plainly that the SwiftUI form was migrated unverified at the UI
tier, or that the generated bindings' hardware cost is asserted from headroom (~27× under the bar on
the hand-written layer) rather than measured.

## Non-goals (→ step 12, "FFI hardening")

`java.lang.ref.Cleaner` backstop (§9) · `@Parcelize` / `Codable` on generated DTOs (which would delete
`StashCodec.kt`) · the Compose parameter-passing rule · l10n key coverage per target · `close()` in
`onCleared()` · `Sendable` on `Send + Sync` Rust classes · `fun interface` for single-method capability
traits · a platform-stdlib name-collision policy (`Date`, `URL`, `Data`, `Error`) · per-language
contract tests generated from the C-IDs · stash schema evolution (§9) · the three upstream filings,
unless KC1 promotes one.

## Exit checklist

- [x] `mise run check` green; `spike-profile-ffi` still in the workspace and still tested. **319, 0 failures.**
- [x] The generated `.so` loads on ART: `test:android:gen` 6/6 (M0 ran it as `pack:android:gen`,
      folded into the repointed `pack:android` at M5 — deliverable 6's "folded away" branch).
- [x] `mise run test:apple` green on **generated** bindings. **Probe 40, app VM 14.**
- [x] `test:android` 45 · `test:android:app` 35 · `test:android:hazard` 3 — each run alone with
      `--rerun-tasks`, each count read from JUnit XML. 0 failures.
- [x] `mise run test:web` unaffected. **8/8.**
- [x] A D23 positive control in each probe, **each verified to fail** with the refusal swallowed
      (planted `try?` / `runCatching {}`, watched red, removed).
- [x] `bench:android:device` on generated bindings: **0.0432 ms p50 / 0.0802 ms p95**, n=2000,
      Pixel 8a — compared in the report against `artifacts/step-11-bench-before.md`. KC5 not hit.
- [x] `test:apple:ui` on generated bindings: 8/9 cold first run (test3b banner-wait flake), 9/9 in
      isolation and on the full re-run.
- [x] `docs/steps/step-11-report.md` written; ROADMAP updated; §9 untouched by this step.
