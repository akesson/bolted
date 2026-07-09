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
| 06 | Design freeze | 2 — Freeze | **ready** — all Phase-1 evidence in; §9's `close()` question answered by step 05 |
| 07 | Kotlin/Compose spike app | 2 — Freeze | pending |
| 08 | Extract bolted-core + conformance suite | 3 — Extraction | pending |
| 09 | bolted-macros | 3 — Extraction | pending |
| 10 | bolted-ffi + regenerate Swift/Kotlin | 3 — Extraction | pending |
| 11 | C# port + generator | 3 — Extraction | pending |
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

- **Step 06 — Design freeze.** A planning session, not an implementation session: reconcile
  all friction logs and probe reports (step-01's F1–F7/Q1–Q6 are recorded: decisions in
  ARCHITECTURE §8, the rest deferred into §9); resolve the OPEN questions in ARCHITECTURE.md §9
  (draft lifecycle/`close()`, echo rule confirmation, stash/restore design, one-shot
  events); update ARCHITECTURE.md to "frozen" status; promote the invariant tests to the
  named conformance suite.
- **Step 07 — Kotlin/Compose spike app.** The risks only a real Android app exercises:
  process death mid-draft (stash/restore), configuration change (draft handle scoping),
  main-thread snapshot delivery. Doubles as the hand-written "generated code" reference for
  Kotlin.

## Phase 3 — Framework extraction

Extract from evidence, in dependency order; the hand-written spike code becomes the golden
reference the generated code is diffed against.

- **Step 08 — Extract `bolted-core`** generics + conformance suite as a reusable crate
  (decide the store concurrency model here).
- **Step 09 — `bolted-macros`** (`value`, `entity`, `rules`, `feature_model`); macro output
  must reproduce the hand-written spike code (golden tests).
- **Step 10 — `bolted-ffi`** (only crate importing boltffi) + regenerate the Swift and Kotlin
  spike apps from macros; per-language contract tests from the conformance suite.
- **Step 11 — C# port + generator.** Hand-write the C# client first (IDisposable ergonomics,
  WinUI binding shape), then the generator template.

## Phase 4 — Verification harness & Ring 0 (unplanned sketch)

`bolted-check` (binding drift, capability coverage, constraint semver snapshots, WASM size
budget), `bolted new` scaffolding, `doctor`, the standard mise verb set — per VISION.md's
in-scope list. Planned after Phase 3 ships evidence about what drifts in practice.
