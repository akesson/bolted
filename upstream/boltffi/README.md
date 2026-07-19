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
| 06 | C# `[MarshalAs(I1)]` on an `FfiBuf` return → `run_*_check` throws | ALIVE | PR [#662](https://github.com/boltffi/boltffi/pull/662) | **FIXED ON MAIN** — #654 merged 2026-07-16 (`53aecd1`). Verified 2026-07-19 against main's source: `return_marshal_i1` is derived per `ReturnPlan` — `true` only for a direct `Primitive(Bool)` return, explicitly `false` for encoded `FfiBuf` returns (`boltffi_backend/src/target/csharp/render/mod.rs`); the exact bug shape is a covered fixture (`check_enabled: Result<bool, LoadError>` in `tests/fixtures/source/callback/async_callback_return_shapes.rs`) and the C# DemoTest exercises throwing async callbacks e2e. **Not in any release yet** (latest 0.27.5 predates the merge) — our tripwire (`mise run test:csharp`) confirms on the next pin bump |

Also filed, beyond the drafts:

- PR [#657](https://github.com/boltffi/boltffi/pull/657) — emit `fun interface` for single-method
  Kotlin callbacks. Open, **approved** (engali94), awaiting merge.
- RFC [#665](https://github.com/boltffi/boltffi/issues/665) — per-invocation metadata capture,
  retiring the source re-scan inside the metadata build. Open; this is the root-cause umbrella
  over draft 03 and the cfg-eval family (#630/#618).

**Watch list (updated 2026-07-19):** #654 **merged** 2026-07-16 and #657 **merged** 2026-07-15 —
the next release after 0.27.5 now picks up all three merged fixes (#663, #654/06, #657); watching
for that release (the C# resume rides it, or a git pin if it drags). Still open: maintainer
response on #664/#665/#666. Remaining TODO on our side: the standalone
macro-items repro issue promised in #665 for draft 03.

## What the bump itself proved (context for filing)

- All five library pins moved 0.27.3 → 0.27.5; every runnable tier stayed green (`check`, `test:web`,
  `test:apple`/`:gen`, `test:android`/`:gen`/`:app`, `test:csharp`). `test:apple:ui` is
  environmentally gated (GUI session), not a regression.
- Generated-surface churn 0.27.3 → 0.27.5 is tiny: Swift and C# bindings **byte-identical**; Kotlin
  changed only `jni/jni_glue.c` (+26 lines of additive `JNI_OnLoad` diagnostics). See the step-15 report.
- CLI reproducibility caveat: `cargo install boltffi_cli --version 0.27.3` no longer builds
  (its floated sibling `boltffi_bindgen` moved to 0.27.5 and dropped `render::kmp` symbols); it needs
  `--locked`. A 0.27.3 rollback via `setup:boltffi` (no `--locked`) would fail to compile.
