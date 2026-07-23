# Step 27 M3 — the Apple adapter graduates: notes

**Branch:** `step-27/m3` (off main at fb53bff, containing merged M0+M1+M2). Builds on M1's
`BodyStream`/`BodyChunk`/`BodyEnd` and M2's `signal.rs` (`FlowSignal`/`FlowObserver`/`FlowSignals`),
`capability.rs` (`ChunkSink`/`StreamingHttp`), and `conformance/stream.rs` (the `DriverStream`
reference driver + rows 12/13) — none redesigned. The Linux adapter (M2) is the worked example the
Apple adapter now mirrors across the FFI.

## Gate results

- **`mise run check`** — green (exit 0). Workspace clippy `-D warnings` (covers `bolted_http_ffi`),
  fmt, all host crate tests + doctests.
- **`mise run test`** — green (exit 0): 73 `test result: ok` lines, 0 failures.
- **`mise run test:apple:http`** (packs via `pack:apple:http` first) — green:
  **`Executed 12 tests, with 0 failures`** (the 7 retained M0–M2/A5/A6 methods + 5 new M3 methods).
  boltffi CLI verified `boltffi_cli v0.28.0`, no `?rev=`, before the pack.

## Built

### 1. The A1 probe machinery graduates into the contract path (`crates/bolted-http-ffi/src/lib.rs`)

The step-25 A1 probe surface (`#[ffi_stream] chunk_stream`, `deliver_chunk(Chunk)`,
`close_chunk_stream`, `chunk_ingested`, the `EventSubscription<Chunk>` ring, `CHUNK_STREAM_CAPACITY`)
is **deleted** and replaced by shipped contract-path re-entry built on M1/M2 types:

- **New `#[data]` FFI types**: `FfiBodyChunk { seq, bytes }` (mirrors `BodyChunk`), `FfiBodyEnd`
  (`Complete { total } | Failed { error: FfiHttpError }`, mirrors `BodyEnd`), `FfiFlowSignal`
  (`Pause | Resume | Cancel`, mirrors `FlowSignal`).
- **`HttpAdapter` trait** (the BoltFFI callback surface): `cancel(token)` is **replaced** by
  `signal(token, FfiFlowSignal)` (the one signal shape, three uses); `execute_streaming(request)` is
  **added** alongside `execute`. The generated Swift protocol now requires `execute` /
  `executeStreaming` / `signal` (verified in freshly-generated
  `apple/bolted-http/Sources/BoltedHttp/BoltFFI/BoltedHttpFfiBoltFFI.swift`).
- **`HttpHarness` re-entry**: `deliver_chunk(token, FfiBodyChunk) -> bool` routes each chunk to the
  parked driver-owned `ChunkSink` (returns `false` when the core raised a typed failure — the harness
  then closes the stream with it and the adapter stops reading); `finish_body(token, FfiBodyEnd)`
  **removes and consumes** the parked sink (the driver-owned deterministic close — step 25's
  `close_chunk_stream()` fix becomes the contract path). Chunks re-enter **synchronously** (like
  `complete_ok`), not through an `ffi_stream` — there is no live native consumer to abandon, which is
  what dissolves F-M3-1 (see §5).
- **`Shared.pending_streams: Mutex<HashMap<u64, Box<dyn ChunkSink>>>`** — one live driver-owned sink
  per in-flight streamed response, token-keyed. `live_streams()` = `pending_streams.len()`, the row-14
  hygiene observable.
- **`NativeAdapter` now implements `StreamingHttp`**; `NativeFactory::streaming()` returns `Some`, so
  rows 12/13 run against the real adapter instead of skipping. `send_streaming` parks the sink, builds
  a `NativeFlowObserver` (forwards each pushed `FlowSignal` across the FFI via `adapter.signal`), and
  calls `execute_streaming`.
- **Poll-watcher deleted**: `NativeAdapter::send` (the buffered path) no longer spawns the 10 ms
  `CancelToken`-polling thread that forwarded `adapter.cancel(token)`. Cancellation is now **pushed**:
  `RequestHandle::with_signals` + a `NativeFlowObserver` whose `FlowSignal::Cancel` forwards
  `adapter.signal(token, Cancel)`. `grep thread::spawn crates/bolted-http-ffi` → nothing; `std::thread`
  / `std::time::Duration` imports removed. (C1 rule-09 / C2 key-cancelled stay green — pushed cancel
  works.)

