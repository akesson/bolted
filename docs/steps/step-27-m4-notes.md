# Step 27 M4 — the Android adapter graduates: notes

**Branch:** `step-27/m4` (off main at 4a7ce9a, containing merged M0+M1+M2+M3). Builds on M3's shared
FFI bridge (`crates/bolted-http-ffi`): `HttpAdapter::execute_streaming`/`signal`, the harness re-entry
`deliver_chunk`/`finish_body`/`live_streams`, and `FfiBodyChunk`/`FfiBodyEnd`/`FfiFlowSignal`. The
**Rust bridge is unchanged** — M3 left it complete for both platforms; M4 is Kotlin + test-tier only.
The Apple M3 adapter is the worked example the Android adapter now mirrors across the FFI.

## Gate results

- **`mise run check`** — green (exit 0): workspace clippy `-D warnings` (covers `bolted_http_ffi`),
  fmt, all host crate tests + doctests.
- **`mise run test`** — green: 73 `test result: ok` lines, 0 failures (Rust untouched, so identical to
  the M3 baseline).
- **`mise run test:android:http`** (packs via `pack:android:http` first) — green **by JUnit XML**:
  `TEST-dev34-_-.xml` → `tests="14" failures="0" errors="0" skipped="0"`. boltffi CLI verified
  `boltffi_cli v0.28.0`, no `?rev=`, before the pack (the `builder v0.1.27 ?rev=` line in
  `cargo install --list` is a different tool). Fourteen tests = the 10 retained M0–M3 methods
  (ConformanceProbe ×2, M1Conformance ×2, M2Conformance ×5, HttpEngineProbe ×1) + 4 new
  StreamConformance methods; the 4 retired `StreamProbe` methods are gone.

## Built

### 1. `BoltedHttp.kt` onto the new `HttpAdapter` shape (`android/bolted-http/.../BoltedHttp.kt`)

