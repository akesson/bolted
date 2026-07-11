---
name: csharp-backend-check-driver-broken
description: BoltFFI 0.27.3 C# backend — run_*_check throws MarshalDirectiveException (wrong return-marshalling); killed step 14; the draft finalizer makes ARCHITECTURE §6 wrong for C#
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

**Lifecycle findings banked for a §6/D26 design pass (ARCHITECTURE left untouched):**
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
