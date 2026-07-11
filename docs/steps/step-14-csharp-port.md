# Step 14 — the C# port: a third backend, a third emitted language, and a finalizer that changes a table

**Phase 3 — Framework extraction. Status: ready.**

Every claim this project makes about "per language" rests on two languages whose bindings one team
(Apple's and Android's) shaped. Step 14 adds the third: BoltFFI's **C# backend** carries the same
generated Rust surface to .NET, a hand-written probe re-runs step 02/05's due-diligence on it, and
`bolted-ffi-gen` emits the **third contract suite** — the first one whose genericity is proven by
*packing and running*, not just at the text level (step 13's recorded open item). The tier is
**headless `dotnet test` on this Mac**: the binding seam is host-portable even though the WinUI face
is not, exactly the step-05 move (probe the boundary years before the UI exists).

> **Process note.** Third doc authored in a planning session for a separate implementation session.
> This one was written after *running the backend*: `boltffi generate csharp` and `boltffi pack
> csharp` were executed against `gen-profile-ffi` into a scratch directory, and the emitted 34
> files were read. The two prior docs erred in opposite directions (step 12 optimistic ×4, step 13
> conservative ×3); this one's guesses are marked as guesses. The report should say where it was
> wrong anyway.

---

## What the design pass verified (by running it, 2026-07-11, boltffi 0.27.3)

- **The C# backend exists and is not experimental.** `boltffi generate csharp` / `pack csharp` are
  first-class (kmp and dart carry `--experimental`; csharp does not). `gen-profile-ffi/boltffi.toml`
  already holds a `[targets.csharp]` stanza, `enabled = false`.
- **`generate csharp` on `gen-profile-ffi` emits a complete surface**: 34 files, ~3 200 lines, and
  **all four load-bearing BoltFFI features are present, idiomatically**:
  1. *Classes with methods* — `ProfileDraftFfi : IDisposable`, every method behind
     `ThrowIfDisposed()`.
  2. *Typed errors* — `error_style = throwing` as exceptions wrapping the decoded DTO
     (`UsernameErrorFfiException(UsernameErrorFfi)`); `DraftClosedFfiException` for D23 refusals.
  3. *Streams* — `IAsyncEnumerable<ProfileSnapshot> Snapshots(CancellationToken)`: the platform's
     own async-iteration idiom, with unsubscribe/free in a `finally`.
  4. *Callback traits* — `interface UsernameChecker` + a vtable bridge and proxy, same shape as
     Kotlin/Swift.
- **`pack csharp` runs to within one missing tool.** It built the host dylib
  (`aarch64-apple-darwin`), generated bindings, staged a complete package project —
  `BoltFFI.CSharp.csproj`, **`net10.0`**, `RuntimeIdentifiers = osx-arm64`, sources under `src/`,
  native assets under `runtimes/<rid>/native` — and failed only at `dotnet pack: No such file or
  directory`. **The .NET SDK is the single bootstrap item**, and mise's `core:dotnet` provides it.
- **Two lifecycle facts that contradict what we've written down** (read in the generated source;
  M1 verifies them at runtime before anyone amends a document):
  - **`ProfileDraftFfi` has a finalizer**: `~ProfileDraftFfi() { Dispose(); }`, with an
    `Interlocked.Exchange` guard making Dispose idempotent and race-free. If the finalizer reaches
    the store-side close (M1's job to prove), then **ARCHITECTURE §6's table row — "Kotlin / C#:
    `close()` only, the GC never frees the Rust draft" — is wrong for C#**, and §6's "forgetting it
    leaks … in every language" overclaims. This is also, verbatim, **D26's recorded revisit
    condition**: "if upstream grows an opt-in Cleaner *inside bindgen*, where the CAS makes it
    safe, revisit." It grew one, on this backend. D26's warning then applies with full force: under
    a finalizer, a forgotten `Dispose()` passes every test that does not provoke a collection —
    which is exactly why the D26 leak-freedom contract test for C# must be designed around GC, not
    despite it (deliverable 2).
  - **Use-after-dispose is a typed refusal, not UB**: every method throws
    `ObjectDisposedException` off the zeroed handle. Step 05's H2 hazard (silent UB, dangling
    aliasing) **may not exist on this backend** — the probe confirms, and the report says what that
    does to the upstream filing's scope.

