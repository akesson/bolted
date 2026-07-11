# Step 15 — report: the boltffi 0.27.5 bump; C# stays broken (branch B)

**Status: done. Branch B taken — the C# check driver is still broken at 0.27.5, so the bump is banked
and the upstream issue kit is finalized; the emitted C# contract suite (step-14 M2/M3) was NOT built,
by design.** M0, M1, M4, M5 delivered; M2, M3 not applicable on this branch.

Planned by Fable; implemented by Opus (the project tracks which model did what — see the commit
co-author lines).

## The verdict, in one line

The step-14 tripwire `CallbackDriverProbe.TheCheckDriverIsBrokenOnThisBackend` **still passes at
0.27.5** (it asserts the break), and the fresh 0.27.5-generated `dist/csharp/src/GenProfileFfi.cs`
still stamps `[return: MarshalAs(UnmanagedType.I1)]` on the `FfiBuf`-returning `run_username_check`
P/Invoke — byte-identical to 0.27.3. 0.27.4 #622 and 0.27.5 #647 did not touch it. So C# does not
resume: bank the bump, file the proof.

## Built

### M0 — the bump

Five pins moved 0.27.3 → 0.27.5: `setup:boltffi`'s `want`, four `Cargo.toml`s (`bolted-ffi`,
`gen-note-ffi`, `gen-profile-ffi`, `spike-profile-ffi`), and `Cargo.lock` (the seven `boltffi_*`
sub-crates). CLI reinstalled through the askama-symlink workaround. `gen:ffi` produced no diff — our
generator is boltffi-version-independent, as intended.

Every runnable tier green at 0.27.5, counts read from artifacts:

| tier | result | source |
|------|--------|--------|
| `mise run check` | 46 suites, 0 fail | cargo |
| `test:web` | 8, 0 fail | wasm-bindgen |
| `test:csharp` | 14, 0 fail | TRX |
| `test:apple` | 75 (probe) + 20 (app), 0 fail | XCTest — incl. Swift `ProfileConformanceSuite` 33 |
| `test:apple:gen` | 7, 0 fail | XCTest |
| `test:android` | 80, 0 fail | JUnit XML |
| `test:android:gen` | 6, 0 fail | JUnit XML |
| `test:android:app` | 36, 0 fail | JUnit XML |

`test:apple:ui` is **environmentally blocked, not a regression**: it compiled and linked the app
against the regenerated `dist/apple` and launched `BoltedProfileUITests-Runner`, then hit "Timed out
while enabling automation mode" — the documented GUI-session + Accessibility precondition (step-03
report; the task's own comment). The load-bearing part (build/link against regenerated dist) passed.
**The owner should run `mise run test:apple:ui` in an interactive GUI session to close that tier.**

### M1 — the verdict, by the tripwire

`mise run test:csharp` at 0.27.5: 14/14 including the tripwire (which asserts
`Assert.Throws<MarshalDirectiveException>`). Confirmed at the source level in freshly-packed
`dist/csharp` (see upstream draft 06). → **branch B.**

### M4 — the upstream issue kit (`upstream/boltffi/`)

Six drafts re-verified against 0.27.5. **Nothing posted, filed, or sent — the owner files after
review** (their explicit gate; honored in the kit's README, in the step doc's non-goals, and here).

| # | disposition | evidence |
|---|-------------|----------|
| 01 pack-android env | **RETIRED (fixed)** | `nm` red/green control; `test:android` green with the workaround removed |
| 02 use-after-close UB (Kotlin) | **ALIVE** | hazard logcat: `id()` after close stale + aliases another draft |
| 03 bindgen macro blindness | **ALIVE** | `probe.sh` table unchanged (silent drop, exit 0) |
| 04 DTO codec `internal` | **ALIVE** | every generated DTO codec still `internal` |
| 05 `Result<Handle,E>` | **NOT REPRODUCIBLE — do not file** | 4 faithful controls compile at 0.27.3 *and* 0.27.5 |
| 06 C# `MarshalAs(I1)` on `FfiBuf` | **ALIVE (reproduces)** | tripwire green; smoking gun in generated source |

