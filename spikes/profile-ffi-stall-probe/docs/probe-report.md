# Step 02 — BoltFFI due-diligence probe (Apple) — Report

> **Provenance.** Written 2026-07-09 on the `design/core-evolution` branch as its
> `docs/steps/step-02-report.md`, against `bolted-core` as of `8ecc1c9` and BoltFFI 0.27.4.
> Main later ran its **own, independent** step 02
> ([plan](../../../docs/steps/step-02-boltffi-probe.md) ·
> [report](../../../docs/steps/step-02-report.md), `crates/spike-profile-ffi/`) which
> confirmed all four features and did **not** observe the push-mode stall this run found.
>
> **Contradiction resolved on re-run (2026-07-15, during the rebase).** The stall lived in
> the stream template the **0.27.3 CLI** generated: a single-shot wake handshake
> (`handlePoll` + `processing` CAS) that drops Ready signals under concurrency. The
> **0.27.5 CLI** (what main pins) generates entirely different machinery — an eager
> background poll loop draining the ring via `popBatch` (batchSize 16) into an unbounded
> `AsyncStream`, exactly what main's report describes. Repacking THIS crate with 0.27.5 and
> re-running the probes: `incremental cap=256: converged=true revivedAfterNewPush=true`,
> `wake-and-read cap=1: finalRead=100 converged=true revived=true` — **the stall is gone;
> the kill criterion below is moot at boltffi ≥ 0.27.5.** §1 remains accurate for the
> 0.27.3-generated code and as the record of why push delivery was distrusted. All 15
> consumer tests pass after the re-run (two updated for the evolved core's C16
> username-check gate).
> Moved here on rebase so the parallel runs stay clearly separated; step references below
> are to the `design/core-evolution` roadmap of 2026-07-09, not main's current one.

**Status: done, with the kill criterion HIT on one of the four features.** Async streams
exist but their push delivery **stalls permanently** under concurrent load (a BoltFFI 0.27.4
bug, mechanism identified below). Everything else passed. Per the working agreement this is
a stop-and-report, not a workaround: **a design session must pick the snapshot-delivery
mechanism before step 03.** Two viable retreats are recorded with evidence.

- **Where:** `crates/spike-profile-ffi-stall-probe/` (standalone, outside the workspace; path-deps on
  `bolted-core` + `spike-profile`, both untouched). 15 Swift consumer tests, all passing
  (stream probes are *characterizing* — they record observed behavior in `PROBE` lines).
- **Toolchain:** boltffi 0.27.3 CLI / 0.27.4 crates, Rust 1.95, Swift 6.3.3, arm64 macOS.
- `mise run check` green; shared `Cargo.lock` unchanged.

## Verdict table

| Probe | Result |
|---|---|
| C1a classes with methods (draft handles) | **Yes** — incl. method *returning* an exported class (`checkout()`) and taking one as a *parameter* (`submit(draft:)`) |
| C1b `Result` + typed error enums | **Yes** — payload variants cross intact (`.tooShort(min: 3, actual: 2)`); nested `#[data]` records/vecs/enum-fields all work (full `ValidationReport` as data) |
| C1c async streams | **Broken in push modes** (async + callback): permanent, capacity-independent stall — kill criterion hit. Batch (pull) mode fully reliable |
| C1e callback traits | Cleared by the packaging spike (cited, not re-run) |
| C1d `#[export(single_threaded)]` | Compiles; generates **no guard whatsoever** — it only skips the `Send + Sync` check; safety is entirely by caller convention |
| C2 observation contract | Naive buffer-1 stream **cannot** be `Latest` (final value lost); wake-and-read also defeated by the stall; batch pull + `snapshot()` getter works today; callback traits are the other honest path |
| Measurements | `try_set` ≈ 1 µs/call, 50-row window ≈ 9 µs, input→snapshot ≈ 82 µs median — all noise at their frequencies |

## 1. The headline: BoltFFI push-mode streams stall permanently

**Observed** (probe outputs, debug; each against a fresh facet):

| Scenario | Result |
|---|---|
| Burst 100 into capacity-256, consumer attaches after | 100/100 delivered, in order |
| Burst 100 into capacity-1 | 2/100 delivered; **final value lost** |
| 100 incremental pushes, capacity-256 stream, live consumer | **15/100**, then stalled; a later push does NOT revive it |
| 100 incremental pushes, capacity-1 wake stream (wake-and-read) | 50 wakes consumed, final read = version 61/100; stalled; no revival |
| Batch (pull) mode, same bursts | 100/100 in order; later pushes remain visible; no stall possible |

