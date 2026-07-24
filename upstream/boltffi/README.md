# BoltFFI upstream issue kit — filed upstream (status as of 2026-07-24)

Step 12 drafted five findings against boltffi 0.27.3; step 14 added a sixth (the C# check-driver
marshalling bug). Step 15 bumped the pin to 0.27.5 and re-verified every draft against it (each
draft carries a `## Re-verification at 0.27.5` section with the evidence). **The owner has since
filed the surviving findings on boltffi/boltffi** — as fix PRs where we had the fix, as issues
where the design is upstream's call. This README now tracks upstream status.

**2026-07-24 (step 29 M3):** the workspace rides **registry 0.28.0**. Both C#-blocking findings
are now **fixed in a release** and re-verified by execution: **06** (the MarshalAs check-driver
bug) and **07** (the IR-backend same-named-stream collapse). #663 (finding 02) is confirmed
shipped in 0.28.0. The step-23 git-pin machinery is obsolete. See the flips below and the
rewritten watch list.

## Upstream status

| # | Title | at 0.27.5 | Upstream | Status (2026-07-24) |
|---|-------|-----------|----------|---------------------|
| 01 | `pack android` omits binding-expansion env → undefined JNI symbols | RETIRED — fixed | not filed | closed here with evidence (`nm` red/green; `test:android` green) |
| 02 | Generated methods ignore `__boltffi_closed` → Kotlin use-after-close UB | ALIVE | PR [#663](https://github.com/boltffi/boltffi/pull/663) + issue [#664](https://github.com/boltffi/boltffi/issues/664) | **#663 SHIPPED in released 0.28.0** (guards JVM handle reads). Confirmed read-only 2026-07-24: the #663 merge commit `2de4597` (merged 2026-07-14) is an ancestor of the `v0.28.0` tag — GitHub compare `v0.28.0...2de4597` = 0 ahead / 74 behind (in), and `v0.27.5...2de4597` = ahead (so it missed 0.27.5). Residual concurrent close-race filed as #664 (open; prior-art comment added) |
| 03 | bindgen silently ignores macro-generated FFI items | ALIVE | RFC [#665](https://github.com/boltffi/boltffi/issues/665) | folded into the RFC's source-re-scan bug family (listed there as "unfiled, repro to follow"); **standalone repro issue still to file (Henrik files)** |
| 04 | DTO wire ser/de is `internal` — unreachable from a shell | ALIVE | issue [#666](https://github.com/boltffi/boltffi/issues/666) | open, no maintainer response yet. As-filed text: `04-issue.md` |
| 05 | A throwing method cannot return a class handle | NOT REPRODUCIBLE | not filed | correctly withheld — 4 faithful controls compile at 0.27.3 and 0.27.5 |
| 06 | C# `[MarshalAs(I1)]` on an `FfiBuf` return → `run_*_check` throws | **FIXED IN RELEASE 0.28.0** | PR [#662](https://github.com/boltffi/boltffi/pull/662) (closed w/o merge; fix rode the #654 IR rewrite) | **FIXED — verified by execution at released 0.28.0** (step 29 M0, 2026-07-24): the step-14 tripwire `TheCheckDriverIsBrokenOnThisBackend` went **red for the right reason** (`Expected: <MarshalDirectiveException> But was: null`) — `run_username_check` returns its bool via an `out` param, so `[MarshalAs(I1)]` survives only on genuinely-bool members. Tripwire then **deleted** per its designed end state; the parked probes came alive and are green (D23 `DraftClosed` refusal after close, D10 `[Pending, Passed]` verdict stream, reentrant checker no-deadlock, `fillValid` create-flow check). `test:csharp` 20/20 (TRX). Dated addendum in `06-…md` |
| 07 | C# IR backend collapses same-named `#[ffi_stream]` methods across classes — draft stream silently lost | **FIXED IN RELEASE 0.28.0** | upstream [#697](https://github.com/boltffi/boltffi/pull/697) (three distinct stream-runtime classes, distinct native `EntryPoint` symbols) | **FIXED — re-verified by execution at released 0.28.0** (step 29 M0, 2026-07-24): the two draft-stream `StreamProbe` rows that timed out at git rev `23cf2ec` are **green**, and `draft.Snapshots()` demonstrably routes to the draft's *own* subscription (distinct `Snapshots()` overloads / native EntryPoints). Never filed (owner-files rule) — filing is now **moot** (upstream fixed it independently via #697 before we could). Dated addendum in `07-…md` |
| 08 | bindgen evaluates no `#[cfg]` — gated items join every target's surface | ALIVE (source-verified at 0.28.0, **runtime-probed 2026-07-23**) | not filed — likely the #630/#618 cfg-eval family under RFC #665; check before duplicating | **Runtime-probe TODO DONE** (step-27 M0, 2026-07-23): the probe **confirmed the union claim** — a `#[cfg(target_os = "ios")]`-gated `#[data]` struct packed for android landed in the Kotlin bindings as a real `data class`, `generate` exiting 0. Still **ALIVE / unfiled** (still likely the #630/#618 cfg-eval family under RFC #665; check before duplicating). `boltffi_scan::ActiveCfg` is a complete but unwired cfg evaluator. This is what forces the per-platform http bridge-crate split. Draft: `08-…md` |

Also filed, beyond the drafts:

- PR [#657](https://github.com/boltffi/boltffi/pull/657) — emit `fun interface` for single-method
  Kotlin callbacks. Open, **approved** (engali94), awaiting merge.
- RFC [#665](https://github.com/boltffi/boltffi/issues/665) — per-invocation metadata capture,
  retiring the source re-scan inside the metadata build. Open; this is the root-cause umbrella
  over draft 03 and the cfg-eval family (#630/#618).

**Watch list (rewritten 2026-07-24, post step-29):** the workspace rides **registry 0.28.0**;
both C#-blocking findings (06, 07) are **fixed in that release** and re-verified by execution
(step-29 M0), so the C# resume is unblocked and shipped (steps 29 M0–M2).

- **Obsolete — the git-pin machinery.** The step-23 M0 apparatus (rev-parameterized
  `setup:boltffi` + doctor rev cross-pin) and the parked branch `step/23-boltffi-git-pin`
  are **superseded** by the move to registry 0.28.0 — there is nothing left to pin at a rev.
  The old "Do NOT re-pin main" guidance is moot and has been removed. Deleting the parked
  branch is **Henrik's call** (not done here).
- **Still open — maintainer response pending:** #664 (concurrent close-race, finding 02
  residual), #665 (the per-invocation-metadata RFC / source-re-scan umbrella), #666 (finding
  04, DTO wire ser/de visibility).
- **#657** (emit Kotlin `fun interface` for single-method callbacks) — open, **approved**
  (engali94), awaiting merge; not yet observed in a release.
- **Our-side TODO — Henrik files, nothing posted by anyone else, ever:** the standalone
  macro-items repro issue promised in #665 for **draft 03**; and the **Defect-2 streaming**
  filings from the bolted-http legs — the `ffi_stream` overflow-drop (F-M0-4) and the
  abandoned-subscription leak (F-M3-1 / F-M0-5) — remain real upstream defects even though
  the shipped contract path no longer exercises them (step-27 report §Open questions).

## What the bump itself proved (context for filing)

- All five library pins moved 0.27.3 → 0.27.5; every runnable tier stayed green (`check`, `test:web`,
  `test:apple`/`:gen`, `test:android`/`:gen`/`:app`, `test:csharp`). `test:apple:ui` is
  environmentally gated (GUI session), not a regression.
- Generated-surface churn 0.27.3 → 0.27.5 is tiny: Swift and C# bindings **byte-identical**; Kotlin
  changed only `jni/jni_glue.c` (+26 lines of additive `JNI_OnLoad` diagnostics). See the step-15 report.
- CLI reproducibility caveat: `cargo install boltffi_cli --version 0.27.3` no longer builds
  (its floated sibling `boltffi_bindgen` moved to 0.27.5 and dropped `render::kmp` symbols); it needs
  `--locked`. A 0.27.3 rollback via `setup:boltffi` (no `--locked`) would fail to compile.
