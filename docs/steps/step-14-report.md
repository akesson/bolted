# Step 14 — report: the C# port stopped on kill criterion 1

**Status: stopped at M1 on kill criterion 1 (a four-feature break). M0 and M1 delivered; M2, M3 not
built — and deliberately so, not for lack of effort.**

## Verdict in one paragraph

The C# toolchain seam is real and the packed artifact loads and calls from `dotnet test` on this Mac
(M0, kill criterion 2 cleared). The hand-written probe (M1) ran the step-05 due-diligence on backend
#3 and found that **three of the four load-bearing features run, and one is broken at runtime**: the
async single-flight check driver `run_username_check` throws `MarshalDirectiveException` on every
call, because BoltFFI 0.27.3's C# backend stamps a bool-return marshalling attribute onto a
struct-returning P/Invoke. That is a **four-feature break** — callbacks cannot be exercised end to
end — which is kill criterion 1: *stop, report, upstream draft, do not work around*. The emitted
contract suite (M2) and the genericity/falsification passes (M3) are therefore not built: a
conformance tier whose C13/C16 tests (and whose `fillValid` create-flow path) must drive the check
would be red on this backend, and making it green by skipping the async-check invariants is exactly
the "work around a kill criterion" the rules forbid. Two lifecycle findings for the ARCHITECTURE
§6 / D26 design pass came out of M1 with runtime evidence; they are recorded here, **not** amended
into the frozen documents.

## What was built (and verified green)

### M0 — the toolchain seam + the packed artifact
- `.NET SDK 10.0.301` via mise's `core:dotnet`, **task-scoped** to `pack:csharp`/`test:csharp` (never
  in `[tools]`), so `mise run check` stays Rust-only — the JDK/Gradle precedent from step 05.
- `[targets.csharp] enabled = true` in `gen-profile-ffi/boltffi.toml`. Namespace `GenProfileFfi`,
  `net10.0`, throwing errors, RID `osx-arm64` — all boltffi defaults, all matching the emitted C#.
- `pack:csharp`: builds the host dylib, generates the C# bindings, and runs `dotnet pack` to emit
  `dist/csharp/packages/gen_profile_ffi.0.1.0.nupkg` (the native rides in it under
  `runtimes/osx-arm64/native`, the NuGet convention the runtime graph resolves). Green end to end.
- `test:csharp`: packs first, evicts the fixed-version package from the NuGet cache (0.1.0 never
  changes across re-packs, so a stale copy would shadow a fresh one), runs `dotnet test` with a
  locale-stable console and a TRX as the audit trail.
- `csharp/profile-probe/` consumes the packed package as a real NuGet dependency (the full pack
  distribution path, not a project reference). `SkeletonProbe` pings the native library across the
  FFI boundary — green. **Kill criterion 2 cleared.**

### M1 — the freeze-contract probe (14 tests, green)
The probe records runtime truth; every finding below is a committed, re-runnable test.

| Feature | Verdict | Evidence in the probe |
|---|---|---|
| 1. Classes with methods | **runs** | `ClassHandleProbe`: store/draft handles, `IDisposable`, `using` release, `Dispose` idempotency. |
| 2. Typed errors | **runs** | `TypedErrorProbe`: `PersonNameErrorFfiException` carrying the DU; `SubmitErrorFfiException.Error is SubmitErrorFfi.Validation` with `ErrorData` key+params. |
| 3. Streams | **runs** | `StreamProbe`: `IAsyncEnumerable<ProfileSnapshot>` delivers on mutation; a fresh subscription is future-only. |
| 4. Callback traits | **BROKEN** | `CallbackDriverProbe`: registration works; `run_username_check` throws (see below). |

Plus the lifecycle probes (`LifecycleProbe`), covered in their own section.

## The kill: `run_username_check` cannot be called (kill criterion 1)

**Symptom.** Every call to `draft.RunUsernameCheck()` throws:

```
System.Runtime.InteropServices.MarshalDirectiveException :
  Cannot marshal 'return value': Invalid managed/unmanaged type combination
  (this value type must be paired with Struct).
  at GenProfileFfi.NativeMethods.ProfileDraftFfiRunUsernameCheck(IntPtr self)
```

It throws **with or without a checker set** — proven by calling it on a draft with no checker (which
the contract says returns `Ok(false)` and invokes nothing). So the break is *not* in the callback
path; it is in the P/Invoke itself, and the checker never gets a chance to run.

**Root cause — a BoltFFI 0.27.3 C# codegen bug.** In generated `dist/csharp/src/GenProfileFfi.cs`:

