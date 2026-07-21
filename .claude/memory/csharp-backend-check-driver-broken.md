---
name: csharp-backend-check-driver-broken
description: "BoltFFI C# backend saga — RESOLVED at released 0.28.0 (verified 2026-07-21): MarshalAs bug fixed (out-param) AND finding 07 fixed (#697, distinct per-class stream runtimes); step-23 resume schedulable (~9-site namespace rename + tripwire flip); git-pin machinery obsolete; §6/D26 amendments (v1.7) still law"
metadata: 
  node_type: memory
  type: project
  originSessionId: ddcc2f3b-af09-4980-882e-723913127f3b
  modified: 2026-07-21T13:26:57.686Z
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

**Update (step-15 DONE, branch B, 2026-07-11):** bumped all five pins 0.27.3 → **0.27.5**; every
runnable tier green (`test:apple:ui` env-blocked, not a regression). The tripwire
`TheCheckDriverIsBrokenOnThisBackend` **stayed green — the C# driver is STILL BROKEN at 0.27.5**:
fresh `dist/csharp/src/GenProfileFfi.cs:883-885` still stamps `[return: MarshalAs(UnmanagedType.I1)]`
on the `FfiBuf`-returning `run_username_check` P/Invoke (byte-identical to 0.27.3; contrast `Validate`
line 887, same FfiBuf return, no attr). 0.27.4 #622 / 0.27.5 #647 did NOT touch it. So M2/M3 (the
emitted C# suite) stayed unbuilt — resuming is a future **step 16**, gated on the tripwire going red.
Upstream kit at `upstream/boltffi/` (6 drafts, **nothing posted — owner files**): 01 pack-android env
**FIXED** (workaround removed after an nm red/green control), 02/03/04/06 **alive → to file**, 05
`Result<Handle,E>` **NOT REPRODUCIBLE** at 0.27.3 or 0.27.5 (4 faithful controls all compile) →
do-not-file. Churn tiny: Swift/C# bindings byte-identical, Kotlin only +26 lines JNI_OnLoad
diagnostics. **Gotcha:** `cargo install boltffi_cli --version 0.27.3` no longer compiles (sibling
`boltffi_bindgen` floats to 0.27.5, drops `render::kmp`) — needs `--locked`; `setup:boltffi` uses no
`--locked`, so a plain 0.27.3 rollback would fail. The §6/D26 findings below are **law** (ARCHITECTURE
v1.7). Note: crates.io API needs a User-Agent header or returns a policy error.

**Update (2026-07-19 evening, step 23 — KILLED on KC3):** the git pin at `23cf2ec` was run.
Two-sided verdict: (1) the tripwire went **red for the right reason** — the MarshalAs bug is
dead; the IR backend moved the bool to an `out` param (no return-MarshalAs on the FfiBuf
envelope), and step-14's parked D23 probe would come alive. (2) **#654 also regressed C#
streams**: the IR backend dedupes `#[ffi_stream]` bindings by unqualified method name, so the
store's and draft's same-named `snapshots` collapse into the store's — `draft.Snapshots()`
silently subscribes to the wrong stream (C header + dylib carry both symbols; generated
`NativeMethods` lacks the draft's; Swift green at the same rev; worked on C# at 0.27.5).
Upstream finding **07** drafted (`upstream/boltffi/07-…-same-named-streams.md`, owner files).
Step 23 stopped on KC3; M0's rev-parameterized pin machinery is parked on branch
`step/23-boltffi-git-pin` (resume = one rev-literal change once 07 is fixed upstream). Churn
note for the resume: C# namespace renamed `GenProfileFfi` → `Gen_profile_ffi`, monolith split
into ~35 files, `run_*_check` ABI now out-param. See `docs/steps/step-23-report.md`.

**Earlier update (2026-07-19, design session):** the fix is **on boltffi main** — PR #654 ("Migrate C# to
the new IR backend") merged 2026-07-16 (`53aecd1`). Verified against main's source, not just the
label: `return_marshal_i1` is derived per `ReturnPlan` in
`boltffi_backend/src/target/csharp/render/mod.rs` — `true` only for a direct `Primitive(Bool)`
return, explicitly `false` in the encoded-`FfiBuf` arm; the exact bug shape exists as an upstream
fixture (`check_enabled: Result<bool, LoadError>`) and the C# DemoTest runs throwing async
callbacks e2e. **No release carries it yet** (latest is 0.27.5, cut 2026-07-10). The definitive
local confirmation is the existing tripwire going red→driver-works: bump the pins (or git-pin) and
run `mise run test:csharp`. #657 (Kotlin fun-interface) also merged, so the next release picks up
#663 + #654 + #657 together.

**Update (2026-07-21 — RESOLVED at released 0.28.0, verified against the finding's own
description, not the PR label):** `pack:csharp` at registry 0.28.0 → (1) **finding 07
FIXED** (#697): three distinct stream-runtime classes, two distinct `Snapshots()` overloads,
distinct native `EntryPoint` symbols for store and draft — the collapse is gone;
(2) **MarshalAs FIXED**: `run_username_check` P/Invoke has no return-MarshalAs, bool is an
`out` param (I1 on the param, valid). The probe suite fails to compile on exactly the known
churn (namespace `GenProfileFfi` → `Gen_profile_ffi`, ~9 using/qualified sites across 7
files) — nothing new. Once renamed, `TheCheckDriverIsBrokenOnThisBackend` **will go red by
design** (its doc-comment's revisit condition): delete it, emit real C13/C16 callback tests.
So the step-23 resume = rename + tripwire flip + build M2/M3 (the emitted C# suite); the
rev-parameterized pin machinery on `step/23-boltffi-git-pin` is obsolete (registry release
suffices). Finding-07 upstream filing is moot; the *streaming* issue (Defect-2-led
issue.md in the boltffi checkout) still stands for Henrik to file. Inspection gotcha: use
`target/aarch64-apple-darwin/debug/` (the target-triple path) — plain `target/debug/`
lacks the FFI symbols.

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
