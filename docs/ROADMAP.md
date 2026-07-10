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
| 10 | bolted-ffi + regenerate Swift/Kotlin | 3 — Extraction | **ready** |
| 11 | C# port + generator | 3 — Extraction | pending |
| — | The `Feature` trait | design session | **needed before Phase 4** — see step-09 report, headline 4 |
| 12+ | Verification harness & Ring 0 | 4 — Harness | unplanned |

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
  been written, in any of five spikes*. The mutation pass (12 mutations, checked in at
  `steps/artifacts/step-09-mutations.py`) found **C07 had no precedence clause**: `commit_gates`
  reordered to check conflicts before orphaned passed all 22 invariants on all four features, because
  every `c07` assertion built a draft failing exactly one gate. C07 amended; ARCHITECTURE is **v1.3**.
  Also caught, by reading the emitted code rather than the tests: a uniform guard was cloning a
  `Username` on every keystroke of the *name* box.
- **Step 10 — `bolted-ffi`** (only crate importing boltffi) + regenerate the Swift and Kotlin
  spike apps from macros; per-language contract tests generated from the C-IDs. Inherits a
  requirements list the probes wrote: **use-after-close must raise a typed error** (silent UB today,
  §9 — and step 07 shows it distorting a ViewModel's shape) and probably a `Cleaner` backstop — note
  D16 hands the mechanism over, since a stale `DraftId` is simply *not live* and the remaining UB
  belongs to BoltFFI's raw-pointer handles, not to the registry; emit **`@Parcelize`/`Codable`** for
  DTOs (a shell that persists one hand-writes a codec today); emit the **Compose parameter-passing
  rule** (a Compose shell must never read core state by calling a VM method — strong skipping makes it
  invisible); **verify l10n key coverage per target**; a Kotlin ViewModel must `close()` in
  `onCleared()`; project `Send + Sync` Rust classes as `Sendable` Swift classes; emit `fun interface`
  for single-method capability traits; a platform-stdlib **name-collision policy** (`Date`, `URL`,
  `Data`, `Error`); no hyphens in crate names; expose the split `begin`/`complete` so `Pending` is
  observable to a `snapshot()` caller. Per-language contract tests will need **generated typed field
  accessors**: `Draft` is id-keyed and cannot expose `Field<V>` heterogeneously, which is why
  `ConformanceFeature` supplies them (step 08, friction 1). `liveDraftCount`'s semantics no longer need
  pinning — C22 did it. Also: **report the `boltffi pack android` bug upstream** and delete the
  workaround in `mise run pack:android`.

  **Inherited from step 09**: the FFI now regenerates from `gen-profile`, not `spike-profile`, so the
  shells lose the inherent `begin_username_check` family and gain `Checked` keyed by `ProfileCheck`
  (D18) — **whether a `CheckId` enum actually crosses `#[data]` is unverified, and if it cannot, D18 is
  wrong** (step 09's kill criterion 4 was assessed by inspection only). `try_set_availability` takes a
  tuple now. §9's **FFI dedup of field-state families** lands here: `dto.rs` stamps three structurally
  identical `…FieldState` families for the three `Raw = String` values, and D19 says that duplication is
  *this* crate's to answer, not the macro's. Watch for the step-09 friction that generalizes: **a
  generated binding can be behaviourally identical and quietly more expensive** — the guard bug was
  invisible to 22 invariants. And `mise run test:android:app` **can report BUILD SUCCESSFUL without
  running a test** (Gradle up-to-date); force `--rerun-tasks` before quoting a number in a report.
- **Step 11 — C# port + generator.** Hand-write the C# client first (IDisposable ergonomics — C18 is
  not optional here, WinUI binding shape), then the generator template.

## Phase 4 — Verification harness & Ring 0 (unplanned sketch)

`bolted-check` (binding drift, capability coverage, constraint semver snapshots, WASM size
budget), `bolted new` scaffolding, `doctor`, the standard mise verb set — per VISION.md's
in-scope list. Planned after Phase 3 ships evidence about what drifts in practice.