Both are *structural* findings about frozen documents. The implementation session **must not**
amend ARCHITECTURE §6 or D26 — it runs the experiments, the report records the runtime truth, and
the next design pass amends with evidence in hand.

## Scope: one probe, one emitter extension, one tier

- **The toolchain seam** — dotnet pinned by mise but **task-scoped** (the JDK/Gradle precedent from
  step 05: `mise run check` must never drag an SDK onto a machine that only builds Rust);
  `[targets.csharp]` enabled; `pack:csharp` and `test:csharp` verbs.
- **The hand-written probe** — step 05's shape on backend #3: re-confirm the four features, answer
  the two lifecycle questions above, and record the IDisposable/`using` ergonomics that the ROADMAP
  sketch called "the C# client." No UI, no ViewModel: binding shape without a WinUI host to judge
  it is guesswork (non-goal).
- **The emitted C# contract suite** — D28 spent a third time: `csharp_contract_suite` beside the
  Kotlin/Swift emitters, over the same `bolted-decl` parse (D25) and the same `BOUNDARY_MAP`
  (22 emitted / C10 exempt — the map is language-neutral and does not change), generic over a
  hand-written values-only fixture (KC3 unchanged), byte-drift-checked inside `mise run check`,
  **run for real** under `test:csharp`. This is the packed-and-run genericity proof step 13
  deferred: if the emitter carries an assumption the text golden cannot see, this is where it
  surfaces.

## What step 13 hands over (use it, don't re-derive it)

The **role model** (checked = the `#[check]` field; primary/secondary = the other text fields in
declaration order), the **values-only fixture** contract (~30 lines per feature per language, C08's
tier-2 rule as `RuleFlip` *data* — name, dirty edits, flipped canonical, pins), the **marker
substitution** emission style (raw-string templates, `@@MARKER@@` + chained `replace`, no
`format!`, no template engine), and the lesson that only *names* mirror across languages — the
Swift bodies were ~as much work as Kotlin's, and C#'s will be too (`using` vs `use{}` vs ARC,
NUnit's `Assume` vs JUnit's, exception pattern-matching vs `guard case`). Budget for real template
work, not a transliteration.

## Deliverables

1. **The toolchain seam.** .NET SDK **10** (the target framework boltffi emits) pinned via mise's
   `core:dotnet`, scoped to the `pack:csharp`/`test:csharp` tasks — **not** in `[tools]`.
   `[targets.csharp] enabled = true` in `gen-profile-ffi/boltffi.toml`; a `pack:csharp` task
   mirroring `pack:apple` (setup:boltffi dependency, guard message); `test:csharp` packs first
   (the `test:apple` precedent) and runs `dotnet test` headless. Telemetry off
   (`DOTNET_CLI_TELEMETRY_OPTOUT=1`) in the task env.
