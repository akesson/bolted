# Bolted — Roadmap

Phased plan to validate the [architecture](ARCHITECTURE.md), freeze the design, then extract
the framework. **Progressive elaboration**: only the current step has a detailed step doc in
`docs/steps/`; later steps are sketched here and get their step doc when they become current
(authored in a planning session, not by the implementer).

**Working agreement**: one step ≈ one focused implementation pass — since 2026-07-19 driven by
the planning session itself, with implementation delegated to Opus sub-agents (previously: a
separate fresh Opus session per step). Every step ends with a
`docs/steps/step-XX-report.md` (what was built, deviations, friction log, open questions) and a
status update in the table below. Kill criteria are real: if hit, stop and report — do not
work around them.

## Status

| Step | Title | Phase | Status |
|------|-------|-------|--------|
| 01 | Core semantics prototype (pure Rust) | 1 — Spike | **done** — [plan](steps/step-01-core-semantics.md) · [report](steps/step-01-report.md) |
| 02 | BoltFFI due-diligence probe (Apple) | 1 — Spike | **done** — [plan](steps/step-02-boltffi-probe.md) · [report](steps/step-02-report.md); an independent parallel probe (different wrapper design, divergent stream verdict at 0.27.3, moot at 0.27.5) lives at `spikes/profile-ffi-stall-probe/` |
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
| 14 | C# port + generator | 3 — Extraction | **stopped on kill criterion 1** — [plan](steps/step-14-csharp-port.md) · [report](steps/step-14-report.md); M0 (toolchain seam + packed artifact loads/calls) and M1 (probe, 14 tests) **done**; the emitted suite + genericity/falsification **not built** because feature 4 (callbacks) is broken on the C# backend: `run_username_check` throws (a boltffi 0.27.3 codegen bug — wrong return-marshalling on a struct-returning P/Invoke). Findings banked: §6's C# "GC never frees" row is **wrong** (a finalizer reaches store-side close — D26 revisit met), H2 looks **dead** (use-after-dispose is typed). Needs a §6/D26 design pass + an upstream fix before resuming |
| 15 | boltffi 0.27.5 bump: resume C#, or prove why not | 3 — Extraction | **done (branch B)** — [plan](steps/step-15-boltffi-bump.md) · [report](steps/step-15-report.md); five pins → 0.27.5, every runnable tier green (`test:apple:ui` env-blocked, not a regression). Tripwire still green → **C# driver still broken at 0.27.5** (byte-identical `MarshalAs(I1)`-on-`FfiBuf` bug), so the emitted C# suite (M2/M3) stayed unbuilt. Upstream kit (`upstream/boltffi/`) re-verified: **01 fixed** (pack-android workaround removed), **02/03/04/06 alive → to file**, **05 not reproducible → do-not-file**; nothing posted *(since filed upstream — status lives in `upstream/boltffi/README.md`)*. Churn tiny (Swift/C# byte-identical, Kotlin +26 lines JNI diagnostics); 0.27.3 CLI now needs `--locked` |
| — | The `Feature` trait | design session | **done** — resolved as **D29** (ARCHITECTURE **v1.8**, step-16 planning pass): §1 rewritten to the store-owned shape that shipped, the unwritten trait struck, the never-built `command` verb demoted to §9. Phase 4's gate is discharged |
| 16 | `bolted-check`: the constraint-surface snapshot | 4 — Harness | **done** — [plan](steps/step-16-bolted-check.md) · [report](steps/step-16-report.md); the third emitter over the one parser (D25). A committed, human-readable, byte-checked `.snap` per feature — a constraint *tightening* now fails the build at the exact line and names the `STASH_SCHEMA_VERSION` duty (D27), where every existing drift check was blind to it. Composites covered via a runtime section; renderer stays pure |
| 17 | Web shell onto `gen-profile` + the wasm size budget | 4 — Harness | **done** — [plan](steps/step-17-wasm-size-budget.md) · [report](steps/step-17-report.md); the last shell leaves the spike (`profile-web` on `gen-profile`, 35+8+2 tests green **unmodified** + a real-browser pass), and a wasm size budget guards the **macro path** via a new `check:web` verb (the `wasm-budget` bin behind a `budget` feature keeps brotli out of the host graph). Macro output weighs **+475 B raw (~0.15%)** over hand-written; every budget red watched then restored green. No kill criteria hit |
| 18 | OS-integration spike I: macOS process-topology probe | 5 — OS spike | **done** — [plan](steps/step-18-os-topology-probe.md) · [report](steps/step-18-report.md) |
| 19 | OS-integration spike II: the Finder-citizen app | 5 — OS spike | **done** — [plan](steps/step-19-finder-citizen-app.md) · [report](steps/step-19-report.md) |
| 20 | OS-integration spike III: Linux/systemd re-confirmation probe | 5 — OS spike | **done** — [plan](steps/step-20-linux-systemd-probe.md) · [report](steps/step-20-report.md) |
| — | Topology design pass | design session | **done** — resolved as **D30–D33** (ARCHITECTURE **v1.9**): the daemon-owned topology is blessed (one store, one owner, every surface attaches; hybrids rejected as two canonicals), the wire is a generated values-only artifact — priced in [topology-wire-pricing.md](steps/artifacts/topology-wire-pricing.md), emitted later — lifecycle is OS-owned with "**on while any surface lives**" as the named steady state, and the `command` verb **graduates** as a scratch-draft transaction (DSL/core packaging wait for the first framework consumer). §9's process-topology and `command` bullets are closed; Phase 5's campaign is complete and `spikes/os-integration/` is **disposal-eligible** |
| 21 | Capability coverage: the capability is a checkout argument | 4 — Harness | **done** — [plan](steps/step-21-capability-coverage.md) · [report](steps/step-21-report.md); **D34** (ARCHITECTURE **v1.10**) shipped through the whole chain: each declared capability is an explicit *optional* parameter of the generated `checkout`/`restore`, the settable slot is deleted, a forgotten capability is a platform **compile error** (watched verbatim on Swift and Kotlin), a `nil` is a declared absence with C16 as its floor — and the emitted C16 test now proves wired-but-unrun still blocks. `Option<Box<dyn Trait>>` crosses boltffi on all three live backends. Tiers: `test:apple` 0 failures · `test:android` 80/0 (+app/gen) · `test:csharp` 14/14 (tripwire green, unrelated). The planned rung-3 `bolted-check` analysis **dissolved**; no kill criteria hit |
| 22 | `doctor`: the environment report for what mise cannot pin | 4 — Harness | **done** — [plan](steps/step-22-doctor.md) · [report](steps/step-22-report.md); the last missing verb of VISION's standard set ships as a pure-std `bolted-check` bin: per-tier, warn-never-fail (exit 0 under failure too, watched), 8 machine-checked rows + manual notes named rather than omitted. Drift-guarded inside `check` by the coverage manifest (every `mise.toml` task ↔ a doctor row or a recorded exemption, both directions) and the boltffi version cross-pin — five falsification reds watched. This machine: 8/8 ok. No kill criteria hit |
| 23 | boltffi git-pin to main: the C# resume, for real | 4 — Harness | **stopped on kill criterion 3** — [plan](steps/step-23-boltffi-git-pin-csharp-resume.md) · [report](steps/step-23-report.md); first step under the Fable-orchestrates model. M0 (the rev-parameterized pin machinery) banked on parked branch `step/23-boltffi-git-pin`; M1's verdict: **finding 06 FIXED at `23cf2ec`** (tripwire red for the right reason — the MarshalAs bug is dead) **but #654 regressed C# streams** (new finding 07: same-named `#[ffi_stream]` methods collapse across classes; draft stream silently lost; verified C-header/dylib/generated-C# + Swift-green cross-control). M2/M3 unbuilt; pin decision back to planning — resume path runs through an upstream 07 fix (kit entry drafted, owner files). bolted-http steps 24+ are C#-independent and do not wait |
| 24 | bolted-http I: the harness, the streaming verdict, the reference adapter | 4 — Harness | **done** — [plan](steps/step-24-http-harness.md) · [report](steps/step-24-report.md); **both freeze gates cleared**: row 16 = `ffi_stream` push (100/100 at 0.27.5, the 0.27.3 stall is dead), row 19 = Linux SPKI pinning feasible (real WebPKI + pins in `bolted-http-linux`). Contract types (MaybeSend seam, one-shot completion by construction), the conformance suite (13 C1 rows incl. two found-by-doing, C2 positive-control-per-key, generated C3), the reqwest reference adapter (suite-green; reqwest's default protocol-NACK retry found and disabled), and a 26-mutation pass (1 blind spot fixed, 2 survivors discharged as hypothesis-2). M1.5 inserted mid-step for two honestly-surfaced contract gaps. **Next: step 25 (S-AP, Apple adapter); the contract freeze follows it** (Henrik, 2026-07-19 — one more real implementor before commitment; agenda = report §Open questions + Apple's friction log) |
| 25 | bolted-http II: the Apple adapter (S-AP, macOS host tier) | 4 — Harness | **done** — [plan](steps/step-25-apple-adapter.md) · [report](steps/step-25-report.md); `BoltedHttp.swift` (delegate-driven URLSession) fully conformant — 15 C1 rows + 10 C2 keys + C3 column, every row watched red first; **A1: the F1 streaming verdict holds on Apple** (200/200 ordered/lossless; kill criterion not hit); A6 classic-loading sweep clean; 20-mutation pass → 2 genuine blind spots found and fixed (total-deadline needs a trickle fixture; the version observable had no control on any implementor). `PermissionDenied` honestly platform-gated. Headline freeze input: the `ffi_stream` subscription-lifecycle fragility (F-M3-1). **Next: step 26 (S-AN, Android); the contract-freeze design session follows it** (v1.15 re-scheduling, Henrik 2026-07-19 — Android is the last reachable implementor and its JNI stream probe feeds the streaming-seam question; agenda = step-24 + step-25 + step-26 report §Open questions) |
| 26 | bolted-http III: the Android adapter (S-AN, instrumented ART tier) | 4 — Harness | **done** — [plan](steps/step-26-android-adapter.md) · [report](steps/step-26-report.md); `BoltedHttp.kt` (OkHttp) fully conformant — 25 driver rows + C3 column, every row watched red first; **total deadline = `callTimeout`, honestly — no synthesis** (opposite of Apple); **N2: the JNI stream is lossless+ordered incl. under saturation** (step-02's ghost dead), but the generated `callbackFlow` drops on overflow (F-M0-4) and the abandoned-subscription leak reproduces shape-changed, native-side, GC-surviving (F-M0-5) — the freeze's streaming-seam evidence, three platforms deep; **NSC `<pin-set>` proven NOT to bind OkHttp** (§9 answered); HttpEngine spike-real/h3-paper; 22-mutation pass → 1 blind spot fixed (hop *order*), double-complete proven compile-impossible. **Next: the contract-freeze design session** (agenda = step-24/25/26 report §Open questions) |
| — | bolted-http contract review | design session | **done** — held 2026-07-21 as scheduled: **all ten open contract questions ruled** (streaming seam adopted as proposed, redirect-ceiling CFG, `content_length` advisory-by-protocol-arithmetic, push-cancellation, `PermissionDenied` device-tier, `HttpError → ErrorData` bridge, packaging conventions, conformance-scope boundary, mid-flight re-entry shape defined once, priority-hint uniformity + bridge-crate merge). Decision record: [contract-freeze-agenda.md](design/contract-freeze-agenda.md) · [streaming-seam.md](design/streaming-seam.md); ARCHITECTURE **v1.16**. Freeze framing softened (Henrik): working decisions for unreleased own-use software, expected to evolve; two standing re-eval triggers are upstream BoltFFI RFCs in draft (stream delivery contract, companion sources). Same day: **S-WIN unparked** — both C# blockers verified fixed at released 0.28.0 |
| 27 | bolted-http IV: the ruled contract, implemented | 4 — Harness | **done** — [plan](steps/step-27-ruled-contract.md) · [report](steps/step-27-report.md); the streaming seam shipped across all three adapters: core-owned bounded ring + seq check + completeness gate + terminal-exactly-once **by construction** (compile-proven, and across the FFI via the parked-sink registry), the one mid-flight signal (pushed pause/resume/cancel — **all three poll-watcher threads deleted**), rows 12/13 on mock+Linux+Apple+Android and row 14 (subscription hygiene) on Apple+Android, every row watched red per implementor; bridge crates merged (Q10, note-08 probe confirmed), redirect-ceiling CFG (core-counted on Linux; FFI legs ride native caps with structural classification, OkHttp text-match deleted), row-11 `total`, `Into<ErrorData>`; **F-M3-1 sidestepped in the shipped path** (synchronous `ChunkSink` re-entry — no live native consumer exists; row 14 counts a deterministic registry, not GC); 12-mutation pass → FFI bridge had zero host watchers (fixed test-side) + 1 recorded blind spot (Linux `Pause`-honouring; the mock's `IgnorePause` row watches the property). **Next: S-WIN C# resume; harness hardening; upstream filings (Henrik)** |
| 28 | Collection-facet spike: the first real collection facet + windowed-observation evidence | 6 — Collection spike | **done** — [plan](steps/step-28-collection-facet.md) · [report](steps/step-28-report.md); 17 W-rows, every one watched red in production code; zero frozen-crate changes; no kill criteria hit. Headline: **the machinery inherits, the host does not** — `Field`/`Draft`/`StoreDraft`/`commit_gates` reused byte-for-byte (C07 precedence inherited, proven by W10's wrong-order red) while `Store<D>` stays single-canonical and the collection is a **peer** container (registry loop re-implemented with one change: canonical looked up by `RowId`). Sharpest frozen-surface signal: `StoreDraft::from_canonical` is **identity-blind** in create-flow. `total_count` = filtered count (fork recorded); sync pull makes single-flight moot (proven, not built); naive re-projection **~0.65–0.69 ms p50** at 10k rows × 4 windows (KC3 ~1 ms clear, independently re-run). **Next: the windowed-collections design session** (agenda = report §Open questions) |
| 29 | S-WIN part I: the C# resume, at released 0.28.0 | 4 — Harness | **done** — [plan](steps/step-29-swin-csharp-resume.md) · [report](steps/step-29-report.md); both step-14/23 blockers **fixed in released registry 0.28.0** and re-verified **by execution**: the step-14 tripwire went red for the right reason (MarshalAs → out-param) then deleted, and the **parked probes came alive** (D23 `DraftClosed` refusal, D10 `[Pending, Passed]`, reentrant checker, `fillValid` create-flow); finding 07 (#697 distinct stream runtimes) confirmed in our shape — the two draft-stream rows that timed out at the git pin are green. Key contract: `run_username_check`'s bool means "a check RAN", not the verdict (read from the snapshot); Kotlin/Swift already encoded it right. **Emitted C# contract suite** (`emit_csharp_contract_suite`, D28 model): 22 C-IDs / 33 `[Test]` rows, values-only fixture, byte-drift-checked in `check`; `foreign_drift` rename landed. **Genericity catch:** C# joined the `golden.rs` family and the new arm caught a real bug on first run — the suite banner had frozen a profile verb into check-less `gen-note`; fixed feature-neutral. Falsification: genericity natural red, drift red on un-regenerated mutation, dotnet planted-red (exit 1 + TRX failed=1), lifecycle law verified on emitted C18 (Dispose-only, no GC). **Kit refreshed at 0.28.0** (06/07 → fixed-in-release; 08 runtime-probe done; #663 confirmed shipped; git-pin machinery obsolete). No kill criteria hit; `test:csharp` **53/53** (TRX). The C# **http** leg (spike-plan §5 W1/W2/W3) is NOT here — own later step, `SkipReason` verdict rides it |
| 29+ | **S-WIN part II: the C# http leg** (W1 .NET adapter probe, W2 conformance through FFI against the ruled contract, `SkipReason` keep-or-delete; after 29) · **harness hardening** (tier-provided sink path, row hard-kill, ALPN TestServer) · `bolted new` (gated on the first out-of-tree framework consumer / a publishing story) · the wire emitter (D31, gated on a product feature needing the daemon topology) · `command` in core/macros (D33, with its first framework consumer) | — | sketched — see Phases 4–5 |

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
- **Step 14 — C# port + generator. Stopped on kill criterion 1 ([report](steps/step-14-report.md)).**
  The tier is headless `dotnet test` **on this Mac** (the seam is host-portable; WinUI waits for
  Windows hardware — a non-goal, the step-07 KC4 precedent). **M0 delivered:** the toolchain seam
  (`pack:csharp`/`test:csharp`, dotnet task-scoped like the JDK), and the packed `net10.0`/`osx-arm64`
  NuGet artifact **loads and calls from `dotnet test`** (kill criterion 2 cleared). **M1 delivered:**
  the step-05 due-diligence probe (14 tests), which found that **three of the four load-bearing
  features run, and one is broken at runtime** — the async single-flight check driver
  `run_username_check` throws `MarshalDirectiveException` on every call, because boltffi 0.27.3's C#
  backend stamps `[return: MarshalAs(UnmanagedType.I1)]` (bool-return marshalling) onto the one
  `Result<bool,_>` verb, whose wire return is the `FfiBuf` struct. That is a **four-feature break**
  (callbacks unusable → C13/C16/D10 and `fillValid`'s create-flow check all unreachable), so the
  emitted suite, the genericity golden, and the falsification pass were **not built** — a conformance
  tier cannot honestly skip the async-check invariants to go green. The bug is in `dist/` bindgen
  output (kill criterion 5 — unfixable from our side); upstream draft is in the report, filing is a
  non-goal. **Findings banked for a §6/D26 design pass:** ARCHITECTURE §6's "Kotlin / C#: the GC never
  frees the Rust draft" is **wrong for C#** — the draft handle's finalizer reaches the store-side
  close (proven with a still-referenced control draft), which is D26's recorded revisit condition met;
  and step-05's H2 silent-UB hazard looks **dead** on C# — use-after-dispose is a typed
  `ObjectDisposedException`. ARCHITECTURE was left untouched. Resuming needs an upstream fix (or a
  pinned/patched boltffi) plus a design decision on §6/D26 and on whether C# belongs on the ladder
  before the driver works.

- **Step 15 — the boltffi 0.27.5 bump: resume C#, or prove why not.** Authored two days after step
  14 stopped, because upstream moved: boltffi shipped 0.27.4 (Jul 9) and 0.27.5 (Jul 10). Neither
  release note names the C# marshalling bug, but 0.27.4's #622 fixed the same *class* of defect
  (payload/envelope confusion in export signatures) and 0.27.5's #647 plausibly retires upstream
  draft 05 — so the question "is the driver fixed?" was empirical and cheap: the step-14 probe was
  built as the tripwire. **Outcome (branch B):** all five pins moved to 0.27.5 and every runnable
  tier stayed green (`test:apple:ui` env-blocked, not a regression); the tripwire
  `TheCheckDriverIsBrokenOnThisBackend` **stayed green — the C# driver is still broken at 0.27.5**
  (the `MarshalAs(I1)`-on-`FfiBuf` bug is byte-identical, confirmed in fresh generated source), so
  the emitted C# suite / genericity / falsification (M2/M3) were **not** built. The **upstream issue
  kit** (`upstream/boltffi/`) re-verified all six drafts: **01** (pack-android env) is **fixed** — the
  workaround was removed after a clean `nm` red/green control and a green `test:android` without it;
  **02, 03, 04, 06** are **alive → to file**; **05** (Result<Handle,E>) is **not reproducible** at
  either 0.27.3 or 0.27.5 across four faithful controls → **do-not-file** (contradicting the step-12
  report — a footnote for planning). **Nothing was posted; the owner files.** *(Since filed — 2026-07-15:
  PR #663 merged for 02, #664/#665/#666 open, PR #662 closed-resolved via the IR backend for 06, plus
  PR #657; current tracker is `upstream/boltffi/README.md`.)* Generated-surface churn
  was tiny (Swift/C# byte-identical; Kotlin +26 lines of additive `JNI_OnLoad` diagnostics), but
  `cargo install boltffi_cli --version 0.27.3` no longer builds without `--locked` (its sibling
  `boltffi_bindgen` floated to 0.27.5) — the recorded rollback fallback is compromised. The planning
  pass had also amended ARCHITECTURE to **v1.7** (§4/§6 per-backend release table, D26's revisit
  condition met and answered) — the step-14 findings, now law rather than banked evidence.

  When the tripwire eventually goes **red** (upstream fixes draft 06), resuming step-14's M2/M3 — the
  emitted C# contract suite, genericity, and the dotnet planted-red failure-mode proof — becomes its
  own step, still gated on that red. (Step 16 is now Phase 4's first harness step; the C# resume takes
  the next free number when the tripwire flips.)

## Phase 4 — Verification harness (unplanned sketch)

*(The founding ROADMAP titled this phase "Verification harness & Ring 0", citing "VISION Rings 0–2" —
a scheme from a pre-repo VISION draft that was never committed. The fossil is struck; VISION.md's
in-scope list is the authority.)*

`bolted-check` (binding drift, capability coverage, constraint semver snapshots, WASM size
budget), `bolted new` scaffolding, `doctor`, the standard mise verb set — per VISION.md's
in-scope list. Planned after Phase 3 ships evidence about what drifts in practice.

**The gate is discharged.** The `Feature`-trait design session (§9's "largest undischarged claim")
was resolved as **D29** (ARCHITECTURE v1.8): §1 is rewritten to the store-owned shape that shipped, the
unwritten trait is struck, and the never-built `command` verb is demoted to a §9 question. Phase 4
therefore **opened with step 16** — `bolted-check`'s first analysis, the **constraint-surface
snapshot** that D27 explicitly deferred here (a constraint change is now a reviewable, committed,
drift-checked diff that names the stash-schema-version duty, instead of one silent token inside a
regenerated file). **Shipped:** the tightening `PersonName max 30→29` fails the build at the exact
line while every existing drift check stays green (the bound never reaches the FFI layer they guard) —
see the [step-16 report](steps/step-16-report.md).

**Step 17 — the web shell onto `gen-profile` + the wasm size budget. Done**
([report](steps/step-17-report.md)). Two debts, one step, one new tier. `profile-web` was the last
shell still consuming the hand-written `spike-profile` (step 11's charter was the FFI shells; the
zero-FFI shell never moved), so §1's "Rust shells consume the contract directly" had never been
proven against *generated* code — and a size budget guarding the frozen spike crate would be
structurally blind to macro-output bloat, the one place web-target size risk actually lives. So:
migrated first (one dependency line + the two step-09 documented deltas — the availability tuple and
`Checked`/`ProfileCheck` for the three spike conveniences; **35 host + 8 wasm + 2 l10n tests green
unmodified**, plus a real-browser pass), then budgeted the framework path. The `wasm-budget` bin in
`bolted-check` (behind a `budget` cargo feature so brotli stays out of the host `check` graph), a
committed `wasm-budget.txt`, and a new **`check:web`** verb — release build + raw-wasm/brotli-wire
maxima at the migrated baseline × 1.10 (re-baselining a deliberate reviewed edit, never automatic —
D27). The tier is its own because step 04 decided it: `check` stays host-only, proven **indifferent**
to the budget's state (M4). The two one-time numbers: **drift since step-04** is +15 438 B raw wasm
(+4.95 %) on the pre-migration app (twelve steps of core evolution + toolchain float), and the
**spike-vs-gen delta** is **+475 B raw (~0.15 %)** — the first measurement of what macro output
*weighs* on the web target, and it is nearly nothing (the JS glue is byte-identical). Where the plan
was wrong: it named two import sites, but `tests/controller.rs` also imported the concrete crate, so
three files repointed (a crate-name swap, no behavioral edit); and it estimated "29" host tests where
there are 35.

**Step 21 — capability coverage. Done** ([report](steps/step-21-report.md)); the planning pass
resolved the design as **D34** (ARCHITECTURE v1.10) and the implementation shipped it through
generator, emitted suites, and all three FFI shells: coverage moved *into the generated surface* —
each declared capability is an explicit optional parameter of `checkout`/`restore`, so forgetting
one is a platform compile error (rung 2, watched verbatim on both compilers) and a `nil` is a
declared absence with C16 as its runtime floor. The rung-3 analysis the sketch imagined for
`bolted-check` dissolved (the D19/KC2 pattern): the uncovered state stopped being representable.

**Step 22 — `doctor`. Done** ([report](steps/step-22-report.md)): the last missing verb of
VISION's standard set — a read-only, per-tier environment report covering exactly what
`mise install` cannot pin (VISION risk 5 — Xcode, the Android SDK/NDK/system image, Chrome, the
cargo-installed boltffi CLI at its pinned version), warning instead of failing (exit 0 even
against a broken environment, watched), with the requirements knowledge guarded against drift by
two rung-3 pins inside `check`: the coverage manifest (every `mise.toml` task maps to a doctor
row or a recorded exemption, both directions — a new machine-bound verb without a doctor
decision fails the build) and the boltffi version cross-pin. No ARCHITECTURE change — doctor is
harness tooling, not contract.

**`bolted new` stays sketched and gains its gate**: scaffolding designed today would be designed
from zero out-of-tree consumers (no publishing story exists; every shell lives in-tree on path
deps) — the D20 error the wire emitter and `command` packaging already refuse. It becomes
current with the first external framework consumer. *(The Phase-4 sketch originally queued these before the OS-integration
spike; the spike was pulled forward as Phase 5 — see below — because capability coverage could
not be designed honestly before the spike showed what OS surfaces demand of capabilities. It
did: surfaces are heterogeneous, and a capability is the surface's own OS access — which is why
D34's parameter is optional, not mandatory.)*

**Sketched lint candidate — counter unit drift.** `LenChars` is defined in Unicode scalar values
(`chars().count()` in the macro expansion), but the shells' char counters count differently: Kotlin
`value.length` is UTF-16 code units, Swift `text.count` is grapheme clusters, only Leptos matches the
core. ASCII hides it; an emoji makes the counter disagree with the verdict (`"🇸🇪"` = 2 core / 4
Kotlin / 1 Swift). "What is a character" is a constraint semantic that leaked back into the shells
even though the numeric bound didn't. Fix direction: document scalar-values as part of the
`Constraint` contract and lint shell counters for `.length`/`.count` on a constrained field
(Kotlin wants `codePointCount`-style counting, Swift `unicodeScalars.count`).

**Sketched lint candidate — Kotlin draft-handle ownership.** On ART the GC never runs a Rust `Drop`
(step-05 H1), so a draft handle nobody `close()`s is a zombie the store rebases forever — and it pins
the whole `FfiState` graph plus the shell's checker object (often a captured Context: an Activity
leak, not just Rust bytes). D26 keeps close deterministic (no `Cleaner` backstop), which makes this an
invariant the host language cannot express — exactly the lint family above. Rule: every
`checkout()`/`restore()` result must flow into `use {}`, `ViewModel.addCloseable(...)`, or a field a
reachable teardown closes; a handle *reassignment* must close the previous handle (the profile VM's
`recheckout()` leaked one FFI handle per successful submit until the addCloseable pass fixed it —
proof the miss is easy even in the reference shell). The adopted idiom, applied to the profile VM:
register the teardown with `addCloseable` at the checkout itself, never in an `onCleared()` override.

## Phase 5 — OS-integration spike (campaign sketch)

The project's **second Phase-1-style campaign**, against VISION risk 2 — *"deep OS integration is
the roughest terrain … it must be spiked, not assumed"* — and ARCHITECTURE §9's process-topology
bullet: where the core runs (embedded vs daemon), whether the contract crosses a process boundary
(a sandboxed file-manager extension reaching a daemon-owned store), and single-instance ownership.
Phases 1–4 assume the core is in-process everywhere; VISION's product promise (daemon + tray +
file-manager badges over **one** core) breaks that assumption the day it is kept. The campaign's
deliverable is knowledge: hand-written probes, kill criteria, friction logs, then a design pass
amending ARCHITECTURE — packaging/installer chores fall out afterwards, they are not the question.

**Why before the remaining Phase-4 sketches:** capability coverage cannot be designed until the
spike shows what OS surfaces demand of capabilities; `doctor`/`bolted new` are independent and slot
in anywhere; the C# resume stays gated on its upstream tripwire regardless.

**Organization (decided in the step-18 planning pass, forward-only):** spike campaigns live under a
top-level `spikes/` root — `spikes/os-integration/` holds this campaign's crates (workspace
members), its Swift probe bits, and a README with charter + disposal criteria; everything in it is
deletable once its findings land in ARCHITECTURE. Campaign-1 crates (`spike-*`, `gen-*`,
`profile-web`) stay where they are: they graduated into harness fixtures (golden references, drift
subjects), and 17+ files including byte-checked generated fixtures hard-code their paths — a
retro-move is churn without payoff.

- **Step 18 — macOS process-topology probe. DONE — no kill criterion hit; every probe row
  executed** ([report](steps/step-18-report.md)). The daemon-owned topology is viable as drawn:
  the contract crossed the wire values-only (H1), the sandboxed Developer-ID client reached the
  app-group socket with zero prompts (R2 — the campaign's riskiest unknown, cleared), launchd
  owns activation/single-instance/respawn at rung 3 (R1), the stash survived a real daemon
  `kill -9` (H6), and the keystroke pair measured 26–45 µs against the 1.0 ms kill bar. Banked:
  the `command`-verb evidence (tier-2 rules are NOT free for session-less mutations), the
  wire-generator requirements (token registry, closed verdicts, tuple shapes), and the priced
  sandbox ceremony (incl. the `__info_plist` bundle-identity trap). Step 19 is now unblocked.
- **Step 19 — the Finder-citizen spike app. DONE — no kill criterion hit; every scripted row
  green** ([doc](steps/step-19-finder-citizen-app.md) · [report](steps/step-19-report.md)).
  The OS-spawned verdict cleared: a hand-assembled (no Xcode) Developer-ID .app/.appex is
  pluginkit-accepted, Finder-spawned into its own sandbox, and reaches the group socket (G3 +
  EPERM control). SMAppService registers the bundled socket-activated daemon with ZERO approval
  ceremony (R3); the SwiftUI editor runs the whole contract over the wire at ~100 µs/keystroke
  vs the 16 ms bar, incl. stash-restore across a real daemon kill -9 (C20 visible in a UI).
  Banked: connect(2) ≠ liveness under socket activation (open-then-verify), the two-client-shapes
  requirement, the continuous-stash idiom, the $HOME-in-SockPathName packaging wrinkle, and the
  idle-exit-vs-persistent-surfaces steady-state question. Step 20 is next.
- **Step 20 — Linux/systemd re-confirmation probe. DONE — no kill criterion hit; every row
  executed** ([plan](steps/step-20-linux-systemd-probe.md) ·
  [report](steps/step-20-report.md)). The topology stands on the second backend: the spike
  sources crossed **byte-unmodified** (13 suites / 32 tests on linux/arm64, incl. step 18's
  whole contract matrix), and the activation seam got *cheaper* — systemd's `LISTEN_FDS` env
  protocol needs zero foreign calls where launchd needed one; the adapters are asymmetric in
  kind, identical in shape (~40 lines/OS). Lifecycle rows L1–L5 all green in a systemd-PID-1
  container (activation, unit-identity single-instance, kill -9 respawn, idle-exit ⇄
  reactivation, H6 stash-restore). The headline verdict: **open-then-verify is portable**
  (connect ≠ liveness on either OS), but systemd accepted the queued post-kill connect in
  ~45 ms — the unaccepted-limbo pathology has only ever been seen under launchd. D4 keystroke
  pair 120.5 µs in-container vs the 1000 µs bar. Banked: the stale socket *file* survives
  socket-unit stop (`RemoveOnStop=` off by default); the user-unit posture priced on paper
  (zero approval ceremony, matching SMAppService's 0 prompts).
- **The topology design pass — DONE (ARCHITECTURE v1.9, D30–D33).** The campaign's capstone, run
  on the three reports' banked evidence. **D30**: one store, one owner — the daemon-owned topology
  is the second blessed deployment shape; when any surface leaves the app process, *every* surface
  attaches over the wire (a hybrid is two canonicals, i.e. a merge protocol, i.e. the perimeter).
  **D31**: the wire is a generated values-only artifact on the D22/D28 road — two client shapes,
  open-then-verify and continuous-stash mandatory in the generated client library; priced from the
  spike's measured line counts in
  [topology-wire-pricing.md](steps/artifacts/topology-wire-pricing.md), emitted as its own step
  when a product feature first needs the topology. **D32**: lifecycle is OS-owned at rung 3
  (socket activation, label/unit identity, idle-exit; no KeepAlive/Restart=always), with the
  steady state named: **on while any surface lives**. **D33**: the `command` verb graduates as a
  scratch-draft transaction (checkout → mutate → commit → close), closing the tier-2-bypass hazard
  by construction; macro/DSL support waits for the first framework consumer. With the findings now
  law, `spikes/os-integration/` meets its README's disposal criteria — deletable whenever the
  owner wants the ~7 s of `check` time back (the wire emitter step will *re-derive*, not rescue,
  anything it needs). Capability coverage (Phase 4) is unblocked: the spike has shown what OS
  surfaces demand of capabilities.

## Phase 6 — Collection-facet spike (campaign sketch)

**Why now.** §9's windowed-collections item has always said "designed when the first real
collection facet lands" — the D20/D29 rule refusing shapes justified by zero examples. The
v1.17 exploration pass (ARCHITECTURE status log) ran five scenarios against the preserved
candidate and turned the item's vague tail into four named questions: the store's canonical
shape (one canonical vs many), sort/filter as per-window query state, the `total_count` shape,
and re-projection etiquette. Step 28 builds the example those questions have been waiting for.

**Shape.** One step, disposable spike (`spikes/collection/`, workspace members like
`spikes/os-integration/` — disposal is `rm -rf` plus the member lines). The facet is
deliberately inbox-shaped: sync-driven mutation *and* editable rows *and* a second
differently-ordered observer, because a read-only top-N would validate almost nothing. Paging
and FFI crossing are out of scope; the frozen `bolted-core` surface is untouched — everything
new is spike-local. Drafts, create-flow, and orphaning are *inherited* (C07/C11/C12), watched
as positive controls, never redesigned.

**Exit.** A report whose §Findings is the agenda for the windowed-collections design session —
the ruling happens there, never in the spike. Like Phase 5, the campaign's output is law folded
into ARCHITECTURE; the spike itself is disposal-eligible afterwards.