```csharp
[DllImport(LibName, EntryPoint = "boltffi_profile_draft_ffi_run_username_check")]
[return: MarshalAs(UnmanagedType.I1)]              // ← marshalling for a *bool* return
internal static extern FfiBuf ProfileDraftFfiRunUsernameCheck(IntPtr self);  // ← returns a *struct*
```

`run_*_check` is the surface's one `Result<bool, DraftClosed>`-returning verb. Its **wire** return is
the `FfiBuf` envelope (tag byte + bool payload), which the wrapper body reads with a `WireReader`
(`ReadU8()` then `ReadBool()`). The backend confused the Rust return's **logical** `bool` payload with
the **wire** return type and emitted `[return: MarshalAs(UnmanagedType.I1)]` — the attribute it
correctly puts on genuinely-bool-returning exports like `ProfileDraftFfiIsLive`. `MarshalAs(I1)` on a
by-value struct return is invalid C# on **every** .NET runtime (the diligence question "is this a
.NET-10 regression?" is moot — a wrong attribute fails everywhere; confirmed by reading, no need to
install net8/net9). Every other `FfiBuf`-returning verb (`Validate`, `Submit`, `TrySet*`, `Snapshot`,
`Stash`, …) is un-attributed and works; `run_username_check` is the **only** one carrying the bad
attribute, and the only `Result<bool, _>` verb in the surface — the two facts are the same fact.

**It is not fixable from our side.** The defect is in `dist/` bindgen output; kill criterion 5 and
the step's cautions forbid touching it, and emitted code consumes the public generated surface only.
Bumping or patching boltffi is a planning decision, not an implementation-session one, and filing the
upstream draft is an explicit non-goal.

**Blast radius — why it is a *feature* break, not one red test.** The verb is the single driver of
the async check. Without it, unreachable on the C# backend:
- **C13** (verdict is value-bound) and **C16** (an unrun check blocks a dirty checked field) — both
  need `run_username_check` to establish a `Passed`/`Failed` verdict.
- **D10's `[Pending, Passed]`** stream sequence — the driver is the only thing that emits `Pending`.
- A **reentrant callback** during a check — no check can be driven to re-enter from.
- **`fillValid`**, the helper the emitted suite uses to make a create-flow **checked** draft
  committable (C16 demands the pinned check has run): it calls `run_*_check`. So **C12** and **C22**,
  which submit a filled create-flow draft, would also fail — the break reaches past the five checked
  tests into the create-flow ones.

So callbacks — one of the four features VISION risk #1 is about — do not run end to end on backend #3.
That is exactly what kill criterion 1 exists to catch, and its parenthetical predicted the shape of
it: *"the design pass saw all four in the emitted text; only M0/M1 can confirm they run."*

## Where this doc was wrong

The step doc was written after *running the backend's generator and packer*, and it was careful to
mark its runtime claims as guesses. Scored honestly:

- **Wrong (the load-bearing one).** "All four load-bearing BoltFFI features are present,
  idiomatically" — true of the *emitted text* (the `UsernameChecker` interface and vtable bridge are
  idiomatic), but the doc let that stand in for "they run." Feature 4 does not run: its driver throws.
  The lesson the doc itself drew from step 12 (don't let emitted-text presence imply runtime
  behaviour) is the exact lesson it half-missed for callbacks. M1 is where it surfaced, as designed.
- **Right.** "`pack csharp` runs to within one missing tool; the .NET SDK is the single bootstrap
  item" — exactly right; `mise install` provided it and pack went green with no other change.
- **Right.** "Use-after-dispose is a typed refusal, not UB; step 05's H2 may not exist on this
  backend" — confirmed dead (see below).
- **Right.** "`ProfileDraftFfi` has a finalizer; if it reaches the store-side close, §6's C# row is
  wrong" — confirmed: it does reach the close (see below).

## Lifecycle findings — input to the ARCHITECTURE §6 / D26 design pass

Recorded with runtime evidence. **Not** amended into ARCHITECTURE §6 or D26 — that is a design
session's job, per the step doc and the working agreement.

