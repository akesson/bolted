# BoltFFI upstream issue kit — filed upstream (status as of 2026-07-19)

Step 12 drafted five findings against boltffi 0.27.3; step 14 added a sixth (the C# check-driver
marshalling bug). Step 15 bumped the pin to 0.27.5 and re-verified every draft against it (each
draft carries a `## Re-verification at 0.27.5` section with the evidence). **The owner has since
filed the surviving findings on boltffi/boltffi** — as fix PRs where we had the fix, as issues
where the design is upstream's call. This README now tracks upstream status.

## Upstream status

| # | Title | at 0.27.5 | Upstream | Status (2026-07-15) |
|---|-------|-----------|----------|---------------------|
| 01 | `pack android` omits binding-expansion env → undefined JNI symbols | RETIRED — fixed | not filed | closed here with evidence (`nm` red/green; `test:android` green) |
| 02 | Generated methods ignore `__boltffi_closed` → Kotlin use-after-close UB | ALIVE | PR [#663](https://github.com/boltffi/boltffi/pull/663) + issue [#664](https://github.com/boltffi/boltffi/issues/664) | **#663 MERGED** 2026-07-14 (guards JVM handle reads; on `main`, not yet in a release — latest is 0.27.5). Residual concurrent close-race filed as #664 (open; prior-art comment added) |
| 03 | bindgen silently ignores macro-generated FFI items | ALIVE | RFC [#665](https://github.com/boltffi/boltffi/issues/665) | folded into the RFC's source-re-scan bug family (listed there as "unfiled, repro to follow"); **standalone repro issue still to file** |
| 04 | DTO wire ser/de is `internal` — unreachable from a shell | ALIVE | issue [#666](https://github.com/boltffi/boltffi/issues/666) | open, no maintainer response yet. As-filed text: `04-issue.md` |
| 05 | A throwing method cannot return a class handle | NOT REPRODUCIBLE | not filed | correctly withheld — 4 faithful controls compile at 0.27.3 and 0.27.5 |
| 06 | C# `[MarshalAs(I1)]` on an `FfiBuf` return → `run_*_check` throws | ALIVE | PR [#662](https://github.com/boltffi/boltffi/pull/662) | **FIXED — verified locally** (step 23 M1, 2026-07-19): at git rev `23cf2ec` the tripwire went **red for the right reason** (`Expected: <MarshalDirectiveException> But was: null` — `run_username_check` now returns its bool via an `out` param, no return-MarshalAs; the attribute survives only on the genuinely-bool `is_live`). Source-level confirmation: `return_marshal_i1` derived per `ReturnPlan` in `boltffi_backend/src/target/csharp/render/mod.rs`; fixture `check_enabled: Result<bool, LoadError>` upstream. **Not in any release yet** — and the same PR introduced finding 07, so the pin was killed anyway (step-23 report) |
| 07 | C# IR backend collapses same-named `#[ffi_stream]` methods across classes — draft stream silently lost | NEW (regression at `23cf2ec`, introduced by #654) | not filed — **owner files** | Found by step 23 M1 (KC3 kill): `draft.Snapshots()` routes to the *store's* stream runtime; C header + dylib export both symbols, generated C# `NativeMethods` lacks the draft's. Worked at 0.27.5; Swift green at the same rev (C#-only). Draft + repro sketch: `07-csharp-ir-backend-collapses-same-named-streams.md` |

Also filed, beyond the drafts:

- PR [#657](https://github.com/boltffi/boltffi/pull/657) — emit `fun interface` for single-method
  Kotlin callbacks. Open, **approved** (engali94), awaiting merge.
- RFC [#665](https://github.com/boltffi/boltffi/issues/665) — per-invocation metadata capture,
  retiring the source re-scan inside the metadata build. Open; this is the root-cause umbrella
  over draft 03 and the cfg-eval family (#630/#618).

**Watch list (updated 2026-07-19, post step-23 kill):** the git-pin attempt at `23cf2ec` was
**killed on step-23 KC3** — #654 fixes 06 but regresses streams (finding 07), so the C# resume
now waits on an upstream fix for 07 (owner to file it first), then a new rev or the release
carrying both. Do NOT re-pin main without confirming 07 is fixed — the step-23 M0 machinery
(rev-parameterized setup:boltffi + doctor rev cross-pin, parked on branch
`step/23-boltffi-git-pin`) reuses with only the rev literal changed. Still open: maintainer
response on #664/#665/#666; merged and waiting on a release: #663, #654, #657, #693. Remaining
TODO on our side: the standalone macro-items repro issue promised in #665 for draft 03.

## What the bump itself proved (context for filing)

- All five library pins moved 0.27.3 → 0.27.5; every runnable tier stayed green (`check`, `test:web`,
  `test:apple`/`:gen`, `test:android`/`:gen`/`:app`, `test:csharp`). `test:apple:ui` is
  environmentally gated (GUI session), not a regression.
- Generated-surface churn 0.27.3 → 0.27.5 is tiny: Swift and C# bindings **byte-identical**; Kotlin
  changed only `jni/jni_glue.c` (+26 lines of additive `JNI_OnLoad` diagnostics). See the step-15 report.
- CLI reproducibility caveat: `cargo install boltffi_cli --version 0.27.3` no longer builds
  (its floated sibling `boltffi_bindgen` moved to 0.27.5 and dropped `render::kmp` symbols); it needs
  `--locked`. A 0.27.3 rollback via `setup:boltffi` (no `--locked`) would fail to compile.
