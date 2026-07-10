# Step 11 — report: the shells run on bindings nobody wrote by hand

**Status: done. No kill criteria hit.** All four shells (Swift probe + app, Kotlin probe + app)
link the generated `gen-profile-ffi`; `pack:apple`/`pack:android` build it; `spike-profile-ffi`
stays in the workspace as the reference, packed by nothing. Every tier is green on the generated
bindings, and both halves of the owed hardware/UI verification are now measured, not asserted.

## The headlines

### 1. `boltffi pack android` works on a generated crate — M0 passed in one run

The step's primary unknown died quietly. `pack:android:gen` (a clone of `pack:android` with the
same 0.27.3 expansion-env workaround, pointed at `crates/gen-profile-ffi`) produced an arm64 `.so`
with **46 crate-qualified JNI symbols and zero undefined boltffi symbols** — step 05's
dlopen-failure mode, absent. A new Kotlin smoke module (`android/gen-profile-smoke`, the twin of
`apple/gen-profile-smoke`) proved the rest on ART: 6/6 on the headless GMD — load, checkout,
keystroke validation, D23 refusal, checker round-trip, `close()` release. KC1 never came close.

### 2. The D23 controls exist, and both were watched failing

Each probe carries a positive control asserting the typed refusal on a store-released draft
(`FreezeContractTests.swift`, `FreezeContractProbe.kt`). Per the step doc, the swallow was planted
before the control was believed: a `try?` (Swift) / `runCatching {}` (Kotlin) around the resolve
call made exactly that control go red on both platforms; removing it went green. One behavioural
surprise en route: **`runUsernameCheck` on a dead draft returns `false` instead of refusing when no
checker is installed** — the no-checker short-circuit runs before the closed check. The controls
install a checker first; the ordering is recorded below as a step-12 question.

### 3. The hardware bet survives codegen: 0.0432 ms per keystroke, ~23× under the bar

`bench:android:device`, same physical Pixel 8a (API 36), USB serial `4B091JEKB25623`, logcat
cleared before the run, n=2000, against `artifacts/step-11-bench-before.md`:

| Measurement | hand-written (before, `55b7faf`) | generated (after) |
|---|---|---|
| `HW.KEYSTROKE` (try_set + snapshot) p50 / p95 | 0.0363 / 0.0466 ms | **0.0432 / 0.0802 ms** |
| `HW.try_set_username` p50 | 0.0070 ms | 0.0101 ms |
| `HW.snapshot_readback` p50 | 0.0175 ms | 0.0162 ms |
| `HW.noop.kotlin` p50 (pure-Kotlin control) | 0.0005 ms | 0.0010 ms |
| `HW.keystroke.cold_first` (one-shot) | 0.7910 ms | 1.3480 ms |

Read honestly: p50 is +19% and p95 +72% — but the pure-Kotlin noop control itself **doubled**
between the two runs, so some of the delta is device state (thermal/scheduler), not codegen; the
snapshot readback half got *faster*. KC5 gates the steady-state keystroke round-trip against
1.0 ms, and 0.0432 ms is ~23× under it — the regression that would matter was defined in advance
as a factor-of-ten event, and this is not one. The **cold first call crossed 1.0 ms** (1.35 ms,
one-shot); the before-artifact already classified that number as an order of magnitude, not a
statistic, and it is not what KC5 measures — noted, not excused: if a design ever cares about the
first keystroke after process start, this is the number to revisit.

### 4. The migration was exactly the five delta classes — KC2 never fired

No shell needed a capability the generated surface lacks. Everything beyond `sed` fell out of the
delta as promised: D23's `try`/typed catches, the checker's new shape, one arity change, and D24's
type unification — which had one *mechanical* consequence the delta didn't spell out: the three
`display()` overloads in each app collapse to one (on the JVM they would collide after erasure;
in Swift they were just redundant). `LocalizationCoverageTest` needed no surgery at all — it
already drives the live core and renders whatever `ErrorData` comes back, so the step doc's worry
about a hand-maintained key list was solved by that test's design back in step 07.

## What was built / changed

- `android/gen-profile-smoke/` — new Kotlin smoke module (6 tests), consumed in place from
  `crates/gen-profile-ffi/dist/android`.
- `mise.toml` — `pack:apple`/`pack:android` now `dir = "crates/gen-profile-ffi"`;
  `pack:apple:gen`/`pack:android:gen` folded away (they had become identical clones);
  `test:apple:gen`/`test:android:gen` kept as the smoke tier, depending on the main pack verbs.
- `apple/profile-probe`, `apple/profile-app` — link `GenProfileFfi`
  (`crates/gen-profile-ffi/dist/apple`); `project.yml` repointed for the XCUITest project.
