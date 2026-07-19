# S-FFI — the response-streaming mechanism verdict (step-24 M2)

**Status:** done. **Verdict: row 16 (response streaming) stays `CORE`; the FFI gate is CLEARED.
Recommended mechanism: F1 — `ffi_stream` async push.** The legal `Memory | File` fallback is
**not** needed: all three push/pull shapes deliver 100/100 with no stall at boltffi 0.27.5 inside
a real http round-trip.

- **What this gates:** feature-matrix §5.11 / row 16 — "Response streaming (chunked delivery)",
  marked `CORE, gated` with the gate being "FFI mechanism at boltffi ≥0.27.5". This probe is
  that gate.
- **Where:** `crates/spike-http-ffi` — the packaging spike's http round-trip, extended with a
  `StreamProbe` class + `Chunk` `#[data]` + `ChunkSink` `#[export]` trait + a localhost chunked
  HTTP server (`src/lib.rs`), driven by `consumer/Tests/.../StreamMechanismTests.swift`.
- **Toolchain (release/git distinction kept honest):** bindings generated **and** packed with
  the **registry release** `boltffi_cli 0.27.5` (`cargo install boltffi_cli --version 0.27.5
  --locked --root <scratch>` into a canonicalized scratch `CARGO_HOME` — the askama
  symlinked-CARGO_HOME workaround; verified `cargo install --list` shows `boltffi_cli v0.27.5:`
  with **no** git-rev, vs the machine-global `~/.cargo/bin/boltffi` which is the killed git rev
  `23cf2ecc`). Runtime crates pinned to **registry** `boltffi 0.27.5` in the crate's own
  lockfile (non-workspace; `cargo update -p boltffi --precise 0.27.5`). The killed step-23 git
  pin was never touched. Swift 6 / arm64 macOS, `swift test -c release`.

## The shape under test

The exact bolted-http response-streaming flow, end to end:

```
localhost HTTP/1.1 server (Transfer-Encoding: chunked, 100 chunks, one `chunk-NNNNNN\n` per
  HTTP chunk, flushed with an inter-chunk delay)
    → URLSession.bytes(from:).lines consumes it on the FOREIGN (Swift) side  ← real http round-trip
      → the adapter pushes each line ACROSS the FFI into StreamProbe (deliver_fN)
        → the core re-delivers to a LIVE Swift consumer via mechanism F1 / F2 / F3
```

This is the step-02 stall shape (`testC2bIncrementalDefaultCapacityStallProbe`: incremental
pushes into a capacity-256 stream with a live consumer — the case that delivered **15/100** then
stalled forever on the 0.27.3-generated Swift drain loop), but now the chunks are **real bytes
from a real local HTTP response**, not synthetic `emit_burst` events. Each chunk carries a
Swift-stamped `t_send_ns`, so per-chunk delivery latency is measured on one clock.

Two pacings per mechanism: `delay=0µs` (server writes all 100 back-to-back — max drain-loop
stress, closest to the original 15/100 repro) and `delay=200µs` (paced, ~a streamed body).

## Per-mechanism results (release, arm64 macOS; representative single run)

| Mechanism | pacing | delivered | ingested | stall point | p50 | p99 | wall (first→last recv) | delivery thread |
|---|---|---|---|---|---|---|---|---|
| **F1** `ffi_stream` async push | 0µs | **100/100** | 100 | none (100) | 188.6µs | 247.1µs | 0.30ms | off-main (Swift concurrency pool) |
| **F1** `ffi_stream` async push | 200µs | **100/100** | 100 | none (100) | 22.0µs | 445.4µs | 16.08ms | off-main |
| **F2** callback-trait push | 0µs | **100/100** | 100 | none (100) | 1.1µs | 17.7µs | 0.61ms | off-main, **producer (adapter) thread** |
| **F2** callback-trait push | 200µs | **100/100** | 100 | none (100) | 2.4µs | 21.5µs | 17.95ms | off-main, **producer thread** |
| **F3** wake-and-read pull | 0µs | **100/100** | 100 | none (100) | 378.7µs | 547.7µs | 1.05ms | off-main |
| **F3** wake-and-read pull | 200µs | **100/100** | 100 | none (100) | 32.8µs | 287.7µs | 16.49ms | off-main |

`stall point = none (100)` means the consumer received a contiguous 1…100 with no gap.
`ingested` (chunks that entered the core, the completeness source-of-truth) equals 100 in every
case, so the http round-trip and the cross-FFI ingest are whole; the table's question is purely
whether re-delivery to the live consumer is whole — **it is, for all three.**

