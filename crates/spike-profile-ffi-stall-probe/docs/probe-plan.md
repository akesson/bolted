# Step 02 — BoltFFI due-diligence probe (Apple)

> **Provenance.** Authored and run 2026-07-09 on the `design/core-evolution` branch as its
> `docs/steps/step-02-boltffi-probe.md`, against `bolted-core` as of `8ecc1c9` and BoltFFI
> 0.27.4. Main later ran its **own, independent** step 02
> ([plan](../../../docs/steps/step-02-boltffi-probe.md) ·
> [report](../../../docs/steps/step-02-report.md), `crates/spike-profile-ffi/`) with a
> different probe design and a different verdict. This plan was moved here on rebase so the
> two runs stay clearly separated — "step 02" below refers to THIS plan, not main's.

**Phase 1 · Spike.** Read first: [VISION.md](../../../docs/VISION.md) (risk #1 is this step's subject),
[ARCHITECTURE.md](../../../docs/ARCHITECTURE.md) §1 (observation contract), §5 (manifestation, reduce
loop), §6 (platform notes), [ROADMAP.md](../../../docs/ROADMAP.md) (working agreement), and
[step-01-report.md](../../../docs/steps/step-01-report.md) (what exists to export). The third cluster of this
step **already ran** (2026-07-09) as `crates/spike-http-ffi/` — findings in
[`crates/bolted-http/docs/spike-packaging-report.md`](../../bolted-http/docs/spike-packaging-report.md);
do not re-run it, inherit it (packaging layout, friction F1–F5, callback-trait numbers).

## Goal

Answer, with compiling code and passing Swift tests, whether BoltFFI can carry the design:

1. **Cluster 1 — the four load-bearing features** (VISION risk #1): classes with methods
   (draft handles), async streams (snapshots), `Result` methods with typed error enums,
   callback traits. The fourth is **already cleared** by the packaging spike — cite it.
2. **Cluster 2 — the observation contract** (ARCHITECTURE §1): how `Latest`/watch semantics
   map onto BoltFFI's stream machinery, burst behavior, window-scale payload cost, stream
   threading, and input→snapshot latency.

The exported feature is step-01's profile feature, wrapped — the wrapper is hand-written
as-if-generated: it is the first draft of what `bolted-ffi` will one day emit. Awkward
wrapper code is a design finding, not a nuisance — log it.

## Non-goals (hard boundaries)

- **No modification to `crates/bolted-core` or `crates/spike-profile`.** The wrapper adapts;
  it never reaches back. If an adaptation is impossible without a core change, that is a
  kill-criterion-adjacent finding: stop and report.
- No UI (step 03). No Kotlin/Android (step 05). No macro work. No re-run of the packaging /
  capability-callback cluster. No resolving ARCHITECTURE §9 OPEN questions — in particular
  the **threading contract for core entry** and **store concurrency** get *evidence* here,
  never a decision.
- The invariant tests (I1–I12) are step-01 territory; do not port them wholesale. This step
  proves the *boundary*, not the semantics again — a handful of end-to-end lifecycle tests
  suffices to show the semantics survive the crossing.

## Known terrain (from the packaging spike and BoltFFI 0.27 docs — verify, don't rediscover)

- Crate names must be underscore-only (`spike_profile_ffi`), or symbol generation breaks.
- Bundled SPM layout: `output` **is** the shipped package root; generated bindings are
  injected next to any hand-written sources under `wrapper_sources`.
- `#[data]` does not derive `Clone`; add it manually where the wrapper needs it.
- Exported classes must be `Send + Sync` (compile-time check); `&mut self` methods are
  rejected. `#[export(single_threaded)]` exists as an opt-out — probe its exact semantics
  (see probe C1d) but do **not** build the main wrapper on it.
- Streams: `#[ffi_stream(item = T)]` on a method returning `Arc<EventSubscription<T>>`
  (SPSC ring buffer, configurable capacity, default 256). Docs say **new events are dropped
  when the buffer is full** — drop-newest, producer never blocks. Modes: `async` (Swift
  `AsyncStream`), `callback`, `batch`.
- `Result<T, E>` with `#[error]` enums (payload variants included) → Swift `throws` with
  typed error enums carrying associated values.

## The structural collision this step must document (not solve)

Step-01's `Store`/`DraftHandle` are deliberately `Rc<RefCell>`-based — `!Send` — while
BoltFFI classes must be `Send + Sync`. The licensed wrapper shape: the FFI facade re-hosts
the store *plumbing* (not the semantics) thread-safely — `ProfileDraft` itself is plain data
and `Send`, so the facade owns draft slots behind `Arc<Mutex<Option<ProfileDraft>>>` with
**stable `u64` draft ids**, mirroring `Store`'s checkout/rebase/orphan/submit logic
(~50 lines). Field/draft semantics are still 100 % `bolted-core`/`spike-profile` code.
Record in the report: exactly what had to be re-hosted and why — this is the primary
evidence for the §9 store-concurrency decision and for replay's stable-identity
precondition. The `Mutex` here is spike plumbing, **not** the threading contract.

## Where

`crates/spike-profile-ffi-stall-probe/` — standalone (own `[workspace]`, outside the bolted workspace,
same pattern as `spike-http-ffi`), package name `spike_profile_ffi`, path-deps on
`bolted-core` and `spike-profile`. Bundled SPM layout into `package/`; a `consumer/` Swift
test package depends on `package/` as one dependency. `mise run check` (the workspace gate)
must remain green and untouched by this crate.

## Probes

### C1 — the four features (kill criterion lives here)

- **C1a Classes with methods (draft handles).** `#[export] impl ProfileFacet` with
  `new()`, `checkout() -> ProfileDraftFfi` (**a method returning another exported class** —
  load-bearing for draft handles), `apply_canonical(...)`, `delete_canonical()`,
  `version()`, `snapshot()`, `submit(...)`. `#[export] impl ProfileDraftFfi` with the
  monomorphic setters, `dirty_fields()`, `conflicts()`, `validate()`,
  `resolve_keep_mine/take_theirs(field)`, `status()`, the single-flight check drive, and its
  own snapshot stream (a draft is a mini facet — ARCHITECTURE §4). Probe whether an exported
  method can take an exported class as a **parameter** (`submit(draft)`); if not, fall back
  to `submit_by_id(u64)` and record it.
- **C1b `Result` + typed error enums.** Mirror `UsernameError` / `PersonNameError` /
  `EmailError` / `DateRangeError` as `#[error]` enums **with payload variants**
  (`TooShort { min, actual }`); assert in Swift that the thrown error is the typed case with
  the right associated values. Mirror `ValidationReport` as `#[data]` (nested structs,
  enum-typed fields, `Vec` of records — each a small probe of `#[data]` expressiveness).
  Export constraint metadata (`constraints_for(field) -> Vec<FfiConstraint>`, payload enum) —
  the "no constraint literals in shells" rule depends on this crossing.
- **C1c Async streams.** `#[ffi_stream]` snapshot streams on facet and draft (probe
  C2 details below). One stream in `callback` mode to observe the delivery thread directly.
- **C1d `#[export(single_threaded)]` side probe (small, bounded).** A trivial class wrapping
  `Rc<RefCell<u64>>`: does it compile, what does the generated Swift do (queue? assertion?
  nothing?), what happens on an off-main call. Inspect + record only — no load-bearing use.
- **C1e Callback traits — cleared.** Cite the packaging spike (protocol + weak back-ref
  wiring, ~8 ns/call, completions off-main). No new code.

**Kill criterion (unchanged from ROADMAP):** any of the four features missing or broken →
stop, report, architecture session before proceeding. "Broken" includes: a draft handle
cannot be returned as an object, typed error payloads don't survive the crossing, or no
stream mode can deliver snapshots reliably.

### C2 — the observation contract

The design's claim (§1): `Latest<T>` — read/await the newest value; coalescing always legal;
intermediates unobservable. BoltFFI's primitive is a **queue** with drop-newest overflow —
prima facie the *wrong* shape (a full buffer drops the newest value, which is the only one
`Latest` cares about). Probe what can be built from it:

- **C2a Burst.** `emit_burst(n)` publishes n=100 version-bumped snapshots with no consumer
  delay, against (i) a default-capacity (256) stream and (ii) a capacity-1 stream. Record
  per case: how many arrive, in what order, and **whether the final value always arrives**.
  The last point is the load-bearing one: if a capacity-1 stream can silently end on a stale
  value, a naive buffer-1 stream cannot implement `Latest`.
- **C2b The wake-and-read pattern.** A capacity-1 (or small) stream of wake events
  (version numbers — drops harmless by construction) + a `snapshot()` getter: consumer
  wakes, reads current truth. Assert: after any burst, the consumer's final read equals the
  final published snapshot. This is the candidate `Latest` encoding if C2a fails; measure
  its cost (wake + getter round-trip).
- **C2c Window payload.** `#[data] Row { id: u64, title: String, subtitle: String }`;
  `window_rows(offset, len) -> Vec<Row>` over a synthetic 10k collection. Measure a 50-row
  fetch in release mode, at scroll-refetch frequency. Budget context: ProMotion frame is
  8.3 ms; refetch is per-threshold, not per-frame (§6) — record the number, no pass/fail.
- **C2d Threading.** Which thread does a `callback`-mode stream fire on? Where does a Swift
  `for await` resume relative to the main thread? Record — evidence for the step-06
  threading-contract decision, alongside the packaging spike's off-main completion finding.
- **C2e Latency + rate.** Input→snapshot latency: `try_set` (or `apply_canonical`) →
  stream delivery, wall-clock. Sustained emission: emit at a fixed rate for a few seconds;
  record delivery rate and drops. And the keystroke measure: `try_set_username` round-trip
  (String in, typed `Result` out) per call in release — Apple-side analog of step-05's JNI
  kill criterion (no threshold here; record).

### Friction to watch for (log, don't fix)

`CheckToken` is opaque (private seq, no constructor) — the wrapper cannot round-trip it
through FFI as a value and must hold it internally keyed by its own id. Whatever shape that
forces is evidence for the replay precondition (stable logical identities for handles and
tokens). Same-name collisions between core types and `#[data]` mirrors; `Option<Profile>`
canonical vs `#[data]` optionality; `Date`/`DateRange` as nested `#[data]`.

## Deliverables

1. `crates/spike-profile-ffi-stall-probe/` — Rust wrapper + `package/` (bundled layout) + `consumer/`
   Swift test package, all probes as named tests (`testC1a…`, `testC2a…` or equivalent).
2. `docs/steps/step-02-report.md` — verdict table per probe; measurements; friction log;
   deviations; open questions routed onward (threading contract → step 06; anything Kotlin
   → step 05). The report must answer §1's standing question: **can `Latest` be expressed
   over BoltFFI, and as what** — that answer feeds the step-03 binding design and the
   response-streaming decision in `bolted-http`.
3. ROADMAP status update (02 → done; 03 → ready).

## Exit checklist

- [ ] All C1 probes pass (or the kill criterion fired and the report says so).
- [ ] C2 has empirical answers: burst table, a working `Latest` encoding (or "none — design
      session needed"), thread records, latency/keystroke/window numbers in release mode.
- [ ] `bolted-core` and `spike-profile` untouched; `mise run check` green.
- [ ] No `unwrap`/`expect`/`panic!` in the wrapper's library code (probe/bench helpers and
      the Swift test code excepted).
- [ ] Report written; ROADMAP updated.

## If you hit a wall

Smallest-reversible-choice rule (CLAUDE.md) applies to wrapper mechanics only. Anything that
would change a trait, an invariant, or ARCHITECTURE.md: stop, record in the report, leave it
for a design session. Kill criteria are real — if a load-bearing feature is missing, the
right output of this step is a report that says so, not a workaround.