- `android/profile-probe`, `android/profile-app` — link `com.example.gen_profile_ffi`.
- `crates/spike-profile-ffi` — **untouched**. Still in the workspace, still built and tested by
  `check` (319 workspace tests), the reference the generated crate is read against.

The final sweep, every tier alone, counts from JUnit XML, `--rerun-tasks` on the Gradle tiers:
`check` 319 · `test:apple` 40 + 14 · `test:apple:gen` 7 · `test:android` 45 · `test:android:hazard`
3 · `test:android:app` 35 · `test:android:gen` 6 · `test:web` 8 (never crossed FFI; unaffected) ·
`test:apple:ui` 9 · `bench:android:device` 4. All zero failures.

## Owed verification, closed

- **`test:apple:ui` on generated bindings**: first post-migration run was **8/9** —
  `test3b_dirtyConflict_takeTheirs` timed out a 3 s wait for the conflict banner during a
  cold-build session. It passed in isolation and a full re-run was 9/9 against the green
  hand-written baseline of 2026-07-10. Verdict: flake, not migration — but that fixed 3 s wait is
  now a known flake candidate.
- **`bench:android:device`**: table above; comparison banked against
  `artifacts/step-11-bench-before.md`.

## Deviations from the step doc

1. **The Localization claim was wrong.** M2 said `Localization.swift` "loses its hardcoded
   `username_taken`; the key now arrives from the declaration." Nothing in either shell's
   localization changes: the key was never hand-written in a shell — it lived in Rust both before
   (`spike-profile-ffi/src/lib.rs:364`) and after (`#[check(failed_key = …)]` in the declaration).
   The shell's template entry maps key → sentence, which is the l10n contract working as designed.
2. **`pack:*:gen` folded rather than kept** (deliverable 6 allowed either). After M5 the clones
   were byte-identical except `dir`; the exit-checklist line "`pack:android:gen` green" is
   satisfied in substance by `test:android:gen` (which packs via `pack:android` and loads the `.so`
   on ART).
3. **`testPostSubmitTombstone` (Swift) asserted the pre-D23 world** — "mutating calls are silent
   no-ops (do not throw)". Under the generated bindings that call throws, so the assertion flipped
   to the typed refusal. This is delta class 2 arriving in a test that *documented* the old
   behaviour, not a contract break.
4. **Process**: the step doc's process note flagged that the Fable/Opus split has never happened.
   This step inverts the usual drift — it was *implemented* in the planning (Fable) session, at the
   owner's explicit request. The CLAUDE.md rewrite question stands, now with evidence in both
   directions.

## Friction log

1. **`runUsernameCheck`'s no-checker short-circuit precedes its closed-draft check**
   (`generated.rs`: the checker is taken from its slot before the draft is looked up). A dead
   draft with no checker answers `false` — indistinguishable from "no checker installed" — rather
   than refusing. Both D23 controls had to install a checker to reach the refusal. Whether the
   closed check should come first is a generator-semantics question → step 12.
2. **Kotlin family renames ripple wider than the Swift ones.** D24 unifies not just the validity
   types but the per-field `*FieldState`/`*FieldSync`/`UsernameCheckFfi` families
   (→ `TextFieldState`/`TextFieldSync`/`CheckStateFfi`), and a mechanical sweep briefly leaves
   duplicate imports where two old families collapse into one new type.
3. **The first `test:apple:ui` run after a repoint is flake-prone** (cold `xcodegen` + xcodebuild
   session inflates fixed UI waits) — re-run before attributing a red to code.
4. **New-module gitignore**: `android/gen-profile-smoke` needed its own `build/`/`.gradle/` ignore
   entries; the first M0 commit swept in ~390 build artifacts and was amended. The repo's
   per-module ignore blocks don't cover new modules silently.

## Open questions (recorded, not resolved)

- **For step 12's `Cleaner` design pass (§9), the M0 observation:** generated Kotlin classes are
  `AutoCloseable`; `close()` is guarded by an idempotent `__boltffi_closed` CAS; **no other method
  consults the flag** (use-after-close still hands a raw dangling pointer to JNI — H2 unchanged);
  nothing registers a `java.lang.ref.Cleaner`. The store-side D23 refusal and the foreign-side
  raw-pointer hazard remain two different problems.
- The check-ordering question from friction 1.
- The cold-first-keystroke number (1.35 ms) if any future design cares about first-paint latency.

## Kill criteria

None hit. KC1 (Android pack) passed on the first run; KC2 (surface gap) never fired — five classes
sufficed; KC3 (unobservable refusal) — both controls observe it, typed; KC4 (generator semantics
change) — `bolted-ffi-gen` untouched; KC5 (1.0 ms) — 0.0432 ms p50, measured on hardware.