1. **§6's C# row is wrong: the GC *does* free the Rust draft.** `ProfileDraftFfi` carries a finalizer
   (`~ProfileDraftFfi() => Dispose()`). An abandoned, undisposed, entity-backed draft is reclaimed by
   the GC, and its finalizer reaches the store-side close: `LiveDraftCount()` falls from 2 to 1 after
   a forced collection, while a still-referenced control draft is untouched (proving the GC was
   selective, per the ART GC-probe lesson that a probe without a control measures nothing). So §6's
   "Kotlin / C#: `close()` only, the GC never frees the Rust draft" and its "forgetting it leaks … in
   every language" **overclaim for C#**. This is **D26's recorded revisit condition met verbatim**:
   "if upstream grows an opt-in Cleaner *inside bindgen*, where the CAS makes it safe, revisit." It
   grew one (a finalizer + `Interlocked.Exchange`-guarded idempotent `Dispose`), on this backend.
   `LifecycleProbe.AForgottenDraftIsReclaimedByTheFinalizer_Section6IsWrongForCSharp`.
   - Nuance for the design pass: this makes a *forgotten* handle non-deterministically safe, not
     safe. D26's warning stands — under a finalizer, a forgotten `Dispose` passes every test that
     never provokes a collection. The D26 leak-freedom contract is therefore written to assert the
     baseline count **immediately after deterministic `Dispose`, before any GC** — so a finalizer
     cannot be what greens it. `LifecycleProbe.DeterministicDisposeReturnsCountsToBaseline_NoGCInvolved`.

2. **H2 looks dead on C# (via the Dispose path).** Step 05's H2 was a dangling-pointer dereference —
   silent UB — reachable because a released Kotlin handle still pointed at freed Rust. On C#,
   `Dispose` zeroes the handle under `Interlocked.Exchange`, and every verb calls `ThrowIfDisposed`
   first, so use-after-dispose is `ObjectDisposedException` — a **typed refusal before any native
   call**, on both the store and the draft, for every verb tried.
   `ClassHandleProbe.UseAfterDisposeIsTyped_NotUB`. Scope, stated honestly: this closes the
   *dispose-then-use* hazard. It does not speak to a hypothetical raw-pointer path that skips the
   guard; the generated wrapper has no such path. **Upstream-filing implication:** the step-05 H2
   filing's scope narrows on C# — the silent-UB variant is not reachable through the generated
   surface here; the guard makes it a typed error.

3. **D23 holds for the verbs that can be reached.** On a submitted (released) draft: a setter refuses
   via its value-error DU's `DraftClosed` case (`PersonNameErrorFfiException.Error is
   PersonNameErrorFfi.DraftClosed`); `resolve_*` refuses via `DraftClosedFfiException`; a second
   `submit` is `SubmitErrorFfi.AlreadySubmitted`; observers (`Snapshot`, `Validate`) stay total (no
   throw). `Assert.Throws` is itself the step-11 positive control — a swallowed no-op fails it.
   `LifecycleProbe.D23_MutatorsOnAReleasedDraftAreTypedRefusals_ObserversTotal`.
   - One casualty of the codegen bug: `run_username_check`'s *own* D23 refusal is unobservable,
     because it throws `MarshalDirectiveException` before it can return the `DraftClosed` envelope.
     Recorded, not asserted as a contract.

## Upstream draft (BoltFFI) — not filed (non-goal), drafted here

> **Title:** C# backend emits `[return: MarshalAs(UnmanagedType.I1)]` on struct-returning
> `run_*_check` P/Invoke → `MarshalDirectiveException` at runtime.
>
> **Version:** boltffi 0.27.3. **Target:** `csharp`, `net10.0`, `osx-arm64` (the attribute is
> platform- and TFM-independent).
>
> **Symptom:** any `Result<bool, E>`-returning `#[export]` verb (here `draft.run_username_check`)
> generates a P/Invoke declared `static extern FfiBuf …` — correct, the wire return is the `FfiBuf`
> envelope — but decorated with `[return: MarshalAs(UnmanagedType.I1)]`. Calling it throws
> `MarshalDirectiveException: Cannot marshal 'return value': Invalid managed/unmanaged type
> combination (this value type must be paired with Struct)`.
>
> **Cause:** the return-marshalling attribute is being chosen from the Rust return type's *payload*
> (`bool`) rather than the *wire* type (`FfiBuf`). The correct output is no `[return: MarshalAs]` at
> all, matching every other `FfiBuf`-returning verb (`submit`, `validate`, `try_set_*`), whose
> generated declarations are un-attributed and work.
>
> **Fix sketch:** apply `[return: MarshalAs(UnmanagedType.I1)]` only when the emitted P/Invoke return
> type *is* `bool` (as for `is_live`), never when it is `FfiBuf`.
>
> **Minimal repro:** `boltffi pack csharp` on any feature with a `Result<bool, _>` verb; call it from
> `dotnet test`. `gen-profile-ffi`'s `run_username_check` is one.

