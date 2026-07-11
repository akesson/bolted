# Step 15 — the boltffi 0.27.5 bump: resume C#, or prove why not

**Phase 3 — Framework extraction. Status: ready.**

Step 14 stopped on kill criterion 1: the C# backend's check driver throws before the checker can run,
and the defect lives in upstream bindgen output. Two days later the ground moved — boltffi shipped
**0.27.4 (Jul 9)** and **0.27.5 (Jul 10)**. This step does two things, in an order a test decides:

1. **Bump the pinned toolchain 0.27.3 → 0.27.5** and prove the entire ladder still holds — every
   backend regenerated, every tier re-run, every count read from an artifact.
2. **Let the step-14 tripwire deliver the verdict.** `CallbackDriverProbe.TheCheckDriverIsBrokenOnThisBackend`
   asserts the *broken* behaviour and was designed to go red the moment upstream fixes the driver. If
   it goes red: resume step 14's unbuilt M2/M3 (the emitted C# contract suite, genericity,
   falsification). If it stays green: the driver is still broken at 0.27.5, and the step banks the
   bump plus the empirical evidence for the filing.

Either way the step ends with the **upstream issue kit**: all six drafted findings (step 12's five +
step 14's one) re-verified against 0.27.5, each surviving one packaged with a minimal repro skeleton
that *proves* it, ready for the owner to file. **Nothing is posted, filed, or sent anywhere — hard
rule, owner-approval gate. This includes `gh issue create`, draft PRs, comments, and "just checking"
API writes.**

## What the planning pass verified (by checking, 2026-07-11)

- **crates.io**: `boltffi_cli` 0.27.4 published 2026-07-09, 0.27.5 published 2026-07-10. We pin
  0.27.3 in five places: `setup:boltffi`'s `want` and four `Cargo.toml`s (`bolted-ffi`,
  `gen-note-ffi`, `gen-profile-ffi`, `spike-profile-ffi` — all `"0.27.3"`, caret semver, so
  `cargo update` floats them; the *written* pins should still be bumped to keep intent legible).
- **Release notes name no C# fix.** But two entries are adjacent enough to matter:
  - 0.27.4 **#622** "Fix native OptionScalar exports returning f64 instead of FfiBuf" — the same
    *class* of defect as ours (payload/envelope confusion when choosing an export's wire signature).
    Our `MarshalAs(I1)`-on-`FfiBuf` bug may or may not have died with it. **Only M1 can say.**
  - 0.27.5 **#647** "lower `Result<Class, E>` returns as object handle, not wire-encoded record" —
    plausibly the fix for **upstream draft 05** (throwing method cannot return a class handle). The
    issue kit must re-verify draft 05 first; if fixed, D27's token-shaped `accept_stash` stays (it is
    *stronger* than the handle-returning shape, per step 12), but the draft is retired with the PR
    reference instead of filed.
- **The upstream tracker knows none of our six findings.** Searched for MarshalAs, C#/Result/bool —
  zero hits. Whatever we don't file, nobody fixes on purpose.
- **The regenerated foreign surface may churn.** 0.27.4/0.27.5 carry Swift and Kotlin codegen fixes
  ("fix swift lexical bindings", "preserve dependency data impl methods", Android JNI packaging).
  `dist/` is uncommitted (repacked on demand), but our **committed emitted artifacts** (D28: the
  Kotlin stash codec, both contract suites) compile against the *public generated surface* — if that
  surface shifted, they need regenerating or mechanical rename-level updates, which are in scope
  (step 11 precedent: D24 renames). Semantic shifts are not — see kill criteria.

## What steps 13/14 hand over (use it, don't re-derive it)

- **The tripwire**: `csharp/profile-probe/CallbackDriverProbe.cs` — the M1 verdict is one
  `mise run test:csharp` away.
