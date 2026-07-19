# Step 23 — boltffi git-pin to main: the C# resume, for real

**Phase 4 — Harness (unblocking leg). Status: ready.**

Step 14 stopped on kill criterion 1 (the C# check driver throws before the checker runs); step 15
bumped to 0.27.5 and proved the driver **still** broken there (branch B — M2/M3 stayed unbuilt).
The ground has now actually moved: upstream **#654 ("Migrate C# to the new IR backend") merged
2026-07-16** and the fix is verified at source level, but **no release carries it** — latest is
0.27.5, cut before the merge. Henrik decided (2026-07-19, the bolted-http go pass): **git-pin
boltffi main** rather than wait. This step executes that decision and finishes what step 15's
branch A specified: the emitted C# contract suite, genericity, falsification.

This step is also the precondition for the bolted-http sequence (steps 24+): S-WIN's FFI leg
(spike-plan §5, W2) rides this pin.

## What the planning pass verified (by checking, 2026-07-19)

- **The fix is real, not just labeled.** On main, `return_marshal_i1` is derived per `ReturnPlan`
  in `boltffi_backend/src/target/csharp/render/mod.rs`: `true` only for a direct
  `Primitive(Bool)` return, explicitly `false` in the encoded-`FfiBuf` arm — exactly the keying
  draft 06 asked for. The exact bug shape is an upstream fixture
  (`tests/fixtures/source/callback/async_callback_return_shapes.rs`, `check_enabled: Result<bool,
  LoadError>`), and the C# DemoTest runs throwing async callbacks end-to-end.
- **The pin rev is `23cf2ecce20327581a0d03b41aee6af9cd081ea3`** (main HEAD 2026-07-18) — after
  #654 (`53aecd1`, the C# IR backend), #657 (Kotlin `fun interface`, merged 2026-07-15), #663
  (JVM use-after-close guard, merged 2026-07-14), and #693 (Kotlin desktop loader fix). One rev
  everywhere; do not mix.
- **The version string is ambiguous.** Main's workspace version is *still* `0.27.5`, so a
  git-installed CLI reports the same `boltffi 0.27.5` as the release. Every existing version
  grep — `setup:boltffi`'s idempotence check, doctor's `BOLTFFI_PINNED` cross-pin
  (`crates/bolted-check/src/doctor.rs:26`, enforced by `tests/doctor_manifest.rs`) — can no
  longer tell the two apart. `cargo install --list` *does* show the source
  (`boltffi_cli v0.27.5 (https://github.com/boltffi/boltffi#23cf2ecc)`); that is the
  discriminating artifact.
- **Churn will be real this time.** Step 15's C# bindings were byte-identical across the bump;
  #654 replaces the whole C# backend, so the regenerated C# surface *will* differ, and the
  committed emitted artifacts (D28) may need mechanical updates. Swift/Kotlin may churn too
  (#657, #663, #693 all touch generated surfaces). Mechanical rename-level updates are in scope
  (step-11 precedent); semantic shifts are not — see kill criteria.
- **Pin surface**: four workspace `Cargo.toml`s (`bolted-ffi`, `gen-note-ffi`, `gen-profile-ffi`,
  `spike-profile-ffi`, all `boltffi = "0.27.5"`) plus `setup:boltffi`'s `want` and doctor's
  literal. The non-workspace spike crates (`spike-http-ffi`,
  `spike-profile-ffi-stall-probe`, loose `"0.27"`) are frozen evidence — untouched.

## What steps 14/15 hand over (use it, don't re-derive it)

- **The tripwire**: `csharp/profile-probe/CallbackDriverProbe.cs` —
  `TheCheckDriverIsBrokenOnThisBackend` asserts the *broken* behavior and goes red the moment
  the driver works. Its going red is this step's M1 verdict and the pin's proof.
- **The C# emitter model, fully mapped** (step-14 report, "What M2/M3 would need"): `readonly
  record struct` DTOs; `abstract record` + nested `sealed record` DUs; PascalCase; `with`
  copies; `enum` field-ids; `<Value>ErrorFfiException` wrappers carrying `.Error`; NUnit
  `Assert.Throws`/`Assume.That`. The emitter is a mechanical port of the step-13
  marker-substitution model (`@@MARKER@@`, plain Rust string-building, no template engine).
- **The seam**: `pack:csharp`/`test:csharp` exist; NuGet cache eviction and
  `DOTNET_CLI_UI_LANGUAGE=en` handled; TRX is the authoritative count source.
- **The boundary map**: `BOUNDARY_MAP` (22 emitted C-IDs, C10 exempt) is language-neutral.
- **The lifecycle law**: ARCHITECTURE v1.7 — the C# leak-freedom test must assert its baseline
  **before any GC** (the finalizer reaches store-side close; a GC'd forgotten Dispose must not
  green the test).

## Deliverables

1. **The git pin.** The four `Cargo.toml`s move to
   `boltffi = { git = "https://github.com/boltffi/boltffi", rev = "23cf2ecce20327581a0d03b41aee6af9cd081ea3" }`;
   `Cargo.lock` updated. `setup:boltffi` installs
   `cargo install --git … --rev … boltffi_cli` (keep the CARGO_HOME canonicalization — the
   askama symlink bug doesn't care where the source comes from) and its idempotence check reads
   `cargo install --list` for the **rev**, not `boltffi --version` (ambiguous, see above).
   Doctor's cross-pin: keep the human-readable `0.27.5` display but extend the literal/manifest
   agreement so the rev is what's actually cross-checked — smallest honest shape wins; record it
   in the report. If the manifest test can't express it cheaply, a recorded exemption with the
   reason is acceptable (doctor is warn-never-fail; the *manifest agreement* is the build gate).
2. **Regeneration + the full ladder green.** `gen:ffi`, all packs, all tiers: `check`,
   `test:web`, `test:apple` (+`:gen`), `test:android` (+`:gen`, `:app`, forced rerun),
   `test:csharp`; `test:apple:ui` if a GUI session exists. Every quoted count read from the
   artifact (JUnit XML / TRX / cargo output), never the wrapper. **Churn log** in the report:
   what the IR backend changed in the regenerated C# surface (this is the evidence for how
   expensive lagging the pin is, and the record the next bump diffs against).
3. **The verdict, delivered by the tripwire.** `TheCheckDriverIsBrokenOnThisBackend` goes
   **red** → flip `CallbackDriverProbe` from asserting the break to asserting the contract:
   checker invoked, verdict lands, `[Pending, Passed]` on the stream (D10, driver-fact scope per
   v1.11), reentrant checker doesn't deadlock, `run_username_check`'s D23 `DraftClosed` refusal
   (step 14 recorded it unobservable — it comes alive here).
4. **The emitted C# contract suite** (step-15 deliverable 3, verbatim): `csharp_contract_suite`
   emitter in `bolted-ffi-gen` on the D28 model; committed generated source at a path
   `dotnet test` already compiles; byte-drift-checked inside `mise run check`; the
   `kotlin_drift` → `foreign_drift` rename lands here (step-13's recorded cleanup). 22 emitted
   C-IDs, values-only fixture, no judgement in the fixture (KC3 discipline).
5. **Genericity + falsification** (step-15 deliverable 4, verbatim): the genericity golden on
   `gen-note`'s surface; per-language planted-red proving the dotnet tier's failure mode
   (nonzero exit, TRX counts); every new drift check watched red before being trusted
   (regenerate-first); the leak-freedom baseline-before-GC shape inherited from
   `LifecycleProbe.DeterministicDisposeReturnsCountsToBaseline_NoGCInvolved`.
6. **Upstream kit refresh** (`upstream/boltffi/`): dispositions re-verified **at the pinned
   rev** — 06 should flip to *fixed-verified-locally* with the tripwire evidence; 02's runtime
   half (#663) is on main too, re-run its probe; 03/04 unchanged unless the rev says otherwise.
   **Nothing posted anywhere — owner files.** Watch list gains: "next release supersedes the
   git pin; return to a version pin when it ships."
7. **Report + ROADMAP.** `step-23-report.md` (built / deviations / friction log / open
   questions, plus the churn log); ROADMAP row updated.

## Milestones

- **M0 — the pin.** Cargo.tomls, lock, `setup:boltffi` (git flavor + rev-aware idempotence),
  doctor cross-pin, `gen:ffi`, `mise run check` green. Commit.
- **M1 — the ladder + the verdict.** All packs, all tiers, artifact counts; tripwire red;
  probe flipped (deliverable 3). Commit.
- **M2 — the emitter.** Deliverable 4. Commit.
- **M3 — genericity + falsification.** Deliverable 5. Commit.
- **M4 — kit refresh.** Deliverable 6. Commit.
- **M5 — report + ROADMAP.** Commit.

## Kill criteria (real; if hit, stop and report)

1. **The tripwire stays green at the pinned rev.** The verified fix does not reach our shape —
   that is a finding, not an obstacle. Stop, bank the evidence in draft 06, report. Do not
   patch `dist/`, do not pin a different rev hunting for green.
2. **Green bought by patching bindgen output or weakening a frozen invariant** — the standing
   step-14/15 rule, unchanged.
3. **The pinned rev introduces a *new* four-feature break on any backend** (the IR migration
   regressing something 0.27.5 did right). Stop; the kit grows an entry; the pin decision goes
   back to planning (fallback: wait for the release after all).
4. **The emitted C# suite cannot honestly cover an emitted C-ID** beyond C10's standing
   exemption. No skips, no `Assume` shims around a broken verb.

## Non-goals (→ elsewhere)

- Filing/posting anything upstream — kit is local; owner files.
- Any `bolted-http` work — that starts at step 24 with this pin as its floor.
- WinUI / Windows hardware (step-07 KC4 precedent; the seam is host-portable).
- ARCHITECTURE §9 questions; performance beyond the tiers' existing assertions.

## Inherited cautions

- `test:android`'s **exit code lies** — JUnit XML only; `--rerun-tasks`, delete stale XML.
- `dotnet` localizes; TRX is truth (`test:csharp` handles both).
- The NuGet cache shadows re-packed fixed-version packages (`test:csharp` evicts).
- A drift check makes a mutation pass vacuous — regenerate first, prove the output changed.
- A forbidding test can forbid nothing — watch every planted red actually red.
- `cargo install` gotchas: `--locked` matters when siblings float (step-15's 0.27.3 lesson);
  a git install reports the *same* `0.27.5` version string as the release — discriminate by
  `cargo install --list` source line, never by `boltffi --version`.
- Commit per milestone; never `git -C`; build/test only via `mise run check` / `mise run test`.

## Exit checklist

- [ ] Four Cargo.toml pins + `setup:boltffi` + doctor cross-pin all at rev `23cf2ec…`;
      `mise run check` and every tier green, counts artifact-derived; churn logged.
- [ ] The verdict recorded **by the tripwire going red**, probe flipped to assert the working
      contract, step-14's blocked probes alive.
- [ ] Emitted C# suite + `foreign_drift` rename + genericity golden + dotnet planted-red proof
      banked; every new drift check watched red first.
- [ ] Kit refreshed at the pinned rev; nothing posted; watch list points at the next release.
- [ ] `step-23-report.md` written; ROADMAP row updated; ARCHITECTURE untouched (v1.13 already
      carries this step's design input).
