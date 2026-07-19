# Step 25 M3 — streaming + sweeps (notes for M4 / the freeze)

**Milestone:** M3 (A1 streaming probe, A6 classic-loading-mode sweep, A5 priority acceptance).
**Branch:** `step/25-apple-adapter`. Scope: the A1 probe (probe-grade — NO contract surface added;
the streaming core-seam stays unfrozen, freeze-agenda Q2), the A6 `usesClassicLoadingMode = false`
sweep, A5 priority acceptance-only. M4 (mutation pass) is untouched.

## Gate result

- `mise run check` — green (host, Xcode-free; workspace clippy `-D warnings`, the apple-ffi crate
  and the conformance server change clean, all crate tests pass).
- `mise run test:apple:http` — green: **9 XCTest methods, 0 failures** (the 5 M0–M2 methods + the 4
  M3 methods below). Stable across repeated runs, including under full 14-core CPU saturation
  (3/3 green loaded, after the teardown fix — see F-M3-1).

New M3 test methods:
- `testA1StreamingProbeIsOrderedLosslessComplete` — F1 `ffi_stream` push, both pacings.
- `testA1CorruptionControlDetectsLoss` — the A1 watched-red (drops one chunk; the probe detects it).
- `testA5PriorityAcceptanceOnTheTask` — the priority→`task.priority` mapping + task carries it.
- `testA6ClassicLoadingModeSweep` — the whole suite + the A1 probe under `usesClassicLoadingMode=false`.

## A1 — the verdict paragraph (for the freeze session)

**The step-24 F1 verdict HOLDS on the Apple path.** A streamed response delivered through the
S-FFI-chosen mechanism — F1 `ffi_stream` async push, surfaced Swift-side as `AsyncStream<Chunk>` —
inside a real http round-trip is **ordered, lossless, and complete** on the Apple host tier. A
URLSession **delegate** consumer (`didReceive data`, on the session's own serial `OperationQueue`)
reads the harness test server's new `/chunked` endpoint (200 chunks, `Transfer-Encoding: chunked`,
real per-chunk flushes), splits `chunk-NNNNNN` lines, and pushes each across the FFI into
`HttpHarness::deliver_chunk`; a live Swift consumer attached **before** delivery drains
`chunkStream()`. For both pacings — burst (`delay=0µs`) and paced (`delay=200µs`) — the consumer
received a contiguous 1…200 with **no gap and no reorder**: `delivered=200/200`, `ingested=200/200`,
`stallPoint=200`, `ordered=true`. The step-02 stall (15/100 on 0.27.3) did **not** reappear at
0.27.5. Threading: the consumer always resumed **off the main thread** (`consumerOffMain=true`), on
the Swift concurrency pool, while the producer pushed from the URLSession delegate queue — the F1
async hop puts re-delivery on a different execution context from the synchronous producer push, so
feeding each chunk back into the core as an input carries no producer-thread re-entrancy hazard
(the exact F1 rationale the step-24 verdict rests on). Latency is tens of µs (p50 ≈ 24–29µs, p99 ≈
130–200µs) — invisible against any network chunk cadence. **Caveat (F-M3-1):** F1's reliability here
is conditional on **deterministic per-run teardown** of the stream; without it, running several
`ffi_stream` consumers in one process leaks stalled/dead subscriptions into the shared streaming
runtime and a later consumer stalls (partial delivery, up to the wait cap) — `ingested` stayed 200
throughout, so this is a **re-delivery/runtime lifecycle** fragility, never a lost-in-transit or
reorder failure. Kill-criterion 3 was therefore **not** hit (no stall/reorder in the frozen path);
the lifecycle caveat is freeze-agenda input for the streaming core-seam design.

### Per-shape numbers (representative green run; µs vary run-to-run by tens of µs)

| Shape | delivered | ingested | stallPoint | ordered | p50 | p99 | wall | consumerOffMain |
|---|---|---|---|---|---|---|---|---|
| F1 `ffi_stream`, delay=0µs (burst) | **200/200** | 200 | 200 | yes | 23.8µs | 132.9µs | 3.23ms | yes |
| F1 `ffi_stream`, delay=200µs (paced) | **200/200** | 200 | 200 | yes | 28.6µs | 197.7µs | 43.67ms | yes |
| A1 CONTROL (drop seq=100) | 199/200 | 199 | **99** | yes | — | — | — | — |

`ingested` = chunks that entered the core via `deliver_chunk` (the completeness numerator source of
truth; whole in every case ⇒ the http round-trip + cross-FFI ingest are whole). `stallPoint` =
highest N with 1…N all received.

### The A1 control (watched-red — a probe that cannot fail proves nothing)

`testA1CorruptionControlDetectsLoss` drops chunk seq=100 **before** it crosses the FFI, then asserts
the completeness check DETECTS the loss: `delivered=199/200`, `stallPoint=99` (exactly one before the
dropped seq), `ingested=199`. So the ordered/lossless/complete assertions are non-vacuous — they go
red on real loss.

## A6 — the classic-loading-mode sweep

