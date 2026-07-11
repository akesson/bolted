---
name: csharp-backend-check-driver-broken
description: BoltFFI 0.27.3 C# backend — run_*_check throws MarshalDirectiveException (wrong return-marshalling); killed step 14; §6/D26 amended (v1.7); step 15 (ready) bumps to 0.27.5 and lets the probe tripwire decide resume-or-file
metadata:
  type: project
---

Step 14 (the C# port) **stopped on kill criterion 1**: on BoltFFI 0.27.3's C# backend, three of the
four load-bearing features run, but **callbacks are broken at runtime**. The async single-flight check
driver `run_username_check` throws `MarshalDirectiveException: Cannot marshal 'return value'` on
**every** call (with or without a checker set), so the checker is never invoked.

**Root cause (a boltffi C# codegen bug, in generated `dist/` — kill criterion 5, unfixable from our
side):** `run_*_check` is the surface's one `Result<bool, _>`-returning verb. Its *wire* return is the
`FfiBuf` envelope, but the backend tags the P/Invoke with `[return: MarshalAs(UnmanagedType.I1)]` —
the marshalling for a *bool* return (correct on `is_live`, wrong here). `MarshalAs(I1)` on a struct
return is invalid C# on every .NET runtime, so it is not a net10 quirk. Fix is upstream-only: don't
emit the I1 attribute unless the P/Invoke return type *is* `bool`.

**Blast radius:** C13, C16, D10's `[Pending, Passed]`, reentrant-checker, and `fillValid`'s create-flow
check (so C12/C22 too). An emitted conformance suite cannot honestly skip these to go green — which is
why step 14's M2 (emitter) + M3 (genericity/falsification) were **not built**. M0 (toolchain seam,
`pack:csharp`/`test:csharp`, packed artifact loads/calls) and M1 (probe, 14 tests) **are** done and
green. Resuming needs an upstream fix or a pinned/patched boltffi.

**Update (step-15 planning, 2026-07-11):** upstream shipped **0.27.4 (Jul 9) and 0.27.5 (Jul 10)**.
No release note names this bug and nobody has filed it (tracker searched), but 0.27.4's #622 fixed
the same class of defect (OptionScalar f64/FfiBuf signature confusion) and 0.27.5's #647
(`Result<Class,E>` lowered as handle) plausibly retires step-12 upstream draft 05. **Step 15**
(`docs/steps/step-15-boltffi-bump.md`, ready) bumps the five pins to 0.27.5 and lets the probe's
tripwire test decide: red → driver fixed → resume M2/M3; green → still broken → bank the bump and
finalize the upstream issue kit (all six drafts re-verified, repro skeletons, **owner files — never
post from a session**). The §6/D26 findings below are now **law**: ARCHITECTURE v1.7 amended §4/§6
(per-backend release table; "GC never frees" is Kotlin-only) and D26 (revisit condition met; leak
test must assert baseline before any GC). Note: crates.io API requires a User-Agent header or it
returns a policy error.

**Lifecycle findings that drove the v1.7 amendment (banked in step 14, amended in step 15's planning
pass):**
- **§6 is WRONG for C#.** `ProfileDraftFfi` has a finalizer (`~ProfileDraftFfi() => Dispose()`), so a
  forgotten, undisposed draft is GC-reclaimed and its finalizer reaches the store-side close (live
  count falls; proven with a still-referenced control draft). This is **D26's recorded revisit
  condition met** ("a Cleaner inside bindgen, where the CAS makes it safe"). D26's leak-freedom test
  must therefore assert the baseline **before any GC**, so a finalizer can't green a forgotten Dispose.
- **H2 looks DEAD on C#.** Use-after-dispose is `ObjectDisposedException` (a typed refusal before any
  native call), not step-05's silent UB — the step-05 H2 upstream filing narrows on C#.

Evidence lives in `csharp/profile-probe/` (committed, green) and `docs/steps/step-14-report.md`.
Related: [[boltffi-bindgen-reads-source-text]], [[the-core-ships-no-lock]],
[[art-gc-probes-need-a-control]] (the control-draft technique used in the finalizer probe).