To file (owner's call): **02, 03, 04, 06**.

## The churn log (deliverable 6) — lagging the pin is cheap on the surface, less so on the CLI

`boltffi generate` for all three languages at 0.27.3 vs 0.27.5 (no native build), diffed:

- **Swift bindings: byte-identical.**
- **C# bindings: byte-identical** (which is exactly why the C# bug persists unchanged).
- **Kotlin: one file, `jni/jni_glue.c`, +26 lines** — purely additive `JNI_OnLoad` diagnostics
  (`boltffi_jni_report_*_load_failure` helpers that `fprintf(stderr,…)` + `ExceptionDescribe` on
  class/method-resolution failure). Happy-path-neutral — `test:android` stayed 80/0.

So the *generated surface* barely moved. The real friction of a boltffi bump is elsewhere:

- **`pack android` silently started working** (draft 01) — a behavior change with no surface diff,
  caught only by re-running the tier without the workaround.
- **CLI build reproducibility regressed for the OLD version.** `cargo install boltffi_cli --version
  0.27.3` no longer compiles: `boltffi_cli` floats its sibling `boltffi_bindgen` under a compatible
  range, cargo now resolves it to 0.27.5, and 0.27.5 removed the `render::kmp` symbols 0.27.3's CLI
  imports (E0432). It builds only with `--locked`. **Consequence: our recorded 0.27.3 rollback
  fallback is compromised** — `setup:boltffi` installs without `--locked`, so rolling `want` back to
  0.27.3 today would fail to compile. (Rollback still possible via `--locked`, or a pre-built binary.)

## Deviations (smallest-reversible choices, recorded)

1. **Removed the `pack:android` binding-expansion workaround.** Draft 01 is fixed at 0.27.5 (clean
   red/green `nm` control + `test:android` green without it). Leaving a block commented "BoltFFI
   0.27.3 bug" on a 0.27.5 pin is actively misleading, and the block's own comment said "Drop this
   block when boltffi fixes pack android." Beyond the literal deliverables but evidence-backed and
   reversible (git).
2. **Draft 05 marked do-not-file, contradicting the step-12 report.** The step-12 report (line 61-64)
   states `Result<Handle, E>` "does not compile" at 0.27.3; four faithful controls here compile at
   both 0.27.3 and 0.27.5. Rather than file a non-reproducing bug, the kit records the negative result
   with a minimal repro crate. See open questions.
3. **Commit co-author is Opus, not Fable.** This is an implementation session.

## Friction log

- **The `dist/` tree is gitignored and absent in a fresh worktree**, so there was no local 0.27.3
  baseline to diff. Churn was measured by regenerating at both versions into scratch dirs instead.
- **The shell's working directory persists across tool calls.** A churn-measurement `cd
  crates/gen-profile-ffi` silently left later `ls docs/…` calls resolving against the crate dir and
  "finding" nothing — almost misread as "the drafts don't exist." Absolute paths, or an explicit `cd`
  back to root, avoid it.
- **The draft-05 rabbit hole was expensive.** Five build/control cycles (minimal crate ×2, real crate
  ×2, expansion-cfg ×1, each with a 0.27.3 control and a verified lock) to establish a *negative*.
  Worth it — it stopped a non-reproducing filing — but the lesson from draft 05 (verify the control,
  don't trust the historical claim) is the general one, and it also vindicated re-verifying 01 with
  its own 0.27.3 control rather than trusting the record.
- **`cargo install boltffi_cli 0.27.3` needs `--locked`** (see churn log). Cost ~50s per reinstall;
  three reinstalls this session for the 0.27.3 controls.
- The recurring ambiguous "0 tests … ok" tail of `mise run check`: scanned with an explicit
  exit-code + `grep -c 'test result: FAILED'` each time (the `-i FAILED` match on "0 failed" lines is
  a false positive; `test result: FAILED` is the real signal).

## Kill criteria — none hit

1. *Bump green only by patching `dist/` or bending a contract* — **not hit.** The bump went green
   honestly; nothing in `dist/` was edited, no invariant weakened. (Rollback fallback noted as
   compromised above, but rollback was not needed.)
2. *A new four-feature break at 0.27.5* — **not hit.** Every tier that passed at 0.27.3 passes at
   0.27.5; the C# break is the pre-existing one, unchanged.
3. *(branch A) emitted suite can't honestly cover a C-ID* — **N/A** (branch B; no emitter built).

## Open questions (for a planning/design pass — none are ARCHITECTURE §9)

- **Was step-12 M3's draft-05 compile failure real?** It does not reproduce at the version it was
  reported against. Either the original diagnosis was imprecise (a narrower signature — a generic, or
  the pre-D27 `StashRefused` shape) or it was already resolved. D27's token workaround stands by
  design regardless (stronger parse-don't-validate), so nothing is blocked — but the step-12 report's
  claim is worth a footnote. Not structural.
- **The `pack csharp` seam has no automated "is the driver fixed yet?" beyond the probe.** The
  tripwire is the watch. When it eventually goes red (upstream fixes #06), step 14's M2/M3 (the
  emitted C# contract suite) becomes the resume work — a natural step 16 candidate, still gated on
  that red.
- **`test:apple:ui` cannot run headless in this session context.** Not new (step 03), but it means the
  UI tier's proof at each bump depends on an interactive run by the owner.

## Exit checklist

- [x] All five pins at 0.27.5; `mise run check` and every runnable tier green, counts artifact-derived;
      churn logged. (`test:apple:ui` env-blocked — owner runs it interactively.)
- [x] C# verdict recorded **by the tripwire**; branch B taken accordingly.
- [x] Branch A (emitter/genericity/planted-red) **explicitly not built**, with the tripwire output as
      the reason.
- [x] Issue kit: six dispositions, each retired-with-evidence, alive-with-repro, or do-not-file;
      **nothing posted anywhere**.
- [x] `step-15-report.md` written; ROADMAP row updated; ARCHITECTURE untouched (v1.7 already carries
      this step's design input).