## Friction log

1. **`dotnet` localises its console to the system language** (this box: Spanish). `test:csharp` sets
   `DOTNET_CLI_UI_LANGUAGE=en` so the summary is legible, and writes a TRX as the real source of
   truth for counts — the step-13 "trust the artifact, not the wrapper" caution, honoured up front.
2. **The NuGet global cache shadows a re-packed fixed-version package.** `gen_profile_ffi` is always
   `0.1.0`, so a second `pack` does not change the version and `dotnet restore` reuses the cached
   copy. `test:csharp` evicts just our package (`rm -rf $NUGET_PACKAGES/gen_profile_ffi`) before
   restoring; NUnit/test-SDK stay cached, so it is cheap and hermetic.
3. **The tier's failure mode was not proven** (M3's job, not reached): I did not run a planted-red to
   confirm `dotnet test` propagates a nonzero exit and where counts read from. From the seam and probe
   runs, `dotnet test` exits nonzero on failure and the TRX is authoritative — but this is *not* the
   deliberately-planted proof M3 would have banked, and a future step must bank it before quoting a
   C# pass count as evidence, exactly as `test:android`'s exit-code lie taught.

## What M2 / M3 would need to resume (for the next planning pass)

Everything is in place except a working check driver:
- The **emitter model** is fully mapped (this session read the whole generated C# surface): C# is
  `readonly record struct` DTOs, `abstract record` + nested `sealed record` for discriminated unions
  (`TextValidity.Valid`, `TextFieldSync.Conflicted`, `SubmitErrorFfi.*`), PascalCase members, `with`
  expressions for value copies, `enum` field-ids, and typed `<Value>ErrorFfiException` wrappers
  carrying an `.Error` DU. NUnit's `Assert.Throws`/`Assume.That` are the JUnit-`Assume`/`XCTSkip`
  analogues. A `csharp_contract_suite` emitter on the step-13 marker-substitution model is a
  mechanical port — **once the driver works**.
- The **`kotlin_drift` → `foreign_drift`** rename (step-13's recorded cleanup) was **not** landed,
  since M2 was not built. It waits for the same future step.
- `BOUNDARY_MAP` and `CONFORMANCE.md` are **unchanged** — no ad-hoc edits. If C# ever ships, C13/C16
  remain `emitted` there; the map is language-neutral and this backend's bug is not a boundary fact.

## Open questions for a design session

1. **Does C# belong on the ladder before boltffi fixes the driver?** The seam is proven and 3.5/4
   features run, but the async-check story — the thing that most distinguishes Bolted — cannot be
   demonstrated on C# today. Options: (a) pin a patched/newer boltffi and resume M2/M3; (b) shelve
   C# until upstream fixes it; (c) proceed with a *documented* partial suite that omits C13/C16/D10
   (rejected here as working around a kill criterion, but a design pass may weigh it differently).
2. **§6 / D26 amendment.** The C# finalizer is real and reaches the store-side close. Does §6 grow a
   per-backend row ("C#: GC *may* free via a finalizer; still non-deterministic, `Dispose` is the
   contract"), and does D26's leak-freedom contract for C# codify "assert before any GC"? Both need a
   design decision; the evidence is banked in `LifecycleProbe`.
3. **H2 filing scope.** With use-after-dispose typed on C#, the step-05 H2 upstream filing is narrower
   than "silent UB in every GC language." Worth re-scoping when it is filed.

## Exit checklist

- [x] `pack:csharp` / `test:csharp` exist, task-scoped dotnet only; `mise run check` unchanged in its
      dependencies and green (46 suites, 0 failures, exit 0).
- [x] The probe answers, with runtime evidence: four features (3 run, 1 broken) · finalizer reaches
      store-side close · use-after-dispose typed (H2 dead) · D23 controls · D26-shaped leak test.
      `[Pending, Passed]` and reentrancy are **blocked by the driver bug**, recorded, not faked.
- [ ] The emitted C# suite — **not built (kill criterion 1)**; `foreign_drift` rename **not landed**.
- [ ] Genericity golden / falsification — **not built (kill criterion 1)**; the tier's failure mode is
      **not** proven (friction 3).
- [x] CONFORMANCE.md and `BOUNDARY_MAP` **untouched** — no ad-hoc edits; the boundary story is
      unchanged, and a divergence would be a report finding, which this is.
- [x] Report written (this file); ROADMAP updated; ARCHITECTURE untouched.