Ran the ENTIRE suite (C1 + extra rows + C2 + C3) **and** the A1 probe with
`usesClassicLoadingMode = false` on the session configuration, comparing row-by-row to the OS-default
baseline. **A single flag on the adapter/session factory** (`BoltedHttp(classicLoading:)` +
`StreamingProducer(classicLoading:)`), the adapter is **not** forked.

**Divergence: NONE.** Every one of the 23 suite rows kept its status under classic-off, and the C3
Apple column is byte-identical:

| Sweep target | default (OS) | classic-off | divergence |
|---|---|---|---|
| C1 rows (11) | all GREEN | all GREEN | none |
| C1 extra rows (2: sink correspondence, redirect trace) | GREEN | GREEN | none |
| C2 keys (10 reachable) | all GREEN | all GREEN | none |
| C3 Apple column (priority-hint present, metrics Phase) | pinned | identical | none |
| A1 probe (F1, burst) | 200/200 | 200/200 | none |

`testA6ClassicLoadingModeSweep` asserts the sweep is all-green AND the C3 column matches AND
`divergences == 0`. The A1 default-path completeness is asserted **strictly**; the A1 classic-off
completeness is **recorded** (not hard-asserted), per kill-criterion 3's "record and continue" — with
`ingested` vs `delivered` logged to localise any future divergence to the URLSession producer vs the
F1 re-delivery. Ordering is asserted even under classic-off (a reorder would be graver than loss).

**Availability note:** `usesClassicLoadingMode` is macOS 15.4+ / iOS 18.4+ (the compiler rejected the
13.1 guess). On the test host (macOS 26.5) the flag takes effect at runtime, so the sweep is a real
false-vs-default comparison. On a host below 15.4 the guard would silently no-op and the sweep would
compare default-vs-default — non-gating for this step's host tier, flagged for the iOS device tier.

## A5 — priority acceptance (acceptance-only; the wire is FLAGGED lore, NOT tested)

Added `FfiRequest.priority` (`FfiPriority` mirroring the contract `Priority`: Throttled/Low/Normal/
High/Critical), mapped in `to_ffi_request` (absent ⇒ Normal — the hint data rides every request).
The adapter maps it to `URLSessionTask.priority` via `BoltedHttp.taskPriority(for:)` and sets it on
the task before `resume()`, recording the applied value (`lastTaskPriority`) for the assertion. The
five contract levels fold onto URLSession's three **named** platform constants (no magic priority
numbers): Throttled/Low → `lowPriority`, Normal → `defaultPriority`, High/Critical → `highPriority`.

Evidence (`testA5PriorityAcceptanceOnTheTask`):
- Mapping (non-vacuous — distinguishes levels): High/Critical → `highPriority`, Normal →
  `defaultPriority`, Low/Throttled → `lowPriority`; `taskPriority(.high) != taskPriority(.low)`.
- Acceptance on the task: a real request built with `.high` yields `lastTaskPriority == highPriority`
  (`0.75`), and **not** the URLSession `defaultPriority` (`0.5`) an adapter that never set the
  priority would leave — that default-vs-High gap is the watched-red (the assertion can fail).
- The C3 Apple column's `priority-hint present` (M2's marker trait) is now backed by real wiring.
- The RFC 9218 wire observation is **not** tested (FLAGGED lore, per the step).

## What M3 built

**Conformance test server (`crates/bolted-http/src/conformance/server.rs`)** — additive, behind the
`conformance` feature, std-only:
- `/chunked?count=N&delay_us=U` — a `Transfer-Encoding: chunked` body of N `chunk-NNNNNN\n` lines,
  each flushed with `delay_us` between them (0 = burst). Mirrors the step-24 S-FFI probe's chunk
  server, now hosted in the harness-owned test server (the "test server's chunked endpoint" the step
  calls for). No existing endpoint or row is touched.

