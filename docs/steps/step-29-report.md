# Step 29 — Report: S-WIN part I, the C# resume at released 0.28.0

**Status: done.** Third attempt at the C# leg, and the first with the ground verified solid.
All four milestones shipped; **no kill criteria hit** (all five checked below); **zero changes
to any frozen crate** — the emitter and golden work landed in `bolted-ffi-gen`, the suite and
fixture under `csharp/profile-probe/`, and the drift check in `gen-profile-ffi/tests`.
Implementation ran as four Opus sub-agent milestones (M0–M3) with orchestrator review, a
commit between each, and `mise run check` re-run on the branch after each. Both step-14/23
blockers are now **fixed in released registry 0.28.0** and re-verified **by execution** (not by
PR label). Final tier: `test:csharp` **53/53** (TRX-derived: 20 hand-written probes + 33
emitted rows over 22 C-IDs); `mise run check` green.

## Built

### M0 — the rename + the verdict (commit `cfbc200`, deliverable 1)

- **The 9-site `GenProfileFfi` → `Gen_profile_ffi` rename** across 7 probe files under
  `csharp/profile-probe/` — the IR backend names the namespace after the raw crate name. The
  probe suite failed to compile on exactly this churn and nothing else.
- **Finding 07 verified by execution first.** The two `StreamProbe` draft-stream rows that
  **timed out** at the step-23 git pin (`23cf2ec`) are **green** at 0.28.0; the generated
  surface now carries three distinct stream-runtime classes and two distinct `Snapshots()`
  overloads routing to draft-own vs store subscriptions with distinct native EntryPoints —
  #697 confirmed in our shape. (Kill criterion 2 was the risk here; it did not fire.)
- **The tripwire went red for the right reason, then was deleted.** The step-14
  `TheCheckDriverIsBrokenOnThisBackend` asserts the *broken* behaviour; at 0.28.0 it went red
  with `Expected: <MarshalDirectiveException> But was: null` — the MarshalAs(I1) moved onto
  `run_username_check`'s out-bool param, so the driver now works. Deleted per its designed end
  state.
- **The parked probes came alive and are green:** D23's `DraftClosed` refusal on
  `run_username_check` after close; D10's `[Pending, Passed]` verdict stream for an accepted
  value; the reentrant checker no-deadlock shape; `fillValid`'s create-flow check (a filled
  create-flow draft is committable only after the pinned username check runs, C16). Each new
  test watched red first (one wrong-expected per test), then green.
- **Contract established:** `run_username_check`'s returned bool means **"a check RAN"**, not
  the verdict — both Pass and Fail return `true`; the verdict is read from
  `Snapshot().UsernameCheck`. (`Ok(false)` is the declared-absent capability.) The Kotlin and
  Swift suites were checked for propagation and **already encoded this correctly** — no wrong
  contract propagated.
- `test:csharp` 20/20 (TRX); `mise run check` green.

### M1 — the emitted C# contract suite (commit `6be9092`, deliverable 2)

- **`emit_csharp_contract_suite`** in `crates/bolted-ffi-gen/src/foreign.rs` (+815, pure
  addition): the third foreign contract-suite emitter, on the same D28 marker-substitution
  model as the Kotlin/Swift ones (plain Rust string-building, `@@MARKER@@` substitution, no
  template engine, provenance headers). The 22 emitted C-IDs (C10 stays exempt via
  `BOUNDARY_MAP`) projected through the public C# surface: `readonly record struct` DTOs by
  value equality, `abstract record` DUs via `Is.InstanceOf` + cast, `with` copies, PascalCase,
  `enum` field-ids, `<Value>ErrorFfiException` wrappers carrying `.Error`, NUnit
  `Assert.Throws` / `Assume.That`.
- **`gen-csharp-suite` bin** + a line in the mise `gen` task mirroring the Kotlin/Swift ones.
- **Committed generated suite** `csharp/profile-probe/Generated/ProfileConformanceSuite.cs`
  (33 `[Test]` rows over 22 C-IDs), globbed by the SDK-style csproj, run by `test:csharp`; plus
  a hand-written **values-only** `ProfileConformanceFixture.cs` beside it (KC3: no judgement in
  the fixture).
- **Byte-drift check** in `crates/gen-profile-ffi/tests/drift.rs`, run inside `mise run check`.
- **`kotlin_drift` → `foreign_drift` rename** of the shared byte-compare helper (step-13's
  recorded cleanup); all four callers updated (Kotlin codec, Kotlin suite, Swift suite, C#
  suite).
- Tier: `test:csharp` **53/53** (TRX total=53 passed=53 failed=0 skipped=0) — 20 hand-written
  probes + 33 emitted rows. Drift smoke-red performed and reverted.

### M2 — genericity + falsification (commit `0c49b20`, deliverable 3)

- **C# joined the `golden.rs` genericity family.** `note_csharp` / `profile_csharp` added;
  both genericity tests now carry a **per-language concept list**. C# gets its own
  `PROFILE_CONCEPTS_CSHARP`, which **drops lowercase `email`/`availability`** (PascalCase C#
  never emits those) and **keeps lowercase `username`** (it is the check's rule literal).
  Kotlin/Swift guards unchanged.
- **The new arm caught a real bug on first run.** M1's `CSHARP_SUITE_BANNER` hardcoded
  `RunUsernameCheck()` — a profile verb — into *every* suite header, including the check-less
  `gen-note` suite. The C# genericity arm fired **naturally** (natural red on `Username` at
  `golden.rs:484`). Fixed feature-neutral (matching the Kotlin/Swift banners); the committed
  profile C# suite was regenerated (header-only change, no test-row behaviour altered); drift
  green on the regenerated bytes.
