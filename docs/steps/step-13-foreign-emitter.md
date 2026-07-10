# Step 13 — the foreign-language emitter: per-language contract tests + the stash codec

**Phase 3 — Framework extraction. Status: ready.**

Step 12 made the generated layer safe to hold, and in doing so found that three of its own
deliverables funnel to one missing capability: `bolted-ffi-gen` emits only Rust. This step builds
that capability — **D28, v1.6**: foreign-language artifacts are committed generated source, exactly
D22 one language out — and spends it twice: the **stash codec** (closing step-12 deliverable 5, which
converted) and the **per-language contract tests from the C-IDs** (CONFORMANCE.md's own "where this
suite is going" row, promised since step 06). The design pass that authored this doc added D28 to §8;
its rationale lives there, its implementation slices here.

> **Process note.** Second doc authored in a planning session for a separate implementation session.
> Step 12's report found the first one wrong in four places, all the same shape — pricing work the
> toolchain cannot do as if it could. This doc was written *after* reading the generator, the emitted
> surface, the probe suites and the build plumbing, and it decides the infra questions M4 stopped on
> rather than leaving them. The report should again say where it was wrong anyway.

---

## Scope: two emitted artifacts on one new pipeline

- **The pipeline (D28).** Per-language emitters in `bolted-ffi-gen`, over the same `bolted-decl::
  Feature` every other emitter consumes (D25). Emitted files are committed at source paths the
  platform builds already compile, carry the `@generated` banner, and are **byte-compared** by a
  drift check inside `mise run check` — no Gradle, no Xcode, no boltffi CLI in the loop. `mise run
  gen:ffi` stays the single regeneration verb: it writes *every* committed generated file, whatever
  the language.
- **The codec** — the smallest real foreign artifact, and the only one with a hand-written golden
  reference (`StashCodec.kt`, Phase 3's own doctrine). It proves the pipeline end to end before the
  test emitter rides it, and deletes the last hand-written file D27's version gate ever lived in.
- **The contract tests** — the C-IDs projected through the public generated surface, emitted in
  Kotlin and Swift, each suite generic over a small **hand-written fixture that carries example
  values and nothing else**.

**What "from the C-IDs" means — the boundary, not the algebra.** The Rust suite already proves the
core's semantics, property-based, against four features. The foreign tier's job is different: it
verifies that the *binding and wrapper preserve* those semantics across the boundary — that Kotlin's
`trySetUsername` refusal carries the typed error, that a conflict's `theirs` survives JNI, that
`close()` twice is idempotent on ART. Example-based is therefore the right strength, not a
concession: the properties stay in Rust, where the generators can drive them; the foreign tests
check the seam. A foreign test that fails names either a binding bug or a wrapper bug — both ours or
upstream's, never the core's, because the core's own tier already passed.

## What the design pass handed over

**D28 — committed generated foreign source (§8, v1.6).** Read the row first. The two facts that
shaped it: byte-comparison is honest for foreign files *because nothing else owns them* (no
formatter rewrites them — rustfmt is what forced D22 to compare code rather than bytes), and
string-building in plain Rust beats a template engine because a template file is a second source of
truth with no compiler on it.

**The fixture shape — example values only, invariant logic never.** The emitter knows every field,
verb and DTO from the declaration, but it cannot know *example values*: a valid raw, a distinct
second valid raw, a raw that fails tier-1 — and rule keys live in `#[bolted::rules]` impl bodies the
declaration never sees (step 12, deliverable 7's lesson). So the emitted suite is generic over a
fixture in exactly the Rust suite's sense: the emitter emits the test functions *and* the fixture
interface; a human writes the ~30-line implementation per feature per language, supplying values
only. If an emitted test cannot pass without the fixture making a *judgement* (anything beyond
"here is a value"), the shape is wrong — that is kill criterion 3, not a thing to work around.

**The observability map is the first deliverable, not a discovery log.** Not every C-ID crosses the
boundary: C10's superseded-completion race cannot be constructed through a synchronous check driver
that runs `begin`/`complete` atomically; parts of C07's precedence clause may need states the public
surface cannot compose. The step's discipline is that every one of the 23 IDs gets an explicit
verdict — **emitted** or **exempt with a stated reason** — recorded in CONFORMANCE.md and enforced
by the same manifest mechanism that already keeps that document honest, so the accounting cannot
rot. An ID that is observable but lacks a surface accessor is not exempt: the generator gains the
accessor (it is our output), and the ID is emitted.

**The typed-accessor question (step 08, friction 1) is mostly already answered — verify, don't
rebuild.** The Rust suite needed `ConformanceFeature::primary()` because no trait can carry
heterogeneous typed field access. The *emitter* has no such problem: it monomorphizes per field from
the declaration, and D24 already gave the surface per-field typed state (`snapshot.username:
TextFieldState`), per-field verbs (`try_set_username`, `resolve_*`), and a public stash DTO with
per-field `raw`/`base`. M0's audit lists what is genuinely missing; expect gaps, not chasms.

## Deliverables

1. **The observability map.** Every C01–C23 classified emitted/exempt in CONFORMANCE.md (new
   section, per-ID accounting with reasons), enforced by a manifest test in `bolted-ffi-gen` that
   parses the document and compares it against the emitter's actual ID list — both directions, the
   `tests/manifest.rs` discipline. CONFORMANCE.md's "where this suite is going" table also gets its
   stale step-10 row corrected (this is step 13's work, and C# is step 14's).
2. **The foreign-emission pipeline.** `foreign` module(s) in `bolted-ffi-gen`; banner; `gen:ffi`
   writes the foreign files; per-feature drift tests byte-compare them inside `mise run check` via
   workspace-relative paths. Layout inside the crate is the implementer's (smallest reversible);
   the doctrine — string-building, no template crate, D25's single parse — is not.
3. **The generated Kotlin stash codec.**
   `android/profile-app/src/main/kotlin/dev/bolted/profileapp/generated/ProfileStashCodec.kt`,
   emitted from the declaration; behavioural referee is the existing stash/restore test set (probe +
   app tiers), golden reference is the hand-written file. **`StashCodec.kt` is then deleted** —
   step-12 deliverable 5, closed for real. Apple gets no codec: nothing stashes on Apple, and an
   emitted file with no consumer is dead code (recorded, revisit when an Apple shell persists).
4. **The emitted Kotlin contract suite.**
   `android/profile-probe/src/androidTest/kotlin/dev/bolted/profileprobe/generated/` — the fixture
   interface + the test class for every M0-emitted ID, running on the existing emulator tier
   (`test:android`). The hand-written fixture (`ProfileConformanceFixture.kt`, values only) lives
   beside it, un-generated.
5. **The emitted Swift contract suite.**
   `apple/profile-probe/Tests/ProfileProbeTests/Generated/` — same IDs, same fixture shape, running
   under `test:apple`. (Emitted file/type names are the implementer's; mirror the Kotlin choices.)
6. **The genericity falsifier.** `gen-note` emission at the text level: a golden test asserting the
   emitters produce plausible output for the second feature and name no profile concept. Packing and
   *running* a second feature on both platforms is tier cost that teaches nothing new about the
   emitter; text-level is the honest scope, and the report says so. (A suite with one implementor is
   shaped like it — the memory that named this rule is why this deliverable exists.)
7. **The falsification pass.** Planted-red per language (break the generator's emission of one
   invariant → watch the foreign suite fail → restore); a positive control for **each** drift check
   (edit a committed foreign file → `check` goes red — a drift check reading a wrong path is green
   forever, step 10's lesson generalized); the mutation discipline from step 10 (regenerate before
   testing, prove the output changed).
8. **Accessor gaps from M0** added to the generated Rust FFI, each named in the report.
9. **Report + ROADMAP.** Including where this doc was wrong.

## Milestones

- **M0 — the observability map.** Walk C01–C23 against the public generated surface; classify;
  write the CONFORMANCE.md section; extend the manifest enforcement; list accessor gaps. **Gate: if
  more than a third of the IDs land exempt, stop — that is kill criterion 4, an FFI-surface finding
  for a design session.** (Prior expectation from the survey: exemptions on the order of C10 and
  fractions of C07/C12, not a third.)
- **M1 — the pipeline, proven on the codec.** Emit the Kotlin codec; diff against the golden until
  the stash test set passes through it; delete `StashCodec.kt`; drift check into `check`.
- **M2 — the Kotlin contract suite.** Two waves if useful (lifecycle/rebase set first; stash + check
  set second); fixture hand-written once; counts read from the JUnit XML.
- **M3 — the Swift contract suite.** Same map, same waves.
- **M4 — the falsification pass + the gen-note golden.** Deliverables 6 and 7.
- **M5 — report + ROADMAP.**

## Kill criteria (real; if hit, stop and report)

1. **The drift check cannot stay pure.** If byte-comparing the committed foreign files inside
   `mise run check` turns out to need Gradle, Xcode or the boltffi CLI, D28's premise fails — stop,
   report, design session. (It should not: generation is text in, text out.)
2. **`dist/` or bindgen internals.** Anything that requires patching BoltFFI's output or reaching
   into the binding's internal machinery — step 12's KC1, carried forward unchanged. Emitted code
   consumes the *public* generated surface only.
3. **The fixture needs a judgement.** Any emitted test that cannot pass without invariant logic in
   the hand-written fixture means the generic shape is wrong — stop; the fix is a design decision,
   not a fatter fixture.
4. **The exempt list exceeds a third of the IDs** (M0 gate above). The step's premise is that the
   public surface can express the contract; if it broadly cannot, that is the finding, and working
   around it per-ID would bury it.

## Non-goals (→ elsewhere)

- **C#** — step 14, which becomes the emitter's third fixture and real genericity test.
- **An Apple codec** — no consumer (deliverable 3).
- **Replacing the hand-written probe suites** — `FreezeContractProbe`/`FreezeContractTests` stay as
  independent evidence; overlap with emitted tests is redundancy, not waste.
- **Deriving fixture values from constraints** — fragile against custom validators and rule keys;
  the fixture stays human.
- **The schema version's derivation** (constraint-semver) — Phase 4, D27.
- **Filing the step-12 upstream drafts** — the owner's action. Note: if filing 04 (public wire
  ser/de) is accepted upstream later, the generated codec retires in favour of `toByteArray()` —
  deliverable 3 is still worth building now; it is also the pipeline's proof.

## Inherited cautions

- `test:android*` tiers: `--rerun-tasks` before quoting any number; counts from the JUnit XML
  (`test:android` and `test:android:hazard` write to the same file).
- `cargo fmt --all` after every `gen:ffi` run — the Rust half of the output is still rustfmt-owned
  (step 12's friction 1). The foreign half is emitter-owned: nothing may reformat it, which is what
  makes byte-comparison honest. If a `.editorconfig`/ktlint/SwiftFormat hook ever touches those
  paths, the drift check will say so loudly — that is it working.
- A forbidding test can forbid nothing: every drift check and the manifest each get a planted-red
  before they are trusted (deliverable 7).
- `pack:android` still carries the expansion-env workaround; nothing here touches it.

## Exit checklist

- [ ] CONFORMANCE.md accounts for every C-ID at the per-language tier (emitted/exempt + reason);
      the manifest test enforces it in both directions.
- [ ] `mise run gen:ffi` writes every committed generated file, Rust and foreign; `mise run check`
      drift-checks them all with no new toolchain dependency.
- [ ] `StashCodec.kt` deleted; the stash/restore test set green through the generated codec.
- [ ] Emitted Kotlin and Swift suites green in their existing tiers; counts quoted from XML/output.
- [ ] Planted-red evidence recorded for: each foreign suite, each drift check, the manifest.
- [ ] The gen-note emission golden exists and names no profile concept.
- [ ] Accessor gaps (if any) landed in the Rust generator and named in the report.
- [ ] `docs/steps/step-13-report.md` written — including where this doc was wrong; ROADMAP updated;
      §9 untouched (nothing here touches it).