**FFI crate (`crates/bolted-http-apple-ffi/src/lib.rs`)** — additive mirror growth:
- `FfiPriority` (mirrors `Priority`) + `FfiRequest.priority` + the `to_ffi_request` mapping (A5).
- `Chunk` `#[data]` (seq/bytes/t_send_ns/last) — the A1 probe chunk (mirrors the S-FFI `Chunk`).
- `HttpHarness` grew a `chunk_stream: Arc<EventSubscription<Chunk>>` (capacity 1024 — well above any
  probe's count so the SPSC ring never drops) + a `chunk_ingested` counter, and the methods
  `deliver_chunk`, `chunk_ingested`, `#[ffi_stream(item = Chunk)] chunk_stream`, and
  `close_chunk_stream` (deterministic per-run teardown — F-M3-1).

**Swift adapter (`apple/bolted-http/Sources/BoltedHttp/BoltedHttp.swift`)**:
- A5: `taskPriority(for:)` (public static) + `task.priority = …` before resume + `lastTaskPriority`.
- A6: `init(classicLoading: Bool?)` (designated) with the no-arg `convenience init()` delegating;
  `usesClassicLoadingMode` set under `#available(macOS 15.4, iOS 18.4, *)`. One flag; not forked.

**Test target (`apple/bolted-http-conformance/…/ConformanceTests.swift`)**: the 4 M3 methods, plus a
`StreamingProducer` (URLSession delegate consumer on its own OperationQueue) and a `StreamCollector`.

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **The `/chunked` endpoint lives in the harness-owned conformance test server**, not a
   probe-private listener. The step calls for "the test server's chunked/delayed endpoints"; the
   server had none, so it gained one (additive, conformance-feature-only, std-only). Keeps the A1
   round-trip going through the same server the rest of the suite uses.
2. **The A1 producer is a URLSession delegate consumer (`didReceive data`), not `bytes(for:).lines`.**
   The delegate runs on the session's own `OperationQueue` — a dedicated thread OFF the Swift
   cooperative pool the `ffi_stream` consumer resumes on — so producer and consumer never contend
   (the `bytes(...)` async producer shared that pool and starved the consumer under load). It is also
   the step's primary phrasing ("delegate `didReceive data` chunks cross the FFI").
3. **200 chunks, ring capacity 1024.** "Many chunks" (2× the S-FFI envelope) with 5× ring headroom so
   the ring never drops even if the consumer lags the burst — the probe measures real completeness,
   not ring pressure. Both are probe internals, not contract constraints.
4. **The A1 classic-off completeness is recorded, not hard-asserted** (the default path is strict).
   Kill-criterion 3 says a stall is a finding to log and continue, not a gate failure; the default
   (frozen) path is the load-bearing A1 result and it is asserted whole.
5. **`FfiPriority` folds 5 contract levels onto URLSession's 3 named priority buckets.** URLSession
   exposes exactly low/default/high; Throttled≈Low and Critical≈High is the honest coarsening. No
   information is invented (the finer contract levels are a hint the platform cannot express).
6. **`FfiRequest.priority` appended as the last field.** No Swift site constructs `FfiRequest` except
   the new A5 test, so field position is free; appending keeps the M2 mirror diff minimal.

## Friction log (freeze-agenda input — friction matters more than usual this step)

- **F-M3-1 — the `ffi_stream` consumer needs explicit per-run teardown or it stalls under load.**
  Running several F1 streams in one process, a stalled/never-cancelled consumer (an `AsyncStream`
  `for await` suspended awaiting an element that stopped arriving) leaves a **live subscription** in
  the shared streaming runtime; a later run's consumer then starves and delivers only partially (up
  to the wait cap). `ingested` stayed 200 throughout — this is a **re-delivery lifecycle** fragility,
  not lost-in-transit and never a reorder. Fixed here with `close_chunk_stream()` (unsubscribe) +
  `await consumer.value` after each run; then 200/200 held even under full 14-core saturation.
  **Freeze question:** the streaming core-seam contract must specify a consumer/end-of-stream
  lifecycle (who closes, and that a completed OR abandoned stream is promptly unsubscribed) — a
  `Drop`/scope-bound subscription would remove this footgun for every native adapter, not just Apple.
  This is the concrete shape of the step-24 verdict's open "how a chunk re-enters the core /
  end-of-body signal" caveat.
- **F-M3-2 — `consumerHopsOffProducer` is not always `true`, but `consumerOffMain` always is.** The
  F1 async pool sometimes reuses a thread the producer also touched (thread-identity reuse), so the
  consumer/producer thread SETS occasionally intersect. This is not synchronous producer-thread
  delivery (that is F2's hazard); F1 is an async push and the consumer resumes off the producer's
  synchronous call stack regardless of which pooled thread it lands on. The load-bearing threading
  fact is `consumerOffMain=true` (always) + async hop, not disjoint thread identities.
- **F-M3-3 — `usesClassicLoadingMode` availability is macOS 15.4+, not the 13.x range one might
  guess.** The setter is gated to macOS 15.4 / iOS 18.4. Below that the A6 flag silently no-ops (the
  sweep degrades to default-vs-default). Non-gating on the macOS 26.5 host tier; relevant if the iOS
  device tier targets an older floor. No contract impact (A6 is a regression guard, not a contract row).
- **F-M3-4 — five priority levels, three platform buckets (carried from the CAP decision).**
  URLSession cannot express Throttled-vs-Low or High-vs-Critical distinctions; the mapping coarsens.
  The contract keeping 5 levels is fine (it is a hint), but the freeze may note that only 3 are
  observable on Apple (and likely elsewhere), so conformance can only ever accept the coarse bucket —
  acceptance-only was the right call.

## M4 hand-off (the mutation pass)

- The A1 probe is probe-grade and adds no contract surface; M4's mutation focus stays on the M2
  syntheses (pinning, deadline, hop trace, cancel, progress, file-sink finalize) per the M2 notes.
- A5's mapping is a candidate for a small mutation (swap High/Low buckets ⇒ the acceptance assertion
  reds) if the mutation table wants an A5 row; the watched-red control is already in.
- A6 asserts zero divergence; a mutation that made the adapter behave differently under classic-off
  would surface as an A6 divergence — the sweep is a live regression guard.