- **Falsification, each red watched with its evidence:**
  - Genericity negative test fired naturally on the banner leak, then green after the fix.
  - C# drift check red on an **un-regenerated** emitter mutation (regenerate-first honoured),
    reverted.
  - dotnet planted-red in the emitted C18 row at the current SDK: **exit 1 AND** TRX
    `failed="1"` (`Expected: 7 / But was: 0`), reverted via regenerate, tier back to 53/53.
  - Lifecycle law verified on the emitted C18: releases via deterministic `Dispose()`, **no
    GC references**; the planted-red's "But was: 0" confirms `Dispose` returned the count to
    baseline.

## Deviations from the step doc (all recorded; none structural)

- **M2 "golden" phrasing.** The step doc's deliverable-3 wording ("the C# genericity golden")
  suggested a committed byte-golden, but the `golden.rs` family is an **in-memory,
  per-language concept-list falsification**, not a committed byte artifact. C# joined the
  family **as it actually works** — an in-memory concept guard — rather than minting a new
  golden shape.
- **Fixture is a static factory, not a free function (D25).** C# has no free functions, so the
  values-only fixture is a static class `ProfileConformanceFixtureFactory.Create()`. A missing
  fixture is a **compile error**, not a silent skip — strictly stronger than the
  free-function precedent.
- **C18 releases via `Dispose()`** — the `close()` analogue on C#. The finalizer/GC path stays
  with the hand-written `LifecycleProbe` per the lifecycle law (baseline asserted *before* any
  GC); the emitted C18 row is Dispose-only by design.
- **The emitter takes two namespaces** — `binding_ns = Gen_profile_ffi` and
  `suite_ns = ProfileProbe.Generated` — mirroring Kotlin's two-package signature.
- **C08's `Assume.That(flip != null, …)`** is the exact step-13 Kotlin/Swift precedent; it
  never fires for profile (TRX skipped=0), so no `Assume` shim papers over a broken verb (KC4
  respected).

## Friction log

1. **The `git checkout --` gotcha (M2).** Reverting a *planted* mutation with
   `git checkout -- …ProfileConformanceSuite.cs` restored **HEAD's pre-fix banner**, silently
   discarding the M2 regenerated header. Lesson: after any revert that touches a **generated**
   file, **re-regenerate** — the committed bytes are an output, and `checkout --` rolls them
   back to the last commit, not to the current emitter's output.
2. **The bool-semantics discovery (M0).** "`run_username_check` returns true" reads like the
   verdict but means only "a check *ran*". Both Pass and Fail return `true`; `Ok(false)` is the
   declared-absent capability; the verdict lives in `Snapshot().UsernameCheck`. Cross-checking
   the Kotlin/Swift suites confirmed they already had this right — the risk was a wrong
   contract propagating into the new C# encoding, not a bug in the old ones.

## Kill criteria — none hit (all five)

1. **Tripwire did not fail to go red** — it went red at 0.28.0 for exactly the right reason,
   then was deleted (M0).
2. **StreamProbe draft-stream rows did not stay red** — both are green; #697 confirmed by
   execution in our shape (M0).
3. **No green bought by patching bindgen output / `dist/` or weakening an invariant** — the
   emitter and golden work is in `bolted-ffi-gen`; `dist/` untouched.
4. **No C-ID dishonestly covered** — 22 emitted, C10 exempt via `BOUNDARY_MAP`, no skips, no
   `Assume` shim around a broken verb (TRX skipped=0).
5. **No NEW four-feature break at 0.28.0** beyond the known churn (namespace, file split,
   out-param ABI). `pack:csharp` revealed only the anticipated churn.

## Open questions / notes for the W1/W2 http step (S-WIN part II)

- **`SkipReason` keep-or-delete rides part II.** Per the step doc's scope boundary, deleting
  it before the C# http suite exists would pre-judge the ruling's own condition — it is the C#
  *http* suite that may need skips. The verdict transfers intact to the later step.
- **The .NET adapter probe (W1) will meet the same IR-backend namespace convention** proven
  here: `Gen_*_ffi` (raw-crate-name namespace). No re-derivation needed.
- **TRX is truth for counts; the exit-code caveat carries over.** `dotnet test` *does*
  propagate a nonzero exit (re-proven by M2's planted red: exit 1 *and* TRX failed="1"), but
  counts are quoted from the TRX, never the wrapper. The NuGet-cache eviction and locale
  pinning already live in `test:csharp`.
- **The kit is refreshed at 0.28.0** (deliverable 4): findings 06 and 07 flipped to
  *fixed-in-release* and re-verified by execution; 08's runtime-probe TODO recorded done (still
  ALIVE/unfiled); #663 (finding 02) confirmed shipped in 0.28.0 read-only (the merge commit is
  an ancestor of the `v0.28.0` tag); the watch list rewritten (git-pin machinery + parked
  `step/23-boltffi-git-pin` branch obsolete — **deleting the branch is Henrik's call**). Henrik
  still owns the standalone draft-03 repro and the Defect-2 streaming filings; **nothing is
  ever posted upstream by anyone but Henrik**.