### 2. `BoltedHttp.swift` onto the seam (`apple/bolted-http/Sources/BoltedHttp/BoltedHttp.swift`)

- **`executeStreaming(request:)`**: a `dataTask` whose `didReceive data` pushes each transport read as
  one `FfiBodyChunk` (seq ascending/gapless from 0) via `harness.deliverChunk`; `didCompleteWithError`
  delivers the single terminal via `harness.finishBody` (`.complete(total:)` on success — the total is
  every byte the transport delivered; `.failed(error:)` on a mapped transport error / deadline / caller
  cancel). The synthesized total deadline (the A3-safe `DispatchSourceTimer`) spans the whole stream.
- **`signal(token:flow:)`** replaces `cancel(token:)`: `Cancel → task.cancel()` (rule 9),
  `Pause → task.suspend()`, `Resume → task.resume()` (socket read-pacing / back-pressure). The Swift
  poll-watcher note is moot — Apple never had one; the deleted watcher was the Rust bridge's.
- **`RequestContext`** gained `streaming`, `nextSeq`, `declaredTotal`, `droppedOne`,
  `streamClosedByCore`; `didReceive data` branches on `streaming` (push vs buffer). The lock is
  released before `deliverChunk`, so a synchronous back-pressure `Pause` re-entering `signal` during a
  deliver is safe (no lock held during the nested FFI call — the same discipline M2's `DriverStream`
  uses: it releases the ingest lock before pushing a signal).
- **`StreamFault`** (`.none | .dropChunk | .skipTerminal`) + `init(streamFault:)`: the scoped
  per-adapter red twin (the Linux `LinuxHttpConfig.stream_fault` precedent, one fault at a time),
  default `.none` on the shipped adapter.

### 3. Rows on the macOS tier (`apple/bolted-http-conformance/.../ConformanceTests.swift`)

- **Rows 12/13** run against the real adapter via a new `HttpHarness::run_stream_rows()`, folded into
  `runFullSuite` so `testFullSuiteIsGreenOnTheRealAdapter` and the A6 sweep both cover them.
  Dedicated `testStreamingRowsGreenOnTheRealAdapter` prints the per-row status and asserts
  `liveStreams() == 0` after conformant streams.
- **Row 14 (subscription hygiene)** is an Apple-tier test pair (`testRow14SubscriptionHygieneGreen`
  / `testRow14RedOnLeakedSubscription`), not a shared-suite row (its observable, `live_streams`, is an
  FFI-bridge concept — exit-checklist "row 14 green on Apple + Android", not mock + Linux).
- The A1 probe tests (`testA1StreamingProbe…`, `testA1CorruptionControl…`) and their machinery
  (`runA1`, `StreamingProducer`, `StreamCollector`, `A1Result`) are **removed**; the A6 sweep dropped
  its A1-probe portion (now non-`async`) and instead sweeps the streaming rows through `runFullSuite`.
- `BrokenHttp` / `AlwaysOkHttp` updated to the new protocol (`executeStreaming` + `signal`).

## Watched-red matrix (each row × the real Apple adapter → what made it red)

| Row / assertion | Implementor | Break (fault) | Observed red | Test |
|---|---|---|---|---|
| Row 12 completeness | Apple/URLSession | `.dropChunk` (drop first read, count its bytes) | `StreamFailed { got: Transport }` (completeness gate fires) | `testStreamingRow12RedOnDroppedChunk` |
| Row 13 terminal-once | Apple/URLSession | `.skipTerminal` (never `finishBody`) | `NoTerminal` | `testStreamingRow13RedOnMissingTerminal` |
| Row 14 hygiene | Apple/URLSession | `.skipTerminal` (subscription never closed) | `liveStreams() == 2 > 0` | `testRow14RedOnLeakedSubscription` |

Console evidence from the green tier run:
`M3 RED row-12 (dropChunk): StreamFailed { got: Transport }`,
`M3 RED row-13 (skipTerminal): NoTerminal`,
`M3 RED row-14 (skipTerminal): liveStreams=2`,
`M3 STREAM [GREEN] C1/row-12-…`, `M3 STREAM [GREEN] C1/row-13-…`.

