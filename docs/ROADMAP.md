# Bolted — Roadmap

Phased plan to validate the [architecture](ARCHITECTURE.md), freeze the design, then extract
the framework. **Progressive elaboration**: only the current step has a detailed step doc in
`docs/steps/`; later steps are sketched here and get their step doc when they become current
(authored in a planning session, not by the implementer).

**Working agreement**: one step ≈ one focused implementation session. Every step ends with a
`docs/steps/step-XX-report.md` (what was built, deviations, friction log, open questions) and a
status update in the table below. Kill criteria are real: if hit, stop and report — do not
work around them.

## Status

| Step | Title | Phase | Status |
|------|-------|-------|--------|
| 01 | Core semantics prototype (pure Rust) | 1 — Spike | **done** — [plan](steps/step-01-core-semantics.md) · [report](steps/step-01-report.md) |
| 02 | BoltFFI due-diligence probe (Apple) | 1 — Spike | **done** — [plan](steps/step-02-boltffi-probe.md) · [report](steps/step-02-report.md) |
| 03 | SwiftUI spike app | 1 — Spike | **done** — [plan](steps/step-03-swiftui-app.md) · [report](steps/step-03-report.md); items 2–6 automated (XCUITest, `test:apple:ui`), item 1 confirmed by hand |
| 04 | Rust web spike app | 1 — Spike | **done** — [plan](steps/step-04-rust-web-app.md) · [report](steps/step-04-report.md); zero-FFI path proven, no kill criteria hit, wasm baseline 304 KiB (85 KiB brotli) |
| 05 | Android headless probe | 1 — Spike | **done** — [plan](steps/step-05-android-probe.md) · [report](steps/step-05-report.md); chattiness kill criterion clears (~80×), `close()` proven mandatory on ART |
| 06 | Design freeze | 2 — Freeze | **done** — [plan](steps/step-06-design-freeze.md) · [report](steps/step-06-report.md); ARCHITECTURE **frozen (v1.0)**, [CONFORMANCE.md](CONFORMANCE.md) C01–C18 with a build-time drift check |
| 07 | Kotlin/Compose spike app | 2 — Freeze | **done** — [plan](steps/step-07-kotlin-compose-app.md) · [report](steps/step-07-report.md); stash/restore lands (C20/C21), a **frozen-core defect** found and fixed (C19), Compose UI tests run **headless**. Kill criterion 4 (hardware chattiness) **unassessed** — no device |
| 08 | Extract bolted-core + conformance suite | 3 — Extraction | **done** — [plan](steps/step-08-extract-bolted-core.md) · [report](steps/step-08-report.md); store is id-keyed and **lock-free** (D16), the FFI's store loop is deleted, suite is generic and runs against **two** features |
| 09 | bolted-macros | 3 — Extraction | **done** — [plan](steps/step-09-bolted-macros.md) · [report](steps/step-09-report.md); `value`/`entity`/`rules` ship, two **generated** features pass the suite unmodified, `feature_model` **cut** (D21) |
| 10 | bolted-ffi + a generated FFI layer | 3 — Extraction | **done** — [plan](steps/step-10-bolted-ffi.md) · [report](steps/step-10-report.md); the FFI layer **generates** and runs from Swift (D22–D25). A macro could never have done it: bindgen reads source text. **Deliverable 10 (repoint the shells) deferred to 11** |
| 11 | Migrate the shells onto the generated FFI | 3 — Extraction | **done** — [plan](steps/step-11-migrate-shells.md) · [report](steps/step-11-report.md); all four shells link `gen-profile-ffi`, `pack:*` repointed, spike kept as reference. D23 controls planted-and-watched on both platforms. Hardware "after": **0.0432 ms p50** per keystroke on the Pixel 8a (~23× under KC5); `test:apple:ui` 9/9 on generated |
| 12 | FFI hardening | 3 — Extraction | **done** — [report](steps/step-12-report.md); D23 fix (3-layer planted-red), leak-freedom pinned (D26), **D27** envelope + **C23**, l10n coverage (Swift's first), name-collision tripwire. Codec deletion **converted** (needs step 13's foreign emitter); 5 upstream drafts. No kill criteria hit |
| 13 | Per-language contract tests from the C-IDs | 3 — Extraction | **done** — [report](steps/step-13-report.md); **D28** shipped: Kotlin stash codec + both contract suites are committed generated source, byte-drift-checked in `check` (no Gradle/Xcode/NDK/boltffi). 22 emitted C-IDs (C10 exempt), 33 tests/language, generic over a values-only fixture (KC3 held — even C08's rule is `RuleFlip` data). `StashCodec.kt` deleted; `delete_canonical` the one accessor gap. Genericity golden caught a live Swift leak; every drift/manifest/suite check watched red. `test:android` 80/80 · `test:apple` 75+20. No kill criteria hit |
| 14 | C# port + generator | 3 — Extraction | pending |
| — | The `Feature` trait | design session | **needed before Phase 4** — see step-09 report, headline 4 |
| 15+ | Verification harness & Ring 0 | 4 — Harness | unplanned |

## Phase 1 — Design validation spike

Everything is hand-written ("write the generated code by hand first"): no macros, no framework
crates published, one deliberately gnarly feature (a profile editor with composite value
object, tier-2 rule, async uniqueness check, live rebase + conflicts). The spike exists to
falsify the design cheaply — friction logs from these steps are the input to the design freeze.

- **Step 01 — Core semantics prototype (pure Rust).** Workspace + mise bootstrap; prototype
  `bolted-core` primitives (`Value`, `Field`, `Draft`, `Store`, single-flight); hand-written
  profile feature; all 12 architecture invariants as tests (§7 of ARCHITECTURE.md). No FFI,
  no UI. *Detailed step doc exists.*
- **Step 02 — BoltFFI due-diligence probe (Apple).** Export the profile feature via BoltFFI;
  verify the four features the design depends on: classes with methods (draft handles), async
  streams (snapshots), `Result` methods with typed error enums, callback traits
  (capabilities). Swift test target, no UI. Measure call overhead.
  *Kill criterion: any of the four features missing/broken → architecture session before
  proceeding (this is VISION risk #1 materializing).*
- **Step 03 — SwiftUI spike app.** Real form UI on the step-02 bindings: validate the text
  echo rule (cursor survives trim-sanitization while typing fast), conflict UI
  (keep-mine/take-theirs), live rebase demo (background canonical change), submit flow.
  Also lands the two core fixes decided after step 01 (ARCHITECTURE §8): value-bound
  async-verdict reset (invariant 13, with its test) and failed `submit` returning the draft
  handle with the error.
- **Step 04 — Rust web spike app.** Same feature, **Leptos** (browser CSR only), consuming the
  core as a plain crate — zero FFI, no codegen. **Done, no kill criteria hit**: wasm32 discipline
  holds with the core still zero-dep; `bolted_core::Store` served a reactive shell unmodified (and
  F3 ran against the real store for the first time); the sans-io async check ran from `spawn_local`
  with no executor in the core; the echo rule survived in a signal framework. Baseline: **304 KiB
  `.wasm`** (85 KiB brotli) — of which a bare Leptos CSR app is 100 KiB, so the feature costs
  ~204 KiB. Key findings for the freeze: a Rust shell **does not want the snapshot stream**
  (read-direct + a version tick is race-free and forks nothing); `submit`'s by-value `!Clone` handle
  cannot be called from a struct field without a scratch checkout; F6's edit-to-equal-theirs reads as
  *confusing* in a running UI; F2 (never-run check) is again the default path.
- **Step 05 — Android headless probe.** `boltffi pack android` + Kotlin instrumented tests on a
  headless Gradle-managed ART emulator, no UI. **Done, no kill criteria hit.** The chattiness
  kill-criterion **clears with ~80× headroom**: a per-keystroke round-trip (`try_set` + `snapshot`)
  costs **12–13 µs** on ART against a 1.0 ms bar, so the core-validates-every-keystroke contract
  needs no shell-side write buffer. All four BoltFFI features re-confirmed on a second codegen
  backend (streams collect on the main Looper; typed error payloads survive; a reentrant callback
  does not deadlock). Two contract findings: **(1)** on Kotlin, GC **never** frees a draft —
  `close()`/`use {}` is the only free path, the exact inverse of Apple/ARC, and an abandoned draft
  is an unreachable zombie that `apply_canonical` keeps rebasing (**this answers §9's `close()`
  question**); **(2)** use-after-close is **silent UB** — no crash, and after allocator churn the
  dangling handle aliases another live draft. Also: a draft snapshot's `version` is frozen at
  checkout (stale after rebase), so step-02's version-stamped reconcile works for observing the
  entity but not a draft. Artifact baseline: **485 KiB stripped** arm64 `.so` (5.36 MB unstripped).
  *Caveat: an arm64 emulator on an arm64 host is the right VM and the wrong CPU — the latency
  numbers are lower bounds, to be re-checked on hardware in step 07.*

## Phase 2 — Design freeze

- **Step 06 — Design freeze.** **Done.** Reconciled all five friction logs and resolved every §9
  question Phase 1 could answer, into ARCHITECTURE §8 as **D1–D13**, each with its losing
  alternative. ARCHITECTURE.md is **frozen (v1.0)**; the invariants are promoted to
  [CONFORMANCE.md](CONFORMANCE.md) (C01–C18) with a test that parses the document and fails the build
  if it drifts from the suite. At the owner's direction the freeze also **conformed the reference
  implementation**, so the contract and the code agree: three separate wounds (step-01 F3/F5,
  step-03 friction 1, step-04 friction 1) turned out to be one and were closed by making the handle a
  lifecycle object; F1/F2 were closed by C13+C16; F6 became C14; F7, Q1–Q4 and the `Copy` question
  are settled. The stale draft `version` step 05 found is fixed (C15) — the version-guarded reconcile
  step 02 shipped had never once fired on a draft stream. **Kill criteria: none hit.** Neither did
  D9 survive contact unchanged: implementing "focused **and dirty**" exposed a caret-eating
  regression, and the shipped predicate is "focused **and touched**" (report, deviation 1).
- **Step 07 — Kotlin/Compose spike app.** **Done. Four of five kill criteria cleared.** Planning it
  found a **verified defect in the frozen core**: `rebase` never compared `theirs` against `base`, so
  a dirty field conflicted whenever the server moved *any other* field — against `theirs` that was its
  own ancestor. C03's proptest never sampled `theirs == base`, and a conformance test had been
  producing the spurious conflict since step 01 without asserting on it. Fixed as **D14/C19**, with a
  regression test at every tier that should have caught it, each verified to fail with the fix
  reverted. **Stash/restore** (§9's last undesigned Phase-2 mechanism) lands as **D15/C20/C21**:
  `{base_version, per-field (raw, base)}` + `Store::adopt`, with `sync` and the async verdict
  deliberately *not* stashed — C13 + C16 then make a restored draft safe with no new invariant.
  **Android has a headless UI tier**: Compose UI tests drive a real render tree on the Gradle-Managed
  Device, which is precisely what XCUITest cannot do. Config change and `onCleared()`→`close()` both
  hold. *Kill criterion 4 (per-keystroke round-trip on physical hardware) is **unassessed**: no device
  was attached. `mise run bench:android:device` is written and double-gated against emulators.*

## Phase 3 — Framework extraction

Extract from evidence, in dependency order; the hand-written spike code becomes the golden
reference the generated code is diffed against.

- **Step 08 — Extract `bolted-core` + the conformance suite.** **Done. No kill criteria hit.** The
  store concurrency question is answered by **D16**: `Store<D>` owns its drafts in a
  `BTreeMap<DraftId, _>`, ships **no lock**, and returns its fan-out as data — so it is `Send` by
  construction and one implementation serves the lock-free web shell and the FFI's single `Mutex`
  alike. `spike-profile-ffi`'s hand-written store loop is **deleted**. The weak-drafts question is not
  answered but **dissolved**: with the store owning drafts and handles being `Copy` ids, there is no
  owner to drop. The price is named in **C18** — `close(id)` is now mandatory in Rust too, and the
  reference implementation stops being forgiving in the one way the GC platforms are not. The RAII
  alternative was built and rejected on evidence (its `Drop` panics on an already-borrowed `RefCell`;
  rung 4). **D17** moves the resolvers onto `Draft` and adds `Stashable`. The suite is extracted into
  **`bolted-conformance`** (22 IDs, 31 generic functions, three tiers, macro-stamped so a fixture
  cannot skip one) and now runs against **two** features — `spike-note` was written expressly to
  falsify "generic", and immediately did: a `StoreDraft::is_based` that consults a single field passed
  all 21 other invariants, on both features. **C12** gained a clause and a test. Also: the
  `liveDraftCount` divergence step 07 could only document is closed by construction (**C22**).
- **Step 09 — `bolted-macros`.** **Done. No kill criteria hit.** `value`, `entity` and `rules` ship;
  `gen-note` (20 code lines, replacing 269) and `gen-profile` (135, replacing 574) each pass
  `bolted-conformance` **unmodified** — the same 37 and 62 tests their hand-written originals score.
  `gen-note` was written *first*, because a macro with one input is shaped like that input.
  **Writing the macro is what made the core honest**: three judgements about to be emitted per feature
  moved down to rung 1 — `Field::required_error` (D13's `Unset` → `required`), `commit_gates` (C07's
  gates), `SingleFlight::violation` (C13 + C16) — and `golden.rs` now *fails the build* if emitted code
  mentions `Validity::`, `CheckState::`, `CommitError::Conflicted/Orphaned` or `is_ok()`. **D8 moved
  from rung 3 to rung 2**: the macro refuses a `Copy` value rather than leaving it to `bolted-check`.
  **D18** gives the async check a contract (`Checked`), and `AsyncCheckFeature` shed four members with
  no test changing. **D19** dissolves "codegen dedup by raw type" (generics already dedup on the axis
  that varies; the residue is FFI-side, step 10). **D20** scopes `#[bolted::value]` to newtypes.
  **D21 cuts `feature_model`** — it needs boltffi, and the `Feature` trait it would stamp *has never
  been written, in any of five spikes*. (*Step 10 amends the first clause: a macro emitting `#[data]`
  tokens links nothing, and `feature_model` was impossible for a better reason — bindgen cannot see
  macro output at all.*) The mutation pass (12 mutations, checked in at
  `steps/artifacts/step-09-mutations.py`) found **C07 had no precedence clause**: `commit_gates`
  reordered to check conflicts before orphaned passed all 22 invariants on all four features, because
  every `c07` assertion built a draft failing exactly one gate. C07 amended; ARCHITECTURE is **v1.3**.
  Also caught, by reading the emitted code rather than the tests: a uniform guard was cloning a
  `Username` on every keystroke of the *name* box.
- **Step 10 — `bolted-ffi` + a generated FFI layer.** **Done. No kill criteria hit.** A feature's FFI
  layer now *generates* from its declaration: `gen-note-ffi` (479 lines from a 20-line declaration) and
  `gen-profile-ffi` (631 generated + 138 hand-written, replacing 1 054), and `apple/gen-profile-smoke`
  proves the whole chain — declaration → generated Rust → generated Swift → compiles → links → runs
  (7 tests). **The headline is that `#[bolted::feature_model]` was never possible**: BoltFFI's bindgen
  `read_to_string`s the crate's sources and parses them with `syn`, so macro output is silently omitted
  from the bindings. D21 reached the right verdict from the wrong premise. Hence **D22** — the FFI layer
  is *committed generated source*, drift-checked by `mise run check`, which buys it rustc, clippy and a
  code-review diff, three rungs macro output never gets. **D23** gives a store-side-released draft a
  typed `DraftClosed` on every mutating verb (observers stay total); **D24** collapses the field-state
  families onto the *raw* type, closing §9's dedup residue; **D25** parses the declaration once, in the
  new `bolted-decl`, because two parsers are two contracts and the drift check would compare a generator
  against itself. §9's **`Pending` across FFI** is answered by measurement: it reaches a stream
  subscriber, never a `snapshot()` caller, so no split `begin`/`complete` is needed. **KC2 dissolved** —
  the generated FFI never crosses a `CheckId` (it monomorphizes `run_username_check()`), and *could not*,
  since `ProfileCheck` is macro output. D18 stands as a Rust-side contract the generator consumes.
  The mutation pass (`steps/artifacts/step-10-mutations.py`) had to **regenerate before testing**, or the
  drift check would catch every mutation vacuously; run honestly it found **six survivors**, all
  *projection* properties — `any_dirty` pinned false, conflicts reversed, `take_theirs` keeping mine, a
  `Pending` check rendering as `Unchecked`. Four new tests; now 14 caught, 0 survived.
- **Step 11 — migrate the shells onto the generated FFI.** *(Step 10's deliverable 10, deferred rather
  than half-done: the four Swift and Kotlin shells still link the **hand-written** `spike-profile-ffi`.)*
  The work-list is measured, not guessed, in
  [`steps/artifacts/step-10-surface-delta.md`](steps/artifacts/step-10-surface-delta.md) — 62 declarations
  hand-written, 57 generated, **42 identical**; the rest are D24 renames, D23's added `try`, the checker
  protocol's new shape, and one arity change. Blast radius: 25 source files, 5 build files. *Detailed step
  doc exists.* **The gate is M0**: `boltffi pack android` has never been run on a generated crate, and
  `pack:android` carries step 05's expansion-env workaround — precisely the environment step 10 found
  triggers the whole-crate metadata blob. If a generated crate cannot pack for Android and the cause is
  upstream, that is kill criterion 1: the Swift half ships alone and the upstream filing comes forward.
  **The trap is D23**: a `try?` or a `runCatching {}` swallows `DraftClosedFfi` and reinstates the exact
  silent no-op D23 abolishes, with every test still green. Each probe gets a positive control, verified to
  fail with the refusal swallowed.

  **Inherited cautions.** `mise run test:android:app` **can report BUILD SUCCESSFUL without running a
  test** (Gradle up-to-date); force `--rerun-tasks` before quoting a number, and read counts out of the
  JUnit XML — `test:android` and `test:android:hazard` write to the *same* file. And step 10's lesson,
  which generalizes past codegen: **a test that forbids something can be forbidding nothing** —
  `golden.rs`'s needles were written against `quote`'s token spacing and matched no line of a
  `prettyplease`-formatted file, green and vacuous. Pin a forbidding test from both sides.
- **Step 12 — FFI hardening. Done** ([report](steps/step-12-report.md); ARCHITECTURE **v1.5**). The
  **D23 bug step 11's controls found** is fixed (the check driver resolves draft liveness before the
  no-checker short-circuit) and watched red on three layers. **D26** leak-freedom is a per-language
  contract test that bites (removing `onCleared()`'s `close()` fails it). **D27** shipped as a
  versioned, parse-don't-validate envelope: the schema version rides the generated DTO, `accept_stash`
  is a typed gate returning a `StashAcceptedFfi` token, `restore` takes only the token — a shape forced
  by BoltFFI being unable to return a class handle from a throwing method, and *stronger* for it.
  **C23** pins the degradation claim. Swift got its **first l10n coverage test** (drive-the-core, not a
  declared-key list — rule keys live in impl bodies, so a declared list cannot be complete). Four
  places the doc mispriced the FFI seam are recorded in the report: **codec deletion converted** (a
  Kotlin emitter is step 13's charter, not an M4 chore), and the ergonomics helpers (6a checker lambda,
  6b Sendable) funnel to the same "`bolted-ffi-gen` emits only Rust" root. Five upstream drafts written
  (not filed). No `dist/` patched; no kill criteria hit. The Fable-plans/Opus-implements split earned
  its keep here — a planner's optimism about the seam is what the implementer caught.
- **Step 13 — per-language contract tests from the C-IDs. Done** —
  [report](steps/step-13-report.md). **D28 shipped**: the Kotlin stash codec and both per-language
  contract suites are **committed generated source** — D22 one language out — emitted by
  `bolted-ffi-gen` over the one parsed declaration (D25), living at source paths the platform builds
  already compile, **byte-compared** by the drift check inside `mise run check` (honest for foreign
  files precisely because no formatter owns them; no Gradle/Xcode/NDK/boltffi in the loop). The
  pipeline was spent twice: the **Kotlin stash codec** (`StashCodec.kt` deleted for real, closing
  step-12 deliverable 5) and the **emitted contract suites** — **22 emitted C-IDs** (C10 the one
  exemption), **33 tests per language**, projected through the public surface in Kotlin and Swift,
  each generic over a hand-written **values-only** fixture. KC3 held: even C08's tier-2 rule is
  `RuleFlip` *data*, no judgement. The foreign tier verifies **the boundary, not the algebra**. M0's
  observability map is manifest-enforced against CONFORMANCE.md both ways; the one accessor gap
  (`delete_canonical`, needed by C07/C11) was added to the Rust surface. The **genericity golden**
  (run on `gen-note`) caught a live Swift leak — `ProfileStashFfi` frozen in a C20 comment — and every
  drift check, the manifest, and both suites were watched **red** before being trusted (per-language
  planted-red on ART and XCTest). `test:android` 80/80 · `test:apple` 75+20. No kill criteria hit.
  Where the doc was wrong: three places, all the planner being *conservative* (exemptions
  over-predicted, Swift priced as a Kotlin mirror when only names mirror, the genericity golden priced
  as a formality that then found a bug).
- **Step 14 — C# port + generator.** Hand-write the C# client first (IDisposable ergonomics — C18 is
  not optional here, WinUI binding shape), then the generator template.

## Phase 4 — Verification harness & Ring 0 (unplanned sketch)

`bolted-check` (binding drift, capability coverage, constraint semver snapshots, WASM size
budget), `bolted new` scaffolding, `doctor`, the standard mise verb set — per VISION.md's
in-scope list. Planned after Phase 3 ships evidence about what drifts in practice.