2. **The C# freeze-contract probe** — hand-written, `csharp/profile-probe/` (a test project
   consuming the packed artifact), the step-05 item list on backend #3:
   the four features exercised for real; `Dispose()` idempotent and `using`-friendly;
   use-after-dispose → `ObjectDisposedException` on every verb class (is H2 dead here?);
   D23's `DraftClosedFfiException` on mutators with observers total, positive-controlled per step
   11's trap (a `catch` that swallows it must fail a control); the typed errors carrying key+params
   data; `IAsyncEnumerable` delivering `[Pending, Passed]` off the check driver (D10's stream-only
   `Pending`, third backend); a reentrant callback not deadlocking; **the finalizer experiment** —
   abandon an undisposed draft, `GC.Collect()` + `GC.WaitForPendingFinalizers()`, read
   `live_draft_count`/`rebasing_draft_count`, **with a still-referenced control draft proving the
   collection actually ran** (the ART GC-probe memory: a probe without a control measures nothing).
   Plus the **D26 leak-freedom contract test, C#-shaped**: deterministic teardown returns C22's
   count to baseline via `Dispose`, and the test must stay meaningful under a finalizer that would
   absolve a forgotten one — say in the report how (e.g., assert *before* any collection can run,
   or pin GC latency mode), rather than shipping a test a GC pause can green.
3. **The emitted C# contract suite.** `csharp_contract_suite(source, namespace)` +
   `check_csharp_contract_suite_drift` in `bolted-ffi-gen::foreign`; a `gen-csharp-suite` bin and a
   `gen:ffi` block; the committed suite at a source path the probe project already compiles
   (`csharp/profile-probe/Generated/ProfileConformanceSuite.cs` or the implementer's equivalent);
   the hand-written values-only `ProfileConformanceFixture.cs` beside it. Same 22 emitted C-IDs, 33
   tests, over the map unchanged. **NUnit** is the doc's choice of framework (its `Assume.That` is
   the direct analog of the JUnit `Assume`/`XCTSkip` arm the emitter already has); if reality
   disagrees, swap and record. While adding the third consumer, land step 13's recorded cleanup:
   **`kotlin_drift` → `foreign_drift`**.
4. **The genericity golden, third language.** `gen-note` emission through the C# emitter; the
   existing `PROFILE_CONCEPTS` needles asserted absent; the can-fire companion extended so every
   needle provably appears in the C# *profile* suite. Same two-sided discipline as step 13 — and
   note step 13's golden caught a live leak on its first run; treat this one as load-bearing.
5. **The falsification pass.** The step-13 evidence table, third row: planted-red through the
   emitter → regenerate → **watch `dotnet test` fail on the named test** → restore via `gen:ffi`
   (not `git checkout` — step-13 friction 3) to byte-identity; a hand-edit positive control for the
   new drift check at a distinct line; the genericity golden watched red by re-introducing a leak.
   The planted-red doubles as the tier's honesty check: confirm a failing C# test makes the
   **task's exit code nonzero** (unlike `test:android`), and say in the report where counts are
   read from (console/TRX).
6. **CONFORMANCE.md.** The "where this suite is going" row and the per-language-tier foot updated
   from two languages to three; the `BOUNDARY_MAP` and per-ID table unchanged (if an ID's C# story
   genuinely differs, that is a report finding, not an ad-hoc map edit). Manifest test still green
   both directions.
7. **Report + ROADMAP.** Including where this doc was wrong, the runtime verdict on the two
   lifecycle facts (input to the §6/D26 design pass), and the upstream-filing implications if H2 is
   dead on C#.

## Milestones

- **M0 — toolchain + the packed artifact.** dotnet via mise (task-scoped); `pack:csharp` green end
  to end; the `.nupkg` carries `runtimes/osx-arm64/native/libgen_profile_ffi.dylib`; a minimal
  `dotnet test` (the `Ping` export) proves the native library loads and calls. **Gate: if the
  packed library cannot be loaded and called from `dotnet test` on this Mac, stop — that is kill
  criterion 2, and an emitted suite with no referee is not worth shipping.**
- **M1 — the probe** (deliverable 2). The four features first, then the lifecycle experiments. The
  finalizer and H2 verdicts get written down *with their evidence* the moment they're known.
- **M2 — the emitted suite** (deliverables 3 and 6). Emitter + fixture + drift check + the
  `foreign_drift` rename; suite green under `test:csharp`; CONFORMANCE prose updated; manifest
  green.