Double-terminal remains impossible **by construction** (`ChunkSink::finish` / `BodyStream::finish`
consume `self`; M1's `compile_fail` doctests still green) — row 13's reachable red is the *missing*
terminal, exercised above.

## Judgment calls / decisions (smallest reversible; recorded)

1. **Chunks re-enter synchronously via `ChunkSink`, not via `ffi_stream`.** The M3 bullet references
   both the M2 `DriverStream` (synchronous `ChunkSink` callback) as "the reference driver shape" and
   the A1 probe's `ffi_stream` async push whose `close_chunk_stream()` "becomes the contract path".
   These reconcile as: the **deterministic-close discipline** graduates (a terminal removes+consumes
   the parked sink), while the **transport** uses the synchronous re-entry the M2 reference driver
   already established (mirroring `complete_ok`). Consequence: **there is no `ffi_stream` and no live
   native consumer in the contract path**, so the F-M3-1 abandoned-consumer leak (an unfixed
   `ffi_stream` runtime defect at 0.28.0) cannot occur here at all — it reduces to the deterministic,
   registry-level "a stream whose terminal never arrives leaves a parked entry", which row 14 detects.
   This is the smallest shape that satisfies §3a–§3d without adding surface to `bolted-http`.
2. **Row 14 detects the leak via the Rust registry count (`live_streams`), not an ARC/GC poll.** The
   step's caution ("a GC/ARC-dependent probe needs a deterministic detection mechanism … never a
   timing poll against a weak reference") is honoured by sidestepping ARC entirely: the driver-owned
   `pending_streams.len()` is exact and synchronous. This is the §3d "live-subscription count back to
   baseline" observable directly.
3. **`deliver_chunk` returns `bool`, not the typed `Result` the Rust `ChunkSink` returns.** On a typed
   ingest failure the harness (which owns the sink registry) closes the stream itself and signals the
   adapter to stop via `false` — avoiding an error round-trip Rust→Swift→Rust. Recorded divergence
   from `ChunkSink::deliver_chunk` (whose Rust callers, e.g. Linux, receive the error and finish
   themselves); the row-12 red still surfaces (the gate fires at `finish_body` time for `.dropChunk`,
   which produces no per-chunk error — the ingest stays gapless, only the declared total disagrees).
4. **`.dropChunk` drops the FIRST read (not the last).** URLSession's `didReceive data` cannot know
   which read is last mid-stream, so "drop the last chunk" (Linux `Truncate`) is awkward. Dropping the
   first read while still counting its bytes toward the declared total produces the identical gate
   failure (`total > ingested`) — the same shape as Linux `StreamFault::DropChunk`.

## Friction / division of labour

- **Back-pressure is wired but not stress-tested on Apple** (same division M2 recorded for Linux). The
  streaming rows drive `/chunked?count=384` (> `RING_CAPACITY` 256), but URLSession coalesces transport
  reads, so the Apple adapter pushes only a handful of large `didReceive data` chunks — the core ring
  rarely fills, so `Pause` rarely fires in practice. The pause/resume path IS wired end-to-end
  (`FlowSignals` → `NativeFlowObserver` → `adapter.signal` → `task.suspend/resume`) and would engage if
  the ring approached capacity; the deliberate back-pressure **stress** stays on the mock (M2's
  `StreamMock`, one `BodyChunk` per line, `> RING_CAPACITY` of them). Recorded division, not a gap —
  Apple proves real-socket completeness + that the signal surface is wired.
- **Apple/Android FFI trait is now shared and changed** (`cancel → signal`, `+ execute_streaming`).
  `mise run check` (host, Rust-only) does not build the Kotlin adapter, so it is green; **M4 must
  update `BoltedHttp.kt`** to the new `HttpAdapter` shape (execute_streaming + signal) — expected by
  the plan ("Same shape on `BoltedHttp.kt`"). The generated Kotlin bindings are only produced by the
  Android pack, which M3 does not run.

## Open questions (for planning)

None outstanding. The four judgment calls above are recorded for review; all are reversible and none
touch ARCHITECTURE §1–§7 or a §9 OPEN question. The synchronous-vs-`ffi_stream` re-entry choice
(decision 1) is worth a planning glance since it changes what "the subscription lifecycle" means in
the shipped path (it becomes a registry entry, not a boltffi-runtime subscription) — but it stays
within the adopted seam shape and the streaming-seam §7 upstream-RFC re-evaluation trigger is
unaffected.