**Mechanism** (from the generated Swift + `boltffi_core` source): the Rust side is sound —
the ring buffer, the poll that checks availability before parking, and the wake-latching
continuation slot are all correct. The bug is in the generated Swift drain loop
(`handlePoll`): it guards re-entrancy with a `processing` CAS and, when a `Ready` signal
arrives while a previous drain is still finishing, **discards the signal and never
re-registers the poll**. The stream's continuation is consumed; the ring still holds items;
subsequent pushes silently fill/drop (a failed push does not notify); the consumer starves
forever. Probability grows with push frequency — at UI snapshot rates this fires within
tens of emissions. The core's canonical state stays perfectly intact throughout; only
delivery dies. A minimal repro exists in this spike; worth filing upstream (the fix is
local: retry/flag instead of dropping the signal).

## 2. The `Latest` answer (ARCHITECTURE §1's standing question)

- **A naive buffer-1 stream is disqualified twice over**: BoltFFI's overflow policy is
  drop-*newest* (the only value `Latest` cares about is exactly the one dropped), and the
  stall kills even the arrives-eventually property.
- **Wake-and-read** (tiny wake stream + `snapshot()` getter) is the right *shape* — drops
  coalesce naturally because a full wake buffer implies a pending wake — but it rides the
  same broken push machinery today.
- **What works today, evidence in hand:**
  1. **Batch/pull mode** (`popBatch`): reliable, ordered, stall-free by construction — but
     needs a driver-side pull cadence (polling; against the taste of §1, fine for a spike).
  2. **Callback traits** (the capability machinery, proven at ~8 ns/call in the packaging
     spike): the driver registers an observer trait; the core pushes snapshots/wakes through
     it. Same generated machinery as capabilities, no ffi_stream involvement. The catch:
     calls arrive synchronously on the *producer's* thread — re-entering the core from
     inside one deadlocks (see §4) — so the shell-side glue must hop before touching
     anything. This is the same threading contract question step 06 already owns.
- **Recommendation for the design session:** treat snapshot delivery as *capability-shaped*
  (callback trait to the driver, driver owns the main-thread hop and `Latest` conflation)
  rather than *stream-shaped*, and track the upstream fix as the condition for revisiting
  `ffi_stream`. Decide there, not here.

## 3. Everything else in C1 passed — and generated pleasingly idiomatic Swift

- **Draft handles**: `checkout() -> ProfileDraftFfi` returns a real Swift object holding
  the core-side draft; the full lifecycle crossed the boundary intact — edit → live rebase
  from `applyCanonical` → per-field conflict (`yours`/`theirs` visible) → typed refusal on
  conflicted submit → `resolveKeepMine` → submit → canonical updated → handle `Consumed`,
  further use a typed `draftClosed`. Refused submits are non-destructive (the §8 decision,
  verified over FFI).
- **Typed errors**: `catch let e as FfiUsernameError { e == .tooShort(min: 3, actual: 2) }`
  works exactly as the errors-as-data rule wants. The whole `ValidationReport` (nested
  records, `Vec`s, enum-typed fields, key+params errors) crosses as `Hashable/Equatable/
  Sendable` Swift values. Constraint metadata crosses as a typed enum
  (`.lenChars(min: 3, max: 20)`) — the no-literals-in-shells rule is implementable.
- **Single-flight over FFI**: latest-begin-wins and stale-completion-ignored (I10) hold
  through the boundary; a pending check blocks `validate()` as data.
- **Object lifetime**: Swift `deinit` releases the Rust object (the store prunes dead
  weaks); no manual `close()` needed **on Apple** — the GC-language question (§9) stays
  open for step 05.

## 4. Threading evidence (for step 06 — not resolved here)

- Callback-mode stream callbacks fired on **background NSThreads** (never main), consistent
  with the packaging spike's completions finding.
- Stream drain and callback-trait calls can run **synchronously on the producer's thread**
  — i.e. potentially *while the core's locks are held*. Shell code re-entering the core
  from such a callback is a deadlock. The driver layer must own (a) the hop to main and
  (b) the no-re-entry rule; what the core promises callers is the step-06 contract.
- `#[export(single_threaded)]` is purely a compile-check opt-out — no queue, no assertion,
  nothing generated. Combined with Swift's any-thread `deinit`, it is unusable for the real
  store wiring; the `Send + Sync` wrapper path is the only credible one on 0.27.

## 5. Measurements (release; arm64 macOS)

| Measure | Result |
|---|---|
| no-op method | 4.9 ns/call |
| `try_set_username` valid (String in, sanitize+validate, snapshot republish) | 0.99 µs/call |
| `try_set_username` invalid (typed throw decoded) | 2.1 µs/call |
| `draft.snapshot()` (4 field views) | 0.71 µs/call |
| `facet.snapshot()` | 0.29 µs/call |
| `window_rows(50)` (50 rows with strings, of 10k) | 9.4 µs/call |
| `draft.validate()` (report as data) | 0.45 µs/call |
| input → snapshot delivery latency (wake stream, spaced inputs) | median 82 µs (min 26, max 236) |