### Reading the numbers

- **The 15/100 stall is gone in the http context too.** F1 is the exact machinery that stalled
  on 0.27.3; at 0.27.5 it delivers 100/100 under both the burst (`delay=0`) and paced cases.
  This confirms, inside a genuine http round-trip, what the step-02 re-run found synthetically:
  the 0.27.5 CLI's eager `popBatch`-drain-loop template fixed the drop-the-Ready-signal bug.
- **F1's `delay=0` p50 (188µs) > its `delay=200` p50 (22µs)** is queueing, not a defect: with no
  pacing all 100 land in the ring almost at once and the tail waits its turn to be drained;
  with pacing each chunk is drained promptly (low p50) at the cost of occasional scheduler
  spikes (higher p99). Both are microseconds against any network chunk cadence.
- **F2 is fastest (~1–2µs p50)** because `on_chunk` is a *synchronous* call with no ring buffer
  and no async hop — but that is the same property the step-02 report §4 flagged as a hazard:
  the callback runs **on the producer's thread** (here the adapter's byte-reading thread), so in
  real bolted-http, where each chunk re-enters the core as an input, the driver must hop threads
  before touching core state or it risks re-entrancy/deadlock. Speed bought with a caution.
- **F3 is the highest-latency** (wake + `drain_f3()` round-trip) and is the *coalescing* shape:
  a full capacity-1 wake buffer drops newest, which is correct for `Latest` snapshots but is the
  wrong instinct for a chunk stream where **every** chunk must survive. It still delivered 100/100
  here only because `drain_f3()` returns the whole buffered backlog per wake (drops coalesce
  *wakes*, never chunks) plus a final post-loop drain — i.e. it works, but it is doing pull-style
  batching wearing a wake coat.

## Recommendation for row 16: **F1 — `ffi_stream` async push (async mode)**

Response streaming wants **ordered, lossless, every-chunk** delivery — the opposite of the
coalescing `Latest` that snapshot delivery wants. Against that requirement:

1. **F1 is the right shape and it is now reliable.** It surfaces as a Swift `AsyncStream<Chunk>`
   — the idiomatic streamed-response-body type — preserves order, drops nothing under capacity,
   and (the whole point of this probe) no longer stalls at 0.27.5. Its built-in async hop means
   the consumer resumes **off** the producer thread, so re-entering the core to feed each chunk
   in as an input carries no re-entrancy hazard. Latency is tens-to-low-hundreds of µs —
   invisible next to network chunk arrival.
2. **F2 (callback-trait) is the performance alternative, not the default.** ~1–2µs and it reuses
   the exact capability machinery `HttpAdapter` already rides — but its synchronous
   producer-thread delivery is the step-02 §4 deadlock caution. Reserve it for paths where the
   driver demonstrably owns the thread hop and the low-latency matters; do not make it the
   contract's response-body carrier.
3. **F3 (wake-and-read) is for snapshots, not bodies.** Its coalescing is a feature for `Latest`
   observation and a mismatch for chunk streams. Keep it in the snapshot-delivery toolbox
   (ARCHITECTURE §1), out of row 16.

**Net:** row 16 enters the portable core as `CORE` carried by an `ffi_stream` async
`AsyncStream<Chunk>`. The `Memory | File`-sink fallback (which would have parked SSE with
WebSocket) is **not** triggered. SSE/streaming-body facets are unblocked on the FFI axis; the
platform axis was already portable (§5.11).

## Caveats / honest boundaries

- **Apple only.** This is the Swift harness (the proven language in this spike family). The JNI
  edition of the same question is explicitly a separate probe — spike-plan §4 N2 for Android —
  and is **not** answered here; the stall could in principle wear a different shape on JNI.
- Latency figures are a single representative release run; they vary run-to-run by tens of µs.
  The load-bearing result is **completeness = 100/100 with no stall**, which was stable across
  runs and both pacings — not the exact µs.
- 100 chunks at 0/200µs pacing is the step-02 stress envelope, not a soak test. No multi-second
  sustained-rate or multi-MB-body run was done (out of scope — envelopes, not optimization,
  per spike-plan §8).
- `StreamProbe` re-hosts the adapter→core→consumer hub as spike plumbing (`Mutex`, a detached
  server thread, `std::time`); this is spike code, not a contract shape. The real bolted-http
  seam (how a chunk re-enters the core as a typed input, back-pressure, the end-of-body signal)
  is contract-freeze design work, not decided here.