- **M3 — genericity + falsification** (deliverables 4 and 5).
- **M4 — report + ROADMAP** (deliverable 7).

## Kill criteria (real; if hit, stop and report)

1. **A four-feature break.** One of classes/streams/typed-errors/callbacks is missing or broken on
   the C# backend at runtime — VISION risk #1 on a third backend. Stop; report; upstream draft.
   (The design pass saw all four *in the emitted text*; only M0/M1 can confirm they run.)
2. **No runnable tier.** The packed artifact cannot be loaded and exercised by `dotnet test` on
   this machine after honest effort. Text-level-only C# is not this step; stop and report.
3. **The fixture needs a judgement** — KC3, carried verbatim from step 13.
4. **The drift check cannot stay pure.** `mise run check` must not grow a dotnet dependency — the
   C# drift is `include_str!` + byte compare, exactly the Kotlin/Swift shape. dotnet belongs to
   `pack:csharp`/`test:csharp` only.
5. **`dist/` or bindgen internals** — carried unchanged. Emitted code consumes the public generated
   surface only.

## Non-goals (→ elsewhere)

- **The WinUI shell, and anything needing Windows.** No Windows machine exists in this loop; the
  tier is deliberately dotnet-on-macOS (the seam is host-portable, the face is not). The Windows
  RID, the WinUI app, tray/services are a future step gated on hardware — the step-07 KC4
  precedent: record unassessed, don't simulate.
- **A C# stash codec.** Nothing stashes on .NET; an emitted file with no consumer is dead code
  (the Apple-codec reasoning, verbatim).
- **An INPC/ViewModel layer.** Binding ergonomics without a real WinUI host to judge them is
  speculation; the probe records raw-surface ergonomics only.
- **Amending ARCHITECTURE §6 / D26.** The probe supplies the evidence; a design session moves the
  documents.
- **NuGet publishing, other RIDs, filing the upstream drafts.**

## Inherited cautions

- **Trust the artifact, not the wrapper**: `test:android`'s exit code masks failures; before
  quoting any C# number, prove once (via M3's planted-red) what `dotnet test` failure actually
  looks like at the task boundary, then quote counts from that proven source.
- The cargo package is **`gen_profile_ffi`** (underscore); a hyphenated `-p` errors — and a
  grep-filtered invocation can hide that error as a silent pass (step-13 friction 2).
- `cargo fmt --all` after every `gen:ffi`; **nothing may reformat the foreign paths** — there is no
  `.editorconfig` in this repo today and `dotnet format` must not be introduced; if anything ever
  rewrites the committed `.cs`, the drift check saying so is it working.
- Restore emitter experiments with `mise run gen:ffi`, never `git checkout` (an uncommitted emitter
  fix makes checkout a lie); `touch` restored files so cargo doesn't reuse a stale binary.
- A forbidding test can forbid nothing: every new drift/genericity check gets its planted-red
  before it is trusted.

## Exit checklist

- [ ] `pack:csharp` and `test:csharp` exist, task-scoped dotnet only; `mise run check` unchanged in
      its dependencies and green.
- [ ] The probe answers, with runtime evidence: four features · finalizer-vs-store-side-close ·
      use-after-dispose typing · D23 controls · D26-shaped leak test · `[Pending, Passed]` ·
      reentrancy.
- [ ] The emitted C# suite is committed generated source, drift-checked in `check`, green under
      `test:csharp`; fixture is values-only; `foreign_drift` rename landed.
- [ ] Genericity golden covers C# with a can-fire companion; planted-red evidence recorded for the
      suite, the drift check, and the golden; the tier's failure mode proven once.
- [ ] CONFORMANCE.md speaks of three languages; manifest green both directions; `BOUNDARY_MAP`
      untouched (or a divergence reported, not patched).
- [ ] `docs/steps/step-14-report.md` written — including where this doc was wrong and the §6/D26
      evidence; ROADMAP updated; ARCHITECTURE untouched.
