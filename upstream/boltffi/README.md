# BoltFFI upstream issue kit — re-verified at 0.27.5

**These are DRAFTS for the owner to file. Nothing here has been posted, filed, or sent anywhere —
no `gh issue create`, no pull request, no comment, no API write. Filing is the owner's action,
after review.** (Step 15, deliverable 5; the owner's explicit gate.)

Step 12 drafted five findings against boltffi 0.27.3; step 14 added a sixth (the C# check-driver
marshalling bug). BoltFFI then shipped 0.27.4 (2026-07-09) and 0.27.5 (2026-07-10). Step 15 bumped
the pin to 0.27.5 and **re-verified every draft against it**. Each draft below carries a
`## Re-verification at 0.27.5` section with the disposition and the evidence.

## Dispositions

| # | Title | at 0.27.3 | **at 0.27.5** | Repro / evidence |
|---|-------|-----------|---------------|------------------|
| 01 | `pack android` omits binding-expansion env → undefined JNI symbols | broken | **RETIRED — fixed** | `nm` red/green control; `test:android` green with no workaround |
| 02 | Generated methods ignore `__boltffi_closed` → Kotlin use-after-close UB | UB | **ALIVE** | `test:android:hazard` logcat: `id()` after close returns stale/aliased silently |
| 03 | bindgen silently ignores macro-generated FFI items | silent drop, exit 0 | **ALIVE** | `step-10-boltffi-visibility/probe.sh` — table unchanged |
| 04 | DTO wire ser/de is `internal` — unreachable from a shell | internal | **ALIVE** | every generated DTO codec still `internal` |
| 05 | A throwing method cannot return a class handle | reported broken | **NOT REPRODUCIBLE — do not file** | 4 faithful controls all compile at 0.27.3 **and** 0.27.5 |
| 06 | C# `[MarshalAs(I1)]` on an `FfiBuf` return → `run_*_check` throws | broken | **ALIVE (reproduces at 0.27.5)** | tripwire green; fresh generated source |

**To file (owner's call): 02, 03, 04, 06.** Retired with evidence: **01**. Do **not** file: **05**
(cannot be reproduced at the version it was reported against — filing it would waste maintainer time).

## What the bump itself proved (context for filing)

- All five library pins moved 0.27.3 → 0.27.5; every runnable tier stayed green (`check`, `test:web`,
  `test:apple`/`:gen`, `test:android`/`:gen`/`:app`, `test:csharp`). `test:apple:ui` is
  environmentally gated (GUI session), not a regression.
- Generated-surface churn 0.27.3 → 0.27.5 is tiny: Swift and C# bindings **byte-identical**; Kotlin
  changed only `jni/jni_glue.c` (+26 lines of additive `JNI_OnLoad` diagnostics). See the step-15 report.
- CLI reproducibility caveat: `cargo install boltffi_cli --version 0.27.3` no longer builds
  (its floated sibling `boltffi_bindgen` moved to 0.27.5 and dropped `render::kmp` symbols); it needs
  `--locked`. A 0.27.3 rollback via `setup:boltffi` (no `--locked`) would fail to compile.