The per-keystroke bet is a non-issue on Apple: 1 µs against a ~50 ms keystroke cadence.
(The JNI analog remains step 05's kill criterion.) Window refetch at 9 µs is invisible next
to the 8.3 ms frame budget. Note `try_set` here includes the wrapper's snapshot recompute +
compare + stream pushes — an honest end-to-end number, not a bare FFI call.

## 6. The store re-hosting (evidence for §9 store-concurrency + replay)

As licensed by the step doc: `Store`/`DraftHandle` (`Rc<RefCell>`, `!Send`) cannot back an
exported class, so the wrapper re-hosts the *plumbing* — `ProfileDraft` is plain data and
`Send`, held in `Arc<Mutex<Option<ProfileDraft>>>` slots with **stable u64 draft ids**, the
facade keeping weak refs (checkout/rebase/orphan/submit ≈ 60 lines, a line-for-line echo of
`Store`). Semantics stayed 100 % in core code; invariants I10–I12 and the rebase/conflict
behavior were re-verified through the boundary without touching them.

Evidence produced: (1) the FFI layer *forces* a concurrency decision the prototype deferred
— whatever step 06 picks (actor/serialized vs `Arc<Mutex>`), the prototype's `Rc<RefCell>`
cannot be it; (2) stable logical draft ids fell out naturally and are exactly replay's
precondition; (3) `submit`-consumes-via-`Option::take` worked cleanly where `Rc::try_unwrap`
could not (Swift may still hold the handle after submit).

## 7. Friction log

- **F1 — `CheckToken` cannot cross the FFI.** Opaque by design (private seq, no
  constructor), so the wrapper must hold tokens in a side table keyed by its own u64 ids.
  Fine once, wrong at scale: core token/handle types need FFI-representable stable
  identities (also the replay precondition). Freeze question.
- **F2 — closed-handle errors have no home.** Every mirrored error enum grew a `DraftClosed`
  variant the core knows nothing about. The real `bolted-ffi` needs one uniform
  closed/consumed-handle channel instead of polluting every domain error type.
- **F3 — all-primitive `#[data]` structs must be `Copy`** (blittable fast path); mixed
  structs must not be. Mechanical but undocumented; a macro-authoring concern for step 09.
- **F4 — `#[data]`/`#[error]` accept extra derives** (`Clone`, `PartialEq`) without fuss —
  pleasant, and needed, since `#[data]` derives neither.
- **F5 — publish-on-mutate needs discipline.** The wrapper republishes the draft snapshot
  on every mutating call (recompute + value-diff, §5's philosophy) — easy to forget a call
  site by hand; in generated code it must be structurally impossible to miss. (One honest
  gap kept: `base_version()` returns 0 on a consumed handle.)
- **F6 — the streams' SPSC constraint is invisible in the API.** Each `#[ffi_stream]`
  method returns the same `Arc<EventSubscription>`; a second Swift-side subscription would
  silently violate single-consumer. Nothing stops the caller. Multi-observer facets (§1:
  tray + window) need per-observer subscriptions by construction.
- (Inherited F-numbers from the packaging spike — underscore crate names, bundled-layout
  `output` semantics — applied cleanly; no new packaging friction.)

## 8. Open questions routed onward

- **→ design session (pre-step-03, the kill-criterion item):** snapshot delivery mechanism
  — callback-trait push vs batch-mode pull vs wait-for-upstream-fix; and whether to file
  the BoltFFI stall bug upstream now (minimal repro ready in this spike).
- **→ step 06:** the core threading contract (this report §4 + packaging spike §3):
  non-main entry, no-re-entry-from-callbacks, who owns the main hop. Store concurrency
  (§6 evidence). `CheckToken`/handle identity as contract-level FFI-representable ids (F1).
  Closed-handle error channel (F2).
- **→ step 05 (Kotlin):** same probes on JNI — especially whether the stream stall
  reproduces in the Kotlin bindings, per-keystroke `try_set`, and handle lifecycle without
  deterministic `deinit`.
- **→ bolted-http contract freeze:** response streaming should NOT enter the portable core
  on current evidence — `ffi_stream` push modes are unreliable, and the contract would be
  betting on them (`crates/bolted-http/docs/architecture.md` §5 records this as the open
  decision; the request/response one-shot shape is unaffected).

## Exit checklist

- [x] C1 probes: all pass except C1c push modes — kill criterion recorded, evidence above.
- [x] C2: burst table, `Latest` answer (with the working encoding and the recommendation),
      thread records, latency/keystroke/window numbers in release.
- [x] `bolted-core` / `spike-profile` untouched; `mise run check` green.
- [x] No `unwrap`/`expect`/`panic!` in wrapper library code (poison-safe locking).
- [x] ROADMAP updated (02 → done with kill-criterion note).
