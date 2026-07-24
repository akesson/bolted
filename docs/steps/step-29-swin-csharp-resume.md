# Step 29 — S-WIN part I: the C# resume, at released 0.28.0

**Phase 4 — Harness (unblocking leg). Status: ready.**

Third attempt, and the first with the ground verified solid. Step 14 stopped on kill
criterion 1 (the C# check driver throws before the checker runs — the MarshalAs(I1) bug);
step 15 branch B proved it still broken at 0.27.5; step 23 git-pinned upstream main at
`23cf2ec`, watched the tripwire go red (the MarshalAs fix is real) — and stopped on KC3,
because the same PR (#654) collapsed same-named `#[ffi_stream]` methods across classes
(finding 07). Both are now **fixed at released 0.28.0**, verified 2026-07-21 against the
findings' own descriptions, not the PR labels (contract-freeze-agenda §Standing inputs):
finding 07 by #697 (three distinct stream-runtime classes, two distinct `Snapshots()`
overloads, distinct native `EntryPoint` symbols) and MarshalAs by the IR backend's
out-param shape (`run_username_check` returns its bool via an `out` param; `I1` survives
only where the type *is* bool). The workspace already rides registry 0.28.0 everywhere
(the pins moved with the bolted-http sequence); the step-23 git-pin machinery is obsolete.

This step finishes what steps 14/15/23 specified and never got to build: the namespace
rename, the tripwire's designed flip, and the emitted C# contract suite with genericity
and falsification (step-15 deliverables 3–4, step-23 deliverables 3–5 — verbatim scope,
minus everything the pin made moot).

**Scope boundary (recorded here so nobody re-litigates it mid-step):** S-WIN's *http* legs
(spike-plan §5 — W1's standalone .NET adapter probe, W2's conformance rows through the
FFI, W3's background family) are **not this step**. They follow as their own step at the
S-AP/S-AN granularity, against the post-27 ruled contract. Consequently the `SkipReason`
keep-or-delete ruling (freeze agenda, smaller decisions) **transfers to that step** — it is
the C# *http* suite that may need skips; deleting before it exists would pre-judge the
ruling's own condition.

## What the planning pass verified (by checking, 2026-07-24)

- **Pins and CLI are already there.** All five workspace `boltffi = "0.28.0"` (registry);
  `setup:boltffi` `want="0.28.0"` with the git-build eviction guard; `cargo install --list`
  shows `boltffi_cli v0.28.0` with no `?rev=`. No pin work exists in this step.
- **The rename is 9 sites across 7 files**, all under `csharp/profile-probe/`:
  `GenProfileFfi` → `Gen_profile_ffi` (the IR backend names the namespace after the raw
  crate name). The 2026-07-21 verification found the probe suite fails to compile on
  exactly this churn and nothing else. Known ABI churn from step-23's log: `run_*_check`
  bool is now an out-param; the monolithic binding split into ~35 per-type files.
- **The emitter seam is ready.** `bolted-ffi-gen/src/foreign.rs` holds the D28 model:
  `BOUNDARY_MAP` (22 emitted C-IDs, C10 exempt, language-neutral) and the
  `emit_kotlin_contract_suite` / `emit_swift_contract_suite` precedents (plain Rust
  string-building, `@@MARKER@@` substitution, no template engine, provenance headers).
  Drift checks live in `crates/gen-profile-ffi/tests/drift.rs` and run inside
  `mise run check`. The shared byte-compare helper is `kotlin_drift`
  (`bolted-ffi-gen/src/lib.rs:348`) — used by the Kotlin codec, Kotlin suite, *and* Swift
  suite checks; its rename to `foreign_drift` is step-13's recorded cleanup and lands here.
- **The genericity precedent** is `bolted-ffi-gen/src/golden.rs` (from ~line 400): the
  foreign emitters are proven non-profile-shaped by emitting for `gen-note`'s surface. The
  C# emitter joins that family.
- **The committed-suite path is free.** `ProfileProbe.csproj` is SDK-style (zero explicit
  `Compile Include`) — a generated `.cs` committed beside the probes compiles with no
  project-file edit, exactly the "path `dotnet test` already compiles" the step-23 doc
  asked for.
- **The C# emitter model is fully mapped** (step-14 report, "What M2/M3 would need"):
  `readonly record struct` DTOs; `abstract record` + nested `sealed record` DUs;
  PascalCase; `with` copies; `enum` field-ids; `<Value>ErrorFfiException` wrappers carrying
  `.Error`; NUnit `Assert.Throws`. Use it; don't re-derive it.

## What steps 14/15/23 hand over (use it, don't re-derive it)

- **The tripwire**: `csharp/profile-probe/CallbackDriverProbe.cs`,
  `TheCheckDriverIsBrokenOnThisBackend` — asserts the *broken* behavior; going red is its
  designed end state (its own doc-comment says so). Step 23 already watched it go red once
  at `23cf2ec` for exactly the right reason (`Expected: <MarshalDirectiveException> But
  was: null`); this step watches it go red at released 0.28.0, then deletes it.
- **The parked probes** (step 14 recorded them unobservable while the driver threw):
  D23's `DraftClosed` refusal on `run_username_check` after close; D10's `[Pending,
  Passed]` on the check stream (driver-fact scope per v1.11); the reentrant checker
  no-deadlock shape; `fillValid`'s create-flow check. They come alive here as the real
  C13/C16 callback tests.
- **The seam**: `pack:csharp` / `test:csharp` exist and handle NuGet cache eviction and
  `DOTNET_CLI_UI_LANGUAGE=en`; TRX is the authoritative count source (`dotnet test` *does*
  propagate a nonzero exit — proven once by step-13 M3's planted red — but counts are
  quoted from the TRX, never the wrapper).
- **The lifecycle law** (ARCHITECTURE v1.7, banked in step 14): C# drafts have a
  finalizer that reaches store-side close, so the leak-freedom test asserts its baseline
  **before any GC** — a GC'd forgotten Dispose must not green the test. The shape to
  inherit is `LifecycleProbe.DeterministicDisposeReturnsCountsToBaseline_NoGCInvolved`.

## Deliverables

1. **The rename + the verdict.** The 9-site `GenProfileFfi` → `Gen_profile_ffi` rename;
   `pack:csharp` + `test:csharp` at 0.28.0. **First check: finding 07 in our shape** — the
   StreamProbe draft-stream rows (the two that timed out at `23cf2ec`) must be green, and
   `draft.Snapshots()` must demonstrably route to the draft's own subscription. Then the
   tripwire goes **red for the right reason** → delete it and bring the parked probes
   alive (D23 refusal, D10 `[Pending, Passed]`, reentrant checker, `fillValid`
   create-flow). `mise run check` green throughout.
2. **The emitted C# contract suite** (step-15 deliverable 3, verbatim):
   `csharp_contract_suite` emitter in `bolted-ffi-gen` on the D28 model; a `gen-csharp-suite`
   bin + a line in the gen task mirroring the Kotlin/Swift ones; committed generated source
   under `csharp/profile-probe/` (compiled by the existing csproj, run by `test:csharp`);
   byte-drift-checked from `crates/gen-profile-ffi/tests/drift.rs` inside `mise run check`;
   the `kotlin_drift` → `foreign_drift` rename lands here. 22 emitted C-IDs, values-only
   fixture beside the generated file, no judgement in the fixture (KC3 discipline, step 13).
3. **Genericity + falsification** (step-15 deliverable 4, verbatim): the C# genericity
   golden on `gen-note`'s surface (join the `golden.rs` family); the dotnet planted-red
   re-proven at the current SDK (nonzero exit *and* TRX counts); every new drift check
   watched red before being trusted (regenerate-first — prove the output changed, then
   compare); the leak-freedom baseline-before-GC shape carried into any emitted lifecycle
   rows.
4. **Upstream kit refresh at 0.28.0** (`upstream/boltffi/`): 06 flips to
   *fixed-in-release* (0.28.0, out-param shape, tripwire evidence); 07 flips to
   *fixed-in-release* (#697) — never filed, filing now moot; 08's runtime-probe TODO is
   done (step-27 M0 confirmed the union claim) — record it; 02's #663 ships in 0.28.0 —
   record which release carries it; watch list rewritten: the git-pin machinery and the
   parked `step/23-boltffi-git-pin` branch are obsolete (record; deleting the branch is
   Henrik's call). The Defect-2 streaming issue and the draft-03 repro remain **Henrik's
   filings — nothing posted anywhere, ever.**
5. **Report + ROADMAP.** `step-29-report.md` (built / deviations / friction log / open
   questions — including anything the resume teaches about the eventual W1/W2 http step);
   ROADMAP row updated.

## Milestones

- **M0 — the rename + the verdict.** Deliverable 1. Commit.
- **M1 — the emitter.** Deliverable 2. Commit.
- **M2 — genericity + falsification.** Deliverable 3. Commit.
- **M3 — kit refresh + report.** Deliverables 4–5. Commit.

## Kill criteria (real; if hit, stop and report)

1. **The tripwire does not go red at 0.28.0** for the right reason. That contradicts the
   2026-07-21 verification — a finding, not an obstacle. Stop, bank the evidence in
   draft 06, report.
2. **The StreamProbe draft-stream rows stay red at 0.28.0** — finding 07 is not fixed in
   our shape (the #697 verification was inspection, not execution). Stop; 07 revives in
   the kit; back to planning.
3. **Green bought by patching bindgen output / `dist/` or weakening a frozen invariant** —
   the standing step-14/15/23 rule, unchanged.
4. **An emitted C-ID cannot honestly be covered** beyond C10's standing exemption. No
   skips, no `Assume` shims around a broken verb.
5. **`pack:csharp` at 0.28.0 reveals a NEW four-feature break** beyond the known churn
   (namespace, file split, out-param ABI). Stop; the kit grows an entry; back to planning.

## Non-goals (→ elsewhere)

- The C# **http** leg — S-WIN W1/W2/W3 (spike-plan §5) is its own later step; the
  `SkipReason` keep-or-delete verdict rides *that* step, not this one.
- WinUI / Windows hardware (step-07 KC4 precedent; the seam is host-portable — `test:csharp`
  runs on this Mac).
- Filing/posting anything upstream — the kit is local; Henrik files.
- Touching the boltffi pins in any direction; touching anything `bolted-http`.
- ARCHITECTURE §9 questions; glossary changes.

## Inherited cautions

- Never pipe a tier through `tail`/`head` when its exit code is load-bearing (step-23 M1's
  self-caught mistake); read the artifact.
- TRX is truth for counts; the NuGet cache eviction and locale pinning are already in
  `test:csharp` — don't duplicate them.
- A drift check makes a mutation pass vacuous — regenerate first, prove the output changed,
  exclude the drift test from the falsification claim.
- A forbidding test can forbid nothing — watch every planted red actually red.
- Symbol inspection uses the target-triple path (`target/aarch64-apple-darwin/debug/`);
  plain `target/debug/` lacks the FFI symbols.
- Before any pack: `cargo install --list` must show `boltffi_cli v0.28.0` with no `?rev=`.
- Commit per milestone; never `git -C`; build/test only via `mise run check` /
  `mise run test` / the named tiers.

## Exit checklist

- [ ] Finding 07 re-verified **by execution** in our shape (draft-stream rows green);
      tripwire watched red at 0.28.0 for the right reason, then deleted; D23 / D10 /
      reentrant-checker / `fillValid` probes alive and green; `mise run check` +
      `test:csharp` green, counts TRX-derived.
- [ ] Emitted C# suite committed + drift-checked inside `check`; `foreign_drift` rename
      done; 22 C-IDs covered, C10 exempt, values-only fixture.
- [ ] C# genericity golden on `gen-note`; dotnet planted-red re-proven; every new drift
      check watched red first; lifecycle law honoured.
- [ ] Kit refreshed at 0.28.0; nothing posted; watch list no longer mentions the git pin.
- [ ] `step-29-report.md` written; ROADMAP row updated; ARCHITECTURE untouched.