- **`executeStreaming(request)`** (new): enqueues the call; on `onResponse`, `streamResponse` reads the
  OkHttp response body **source** in the adapter's own read loop (`source.read(buffer, READ_WINDOW)`),
  pushing each read as one `FfiBodyChunk(seq, bytes)` (seq ascending/gapless from 0) through
  `harness.deliverChunk`. Clean end-of-body → `harness.finishBody(Complete { total })` where `total` is
  every byte the transport delivered (the completeness gate's declared total); a mid-body `IOException`
  → `finishBody(Failed { error })` mapped by the same `classify()` the buffered path uses. A `false`
  from `deliverChunk` (core raised a typed `seq`/overflow failure and already closed the stream) → the
  loop stops, cancels the call, sets `streamClosedByCore`, and does **not** finish. `onFailure` delivers
  `finishBody(Failed(classify(..)))` unless the core already closed.
- **`signal(token, flow)` replaces `cancel(token)`**: `CANCEL` → record the caller-cancel cause +
  `call.cancel()` (the buffered path's pushed cancel — the Kotlin side of M3's poll-watcher deletion) +
  wake any paused read; `PAUSE`/`RESUME` → read-pacing of the streaming loop (a `paused` `AtomicBoolean`
  + a `pauseLock` monitor the loop guarded-waits on). `FfiFlowSignal` generated as an `enum class`
  (`PAUSE`/`RESUME`/`CANCEL`), so the `when` is exhaustive without `else`.
- **Read-pacing is lost-wake-up-safe** (the Linux `LinuxFlowObserver` precedent): `Pause` only sets the
  flag (safe to re-enter synchronously during a `deliverChunk`, since no lock is held there — the
  M3/Linux discipline); `Resume`/`Cancel` clear the flag and `notifyAll` under the monitor, and the loop
  re-checks `paused` under the monitor before waiting. `call.isCanceled()` is re-checked in the wait
  loop so a deadline/cancel firing while paused cannot hang.
- **`StreamFault` enum** (`NONE`/`DROP_CHUNK`/`SKIP_TERMINAL`) + a second defaulted constructor param —
  the scoped per-adapter red twin, mirroring Swift's `.none`/`.dropChunk`/`.skipTerminal` and Linux's
  `StreamFault`. One flag; the adapter is not forked.
- **`buildPerCallClient(request, ctx)`** extracted (shared by `execute` + `executeStreaming`): the
  total-deadline enforcement, the optional trust anchor + SPKI pins, and the redirect-classification
  network interceptor (below).
- **The OkHttp redirect text-match is deleted** — `TOO_MANY_REDIRECTS_PREFIX` is gone (see §2).

### 2. Redirect exhaustion re-classified structurally, not by text (Q2) — a judgment call

The step asks to delete the `TOO_MANY_REDIRECTS_PREFIX = "Too many follow-up requests"` message match
and do "the equivalent" of M2 (Linux) / M3 (Apple). **These two are not the same shape**, and neither is
directly available to OkHttp:

- **Linux core-counts**: reqwest's native follow is `Policy::none`, the adapter follows manually and a
  Rust-side `RedirectCeiling` (from `LinuxHttpConfig`) counts the hop trace. This needs (a) a ceiling
  value and (b) manual following.
- **Apple uses the native cap**: URLSession's own internal redirect cap fires and surfaces as the
  **typed** `URLError.httpTooManyRedirects` code, which `mapError` maps to `TooManyRedirects(limit: 0)`
  — `0` being the documented "adapter-internal cap" sentinel. No core ceiling, no text.

For OkHttp: its follow-up cap is **not publicly configurable** (so "native limit above a core ceiling"
cannot be expressed while keeping auto-follow), and **no redirect ceiling crosses the FFI surface**
(`FfiRequest` carries none; adding one would be a bridge change *and* would diverge from Apple, which
uses the native cap). OkHttp's cap fires as a `ProtocolException` whose **only** distinguishing feature
is its message — hence the old text match.

**Decision (smallest reversible, recorded):** mirror **Apple** — keep OkHttp's native auto-follow and
its own cap (the `limit: 0` sentinel), but classify the exhaustion **structurally** instead of by text.
A per-hop **network interceptor** records `ctx.lastHopWasRedirect = response.isRedirect` for each hop;
when OkHttp exhausts its cap on `/redirect-loop` the last hop it saw is a 3xx, so `classify()` maps the
resulting `ProtocolException` to `TooManyRedirects(0)` **by that recorded cause** when
`lastHopWasRedirect`, else `Transport`. This deletes the text match, invents no constraint literal, adds
no bridge change, and is consistent with Apple's native-cap approach. It is *not* literal core-counting
(no core ceiling crosses to the FFI adapters — Apple's isn't either); the step's "now core-counted"
prose describes M2's Linux path, which the FFI adapters do not share. **This did not trip kill-criterion
territory** (no `bolted-http` change, no bridge change, no structural decision) — it is a scoped adapter
classification choice. **Verified green**: `C2/key-too-many-redirects` passes on the real adapter, and
is watched-red under `BrokenHttp` (`WrongErrorKey { expected: TooManyRedirects, got: Transport }`).

### 3. N2's `StreamProbe` machinery graduates (`android/bolted-http-conformance/...`)

- **`StreamProbe.kt` deleted.** It drove the pre-0.28.0 A1/N2 `ffi_stream` probe surface (`Chunk`,
  `chunkStream()` as a `callbackFlow`-backed `Flow`, `deliverChunk(Chunk)`, `closeChunkStream`,
  `chunkIngested`) — all removed from the Rust bridge in M3, so the file no longer compiled. Its stale
  `trySend`/`callbackFlow`/`ffi_stream` comments (the re-delivery model that no longer exists) went with
  it — the step's "clean in passing" satisfied by deletion. (Only a single descriptive mention survives,
  in the new `StreamConformance` header, documenting *what* graduated.)
- **`StreamConformance.kt` added** — rows 12/13/14 against the real `BoltedHttp` adapter via
  `harness.runStreamRows()` and `harness.liveStreams()`, mirroring Apple's `ConformanceTests.swift`:
  `theStreamingRowsAreGreenOnTheRealAdapter` (rows 12/13 green + the row-14 baseline 0 asserted first as
  the positive control), and one watched-red test per row via the `StreamFault` twins.
- **`Adapters.kt`** (`BrokenHttp`/`AlwaysOkHttp`) updated to the new trait: `cancel` → `signal` (no-op)
  and `executeStreaming` added (Broken → `finishBody(Failed(Transport))`, AlwaysOk → `Complete(0)`),
  mirroring the Swift breaks. `HttpEngineProbe.kt` (N5) needed no change (buffered-path only).

## Watched-red matrix (each row × the real Android/OkHttp adapter → what made it red)