- **The C# emitter model, fully mapped** (step-14 report, "What M2/M3 would need"): `readonly record
  struct` DTOs; `abstract record` + nested `sealed record` DUs; PascalCase; `with` copies; `enum`
  field-ids; `<Value>ErrorFfiException` wrappers carrying `.Error`; NUnit `Assert.Throws`/`Assume.That`
  as the JUnit-Assume/XCTSkip analogues. The emitter is a mechanical port of the step-13
  marker-substitution model (`@@MARKER@@`, plain Rust string-building, no template engine).
- **The seam**: `pack:csharp`/`test:csharp` exist, dotnet task-scoped, NuGet cache eviction and
  `DOTNET_CLI_UI_LANGUAGE=en` already handled; TRX is the authoritative count source.
- **The boundary map**: `BOUNDARY_MAP` (22 emitted C-IDs, C10 exempt) is language-neutral and
  unchanged — C# consumes it as Kotlin and Swift do.
- **The lifecycle law is now written down**: ARCHITECTURE v1.7 (this planning pass) amended §4/§6 and
  D26 — the C# leak-freedom test **must assert its baseline before any GC**. The emitted C# suite's
  D26-shaped test inherits that shape from `LifecycleProbe.DeterministicDisposeReturnsCountsToBaseline_NoGCInvolved`.

## Scope: one bump, one branch decision made by a test, one issue kit

The step doc deliberately branches at M1; both branches are honest completions. If M0's churn is
larger than expected and consumes the session, **stop after M1 + M4 and report** — M2/M3 split into a
step 16 with the bump already banked. Do not rush an emitter to claim the branch.

## Deliverables

1. **The bump.** `want="0.27.5"` in `setup:boltffi`; the four `Cargo.toml` pins; `Cargo.lock`.
   `mise run check` green. All packs regenerated; all tiers green: `test:web`, `test:apple` (+`:gen`),
   `test:android` (+`:gen`, forced rerun), `test:apple:ui`, `test:android:app` (the shells link
   regenerated `dist/`, so the UI tiers are part of the bump's proof, not optional), `test:csharp`.
   Every quoted count read from the artifact (JUnit XML / TRX / cargo output), not the wrapper.
2. **The C# verdict**, recorded by the probe. If the driver works: flip `CallbackDriverProbe` from
   asserting the break to asserting the contract — checker invoked, verdict lands, `[Pending, Passed]`
   on the stream (D10), reentrant checker doesn't deadlock, `run_username_check`'s own D23
   `DraftClosed` refusal (the one step 14 recorded as unobservable). The step-14 blocked probes come
   alive here.
3. *(branch A — driver fixed)* **The emitted C# contract suite**: `csharp_contract_suite` emitter in
   `bolted-ffi-gen` on the D28 model; committed generated source at a path `dotnet test` already
   compiles; byte-drift-checked inside `mise run check`; the `kotlin_drift` → `foreign_drift` rename
   lands here (step-13's recorded cleanup). 22 emitted C-IDs, values-only fixture, no judgement in
   the fixture (KC3 discipline).
4. *(branch A)* **Genericity + falsification**: the genericity golden run on `gen-note`'s surface;
   per-language planted-red proving the dotnet tier's failure mode — nonzero exit, TRX counts — the
   debt step-14 friction 3 named; every new drift check watched red before being trusted
   (regenerate-first, or the drift check makes the mutation pass vacuous).
5. *(both branches)* **The upstream issue kit** under `upstream/boltffi/`: for each of the six drafts,
   a disposition at 0.27.5 — **retired** ("fixed in 0.27.x by #NNN", evidence attached) or **alive**
   (final issue text + a minimal, self-contained repro skeleton: smallest crate/config that exhibits
   the defect, with the one command that shows it and the output it prints). The bump run itself
   supplies most re-verification evidence for free — 01 (pack android env), 02 (use-after-close UB),
   03 (bindgen macro blindness), 05 (Result<Class,E>) all have existing probes/artifacts that re-run
   under 0.27.5. **Nothing posted; the kit is local files for owner review.**
6. **Report + ROADMAP** (`step-15-report.md`), including the churn log: what 0.27.5 changed in the
   regenerated surfaces, as evidence for how expensive lagging the pin actually is.

## Milestones

- **M0 — the bump.** Pins, `cargo update`, `setup:boltffi` reinstalls, `gen:ffi`, `mise run check`,
  all packs, all tiers (forced reruns; artifact counts). Commit.
- **M1 — the verdict.** `mise run test:csharp` at 0.27.5. Tripwire red → driver fixed → flip the
  probe (deliverable 2), commit, proceed to M2. Tripwire green → still broken → record it in the
  issue draft as "reproduces at 0.27.5", skip to M4.
- **M2 (branch A) — the emitter.** Deliverable 3. Commit.
- **M3 (branch A) — genericity + falsification.** Deliverable 4. Commit.
- **M4 — the issue kit.** Deliverable 5. Commit.
- **M5 — report + ROADMAP.** Commit.

## Kill criteria (real; if hit, stop and report)

1. **The bump cannot go green without patching `dist/` or bending a frozen contract.** Rolling back
   to 0.27.3 is the recorded fallback, not a failure — but a green suite bought by editing bindgen
   output or by weakening an invariant is a kill, exactly as in step 14.
2. **0.27.5 introduces a *new* four-feature break on any backend** (VISION risk #1 again — e.g. the
   IR migration fixes broke a callback path that worked at 0.27.3). Stop; the issue kit grows a
   seventh entry; the pin decision goes back to planning.
3. *(branch A)* **The emitted C# suite cannot honestly cover an emitted C-ID** beyond C10's standing
   exemption. No skips, no `Assume` shims around a broken verb — that is the step-14 workaround
   refusal, still in force.

## Non-goals (→ elsewhere)

- **Filing/posting anything upstream** — the kit is prepared locally; the owner files after review.
- WinUI / Windows hardware (the step-07 KC4 precedent stands; the seam is host-portable).
- The `Feature` trait or any ARCHITECTURE §9 question — the trait's design session still gates
  Phase 4 and is not this step.
- `bolted-check` / Phase-4 work.
- Performance work beyond the tiers' existing assertions.

## Inherited cautions

- `test:android`'s **exit code lies** — JUnit XML only; Gradle can report success while running
  nothing — `--rerun-tasks` and delete stale result XML before quoting counts.
- `dotnet` localizes its console; the TRX is the source of truth (`test:csharp` already handles both).
- The NuGet cache shadows re-packed fixed-version packages (`test:csharp` already evicts).
- A drift check makes a mutation pass vacuous — regenerate first, prove the output changed.
- A forbidding test can forbid nothing — every planted red must be *watched* red, every refusal
  needs its positive control (`Assert.Throws` serves).
- The genericity golden is not a formality — it caught a live Swift leak in step 13.
- Commit per milestone; never `git -C`; build/test only via `mise run check` / `mise run test`.

## Exit checklist

- [ ] All five pins at 0.27.5; `mise run check` and every tier green, counts artifact-derived; churn
      logged in the report.
- [ ] The C# verdict recorded **by the tripwire**, and the branch taken accordingly.
- [ ] Branch A: emitted suite + `foreign_drift` rename + genericity golden + dotnet planted-red
      failure-mode proof all banked — or branch B: explicitly not built, with the tripwire output as
      the reason.
- [ ] The issue kit: six dispositions, each either retired-with-evidence or alive-with-skeleton;
      **nothing posted anywhere**.
- [ ] `step-15-report.md` written; ROADMAP row updated; ARCHITECTURE untouched (v1.7 already carries
      this step's design input).