Evidence from the per-test **logcat** captured under
`android/bolted-http-conformance/build/outputs/androidTest-results/managedDevice/debug/dev34/`
(the GMD JUnit XML carries no `<system-out>` — F-M1-6 — so the typed reason is read from the captured
logcat file, while the pass/fail count is the JUnit XML's).

| Row / assertion | Break (fault) | Observed red | Test / logcat evidence |
|---|---|---|---|
| Row 12 completeness | `DROP_CHUNK` (drop first read, count its bytes) | `StreamFailed { got: Transport }` (completeness gate fires) | `theStreamingRow12IsRedOnADroppedChunk` — `M4 RED row-12 (DROP_CHUNK): passed=false msg='StreamFailed { got: Transport }'` |
| Row 13 terminal-once | `SKIP_TERMINAL` (never `finishBody`) | `NoTerminal` | `theStreamingRow13IsRedOnAMissingTerminal` — `M4 RED row-13 (SKIP_TERMINAL): passed=false msg='NoTerminal'` |
| Row 14 hygiene | `SKIP_TERMINAL` (subscription never closed) | `liveStreams() == 2 > 0` | `theStreamingRow14IsRedOnALeakedSubscription` — `M4 RED row-14 (SKIP_TERMINAL): liveStreams=2` |

Green evidence (`theStreamingRowsAreGreenOnTheRealAdapter`): `M4 STREAM [GREEN]
C1/row-12-slow-consumer-completeness`, `M4 STREAM [GREEN] C1/row-13-terminal-exactly-once`, and
`liveStreams() == 0` both before (baseline positive control) and after conformant streams (row 14
green). The matrix is identical in shape to Apple's M3 matrix.

Double-terminal remains impossible by construction (the Rust `ChunkSink::finish` consumes the sink); the
reachable row-13 red is the *missing* terminal, exercised above.

## Judgment calls / decisions (smallest reversible; recorded)

1. **Redirect exhaustion classified structurally, mirroring Apple's native cap** (see §2 above) — the
   headline call. Not literal core-counting; consistent with the FFI adapters' native-cap reality.
2. **`DROP_CHUNK` drops the FIRST read** (as on Apple): OkHttp's source read loop cannot know which read
   is last mid-stream, so "drop the last chunk" (Linux `Truncate`) is awkward. Dropping the first read
   while counting its bytes toward the declared total produces the identical gate failure
   (`total > ingested` ⇒ `StreamFailed { Transport }`). On this loopback body the whole `/chunked`
   response often arrives in one read, so `DROP_CHUNK` may drop the *only* read (ingested 0, declared
   full) — the gate still fires; the red is robust either way.
3. **`READ_WINDOW_BYTES = 8192` is a transport I/O buffer, not a contract constraint.** okio's
   `BufferedSource.read(Buffer, byteCount)` requires an explicit max per read (unlike URLSession's
   `didReceive data` or reqwest's `bytes_stream`, which hand the adapter natural transport reads). The
   completeness gate counts total bytes regardless of chunking, so the value has no semantic effect. It
   is the socket-read hand-off granularity, explicitly *not* a ring capacity / watermark / ceiling
   (those still come from the core). Recorded so it is not mistaken for a smuggled constraint literal.
4. **Row 14 is an Android-tier test, detecting the leak via the Rust registry count (`liveStreams`),
   not an ART/GC poll** — the same shape as Apple's row 14 (its observable, `live_streams`, is an
   FFI-bridge concept, exit-checklist "row 14 green on Apple + Android"). This sidesteps the F-M3-1
   `ReferenceQueue`-vs-`WeakReference` caution entirely: the count is exact and synchronous.

## Friction / division of labour

- **Back-pressure is wired but not stress-tested on Android** — the same division M2/M3 recorded for
  Linux/Apple. `/chunked?count=384` is a ~5 KB body; OkHttp reads it in one or two ~8 KB transport reads,
  so the adapter pushes only a handful of `FfiBodyChunk`s and the core ring (capacity 256) never fills,
  so `Pause` rarely fires in practice. The pause/resume path IS wired end to end (`FlowSignals` →
  `NativeFlowObserver` → `adapter.signal` → the `paused`/`pauseLock` read-pacing) and would engage if the
  ring approached capacity; the deliberate back-pressure **stress** stays on the mock (M2's `StreamMock`,
  one `BodyChunk` per line, `> RING_CAPACITY` of them). Recorded division, not a gap — Android proves
  real-socket completeness + that the signal surface is wired.
- **The GMD JUnit XML has no `<system-out>`** (F-M1-6). The `test:android:http` gate reads pass/fail from
  the XML (the authority — never the exit code); the *typed reason* for each watched-red is read from the
  per-test logcat file the GMD captures. Both are cited in the matrix above.

## Open questions (for planning)

None outstanding. The four judgment calls are recorded for review; all are reversible and none touch
ARCHITECTURE §1–§7 or a §9 OPEN question. The redirect-classification call (decision 1) is worth a
planning glance because the step's "now core-counted" prose does not match what the FFI adapters
actually do (both Apple and Android use the native engine cap, not a core `RedirectCeiling` — only
Linux core-counts); the deletion of the fragile text match is achieved, which is the load-bearing part.
