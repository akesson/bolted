# Step 27 M2 — the mid-flight signal + the Linux adapter + the host-side rows: notes

**Branch:** `step-27/m2` (off main at 294b154, the M1 head). Builds on M1's
`BodyStream`/`BodyChunk`/`BodyEnd`/`RedirectCeiling` — none redesigned. Gate — `mise run check`
(exit 0) + `mise run test` (73/73 test-result lines ok, 0 failing) — fully green, including the
new rows on mock + Linux and the `--features bolted-core` / `--features conformance` lines.

## Built

### 1. The one core→adapter mid-flight signal surface (Q4 + streaming-seam §3b option C)

New module **`crates/bolted-http/src/signal.rs`** — ONE shape, two uses, push-based, sans-io,
lock-free (kill criterion 2 respected):

- **`enum FlowSignal { Pause, Resume, Cancel }`** (`#[non_exhaustive]`). Pause/Resume =
  back-pressure; Cancel = the pushed cancel that replaces poll-watching.
- **`trait FlowObserver: MaybeSend + Sync { fn on_signal(&self, FlowSignal); }`** — the adapter's
  reaction. The core invokes it synchronously; the reaction is the adapter's own (an `AtomicBool`,
  a tokio `Notify`, an abort). **The contract mandates no thread and no channel** — the observer is
  an `Arc` fixed at construction, so emitting is a plain synchronous call with no interior
  mutability and no lock in the contract type.
- **`struct FlowSignals { observer: Arc<dyn FlowObserver> }`** — the core-held emitter (`pause()` /
  `resume()` / `cancel()`). Cheap clone; the driver holds it, the adapter's observer receives.

**Naming** (M1's house style — sketches settle here): `FlowSignal` / `FlowObserver` / `FlowSignals`
(the emitter reads as "the signals you push"). The adapter→core direction (chunks) is `ChunkSink`;
the doc records the cookie per-hop re-entry (Q9) as the named future third instance so it can attach
without re-opening the contract.

### 2. The streaming dispatch (capability-shaped, opt-in — never a widening of `Http`)

`crates/bolted-http/src/capability.rs`:

- **`trait ChunkSink: MaybeSend`** — adapter→driver body delivery: `deliver_chunk(&self, BodyChunk)
  -> Result<(), HttpError>` (repeatable) + `finish(self: Box<Self>, BodyEnd)` (one-shot — the
  terminal fires exactly once **by construction**, a use-after-move, the `CompletionSink` discipline
  for streamed bodies).
- **`trait StreamingHttp: Http { fn send_streaming(&self, HttpRequest, Box<dyn ChunkSink>) ->
  FlowSignals; }`** — an opt-in capability like `Metrics`. The buffered path is untouched for
  adapters that do not stream.
- **`RequestHandle` gained an optional `FlowSignals`** (`with_signals` ctor; `for_token` unchanged →
  `None`). `cancel()` now sets the poll token **and** pushes `FlowSignal::Cancel` when signals are
  present. This is what lets an adapter delete its poll-watcher while the FFI bridges keep polling
  (additive; FFI untouched, still compiles).

`AdapterFactory` gained `fn streaming(&self) -> Option<Box<dyn StreamingHttp>>` (default `None`),
generated-from-the-types like `metrics()`; a factory without streaming records a **skip**, never a
vacuous pass.

### 3. `bolted-http-linux` onto the full seam

`crates/bolted-http-linux/src/lib.rs`:

- **Poll-watcher deleted.** `wait_cancelled` (the 10 ms `token.is_cancelled()` loop) is **gone**
  (`grep wait_cancelled crates/bolted-http-linux` → nothing). Both the buffered path (`perform`) and
  the streaming path now race a **pushed** cancel: a `LinuxFlowObserver` notifies a tokio
  `tokio::sync::Notify` (`notify_one`, so a cancel racing the `select!` setup is not lost), and
  `perform`/`stream_perform` `select!` on `cancel.notified()`. `send` returns
  `RequestHandle::with_signals(..)`.
- **Streaming (`impl StreamingHttp` + `stream_perform`)**: follow redirects to the terminal
  response, then `resp.bytes_stream()` → one `BodyChunk` per transport read → `deliver_chunk`.
  **Pause honoured by socket read-pacing**: while `paused` is set the loop stops polling the stream
  (register-the-resume-waiter-before-rechecking, so no lost wake-up), so the socket back-pressures
  the server and the ring never overflows. Closes with the **real terminal**: `Complete { total }`
  from the bytes it counted, or `Failed(Transport/Cancelled/Timeout)` on mid-body error / cancel /
  deadline (one **total** deadline via a single pinned `sleep` across follow + body).
- **Redirect exhaustion re-pointed at core counting (Q2).** The inline `if hops.len() as u32 >=
  redirect_limit` is deleted; `Inner` holds a `RedirectCeiling`, and after each hop is recorded
  `redirect_ceiling.enforce(&hops)?` fires. reqwest's native follow is `Policy::none` (it follows
  **zero** hops), so the ceiling below it is the sole authority — trivially "native limit above the
  ceiling". Observably identical for the suite: `c2_too_many_redirects` (`/redirect-loop`) still
  yields `TooManyRedirects { limit: 10 }`, same key, same param (verified green). The strict-`>`
  boundary (M1) differs from the old `>=` by exactly one hop — invisible on an infinite loop, and no
  suite row drives a chain at the boundary.

### 4. New suite rows + rule-11 total, host-side

New module **`crates/bolted-http/src/conformance/stream.rs`**:

- The **driver** (`DriverStream`): the core `BodyStream` behind a `Mutex` (the sync lives in the
  harness, **never** the contract crate), a slow consumer that drains + pushes pause/resume by ring
  occupancy (`HIGH = ¾·RING_CAPACITY`, `LOW = ¼·RING_CAPACITY` — derived, never literals), and the
  recorded terminal. The terminal drains the ring tail before consuming the ingest, so the consumer
  never loses bytes.
- A **synthesising `StreamMock`** implementor (the mock leg): produces the exact body the test
  server's `/chunked` endpoint does (`chunk-NNNNNN\n` per line), one `BodyChunk` per line, on a
  producer thread that honours pause via a condvar. `ROW_CHUNKS = RING_CAPACITY + 128` (> capacity),
  so back-pressure is genuinely exercised and load-bearing on the mock. Faults: `Truncate`,
  `SkipTerminal`, `IgnorePause`.
- **Row 12** (`C1/row-12-slow-consumer-completeness`) and **Row 13**
  (`C1/row-13-terminal-exactly-once`), run on any streaming-capable factory (skip otherwise). Both
  drive `/chunked?count=ROW_CHUNKS`; row 12 asserts terminal `Ok(total)` with `total ==
  drained.len() == body`, row 13 asserts a terminal arrived.
- **Linux runs the same `stream::rows()`** (`streaming_rows_pass_against_reqwest_adapter`, green).

### 5. Rule 11's `total` assertion (Q8)

`judge_progress` (c1.rs) now also requires the final progress sample's `total == Some(body_len)`
(new `FailureReason::ProgressWrongTotal`). `content_length` is always known for our `Bytes | File`
bodies. New netmock flag `honest_progress_total` (default true); the twin reports `Some(total + 1)`.

## Watched-red matrix (each row × implementor → what made it red)

| Row / assertion | Implementor | Break | Observed red | Test |
|---|---|---|---|---|
| Row 12 completeness | StreamMock | `Truncate` (drop last chunk, declare full total) | `StreamFailed { Transport }` (completeness gate fires) | `conformance::stream::tests::row_12_red_on_truncation` |
| Row 12 back-pressure | StreamMock | `IgnorePause` (ignore Pause under slow consumer) | `StreamFailed { StreamOverflow }` (ring overflows) | `…::row_12_red_on_ignored_back_pressure_is_overflow` |
| Row 12 completeness | Linux/reqwest | `StreamFault::DropChunk` (skip a chunk, count its bytes) | `Fail(_)` (completeness gate fires) | `conformance::streaming_row_12_red_on_dropped_chunk` |
| Row 13 terminal-once | StreamMock | `SkipTerminal` (never `finish`) | `NoTerminal` | `…::row_13_red_on_missing_terminal` |
| Row 13 terminal-once | Linux/reqwest | `StreamFault::SkipTerminal` | `Fail(NoTerminal)` | `conformance::streaming_row_13_red_on_missing_terminal` |
| Rule 11 `total` | socket mock | `honest_progress_total=false` (report `total+1`) | `ProgressWrongTotal` | `conformance::c1::tests::rule_11_red_when_total_is_dishonest` |
| Back-pressure load-bearing (positive) | StreamMock | (conformant, `ROW_CHUNKS > capacity`) | Pass only because Pause is honoured — the proof the surface works | `…::back_pressure_is_load_bearing_no_overflow_under_slow_consumer` |

Double-terminal (row 13) is **impossible by construction** — `ChunkSink::finish`/`BodyStream::finish`
consume `self` — so its red case is a compile error, proven by M1's `compile_fail` doctests
(`stream::TerminalIsExactlyOnceByConstruction`, still green). The reachable red is the *missing*
terminal, exercised above.

Rule-11's total watched-red lives on the **socket-mock** implementor (the netmock twin), following
the established precedent that all rule-11 red twins are netmock flags; Linux is the honest positive
(reports `Some(body_len)`, passes in `c1_all_rules_pass_against_reqwest_adapter`). No Linux
upload-total-lie config flag was added (it would be the only rule-11 break with one).

## The truncation-key decision — Transport sufficed, no key minted

**Kept `HttpError::Transport`; no dedicated truncation key minted.** The M1 revisit trigger fires
only IF row 12's red is *ambiguous* — the row unable to tell the truncation it forbids from a generic
transport failure. It is **not** ambiguous: row 12 expects a *complete* body (terminal `Ok(total)`
with `total == drained == body`), so **any** failure terminal makes it red — the redness comes from
"expected the whole body, got a failure", never from the failure's key. A truncation (dropped chunk
⇒ declared total > ingested) surfaces as the completeness gate's `Err(Transport)` and the row is red
regardless of which key it carries. Minting a `http.truncated` key would add contract surface that
buys the row nothing. Recorded here as the forcing evidence for *not* minting it; the M1 pointer
comments in `stream.rs` stay (a future consumer that must *branch* on truncation vs. reset is the
trigger, and that consumer does not exist).

## StreamOverflow C2 reachability

Updated the c2 justification (`reachability`): the adapter-driven control now **exists** — row 12's
`IgnorePause` twin overflows the ring through a real `StreamingHttp` adapter. It stays classified
`ContractGap` (not upgraded to `Reachable`) on purpose: **no conformant adapter ever produces
StreamOverflow** — it is the typed failure a *broken* adapter earns, reachable only under fault
injection, so a `Reachable` C2 row asserting a correct adapter yields it would be a lie. The note
names the control; nothing is silently skipped.

## Friction

- **Watched-red tool choice** (same as M1): red twins gathered with targeted `cargo test -p
  bolted-http --features conformance …` / `cargo test -p bolted-http-linux …` — the identical test
  binaries the gate builds — then the real gate (`mise run check` + `mise run test`, both green) as
  the final confirmation. Per-mutation full-`mise-check` is impractical; the "only mise" rationale
  (platform-tier exit codes mask failures) does not apply to host Rust unit tests.
- **`Instant::now` is disallowed workspace-wide**, so the slow consumer uses a **bounded tick loop**
  (`TICKS × TICK ≈ 3 s`) rather than a wall-clock budget — no `#[allow]` needed. A `SkipTerminal`
  red twin therefore waits the full tick budget before reporting `NoTerminal` (≈ 3 s per such test).
- **reqwest coalesces transport reads**, so a small `/chunked` body yields only a handful of
  `BodyChunk`s on Linux — the ring rarely fills there and Linux's pushed-pause path is *wired but not
  stress-tested*. The mock (one `BodyChunk` per line, `> RING_CAPACITY` of them) is the deliberate
  back-pressure stress; Linux proves real-socket completeness + that the signal surface is wired.
  Recorded division, not a gap.

## FFI / M3 / M4 flag

- **`bolted-http-ffi` still compiles unchanged** (verified: `cargo build -p bolted_http_ffi` green).
  It uses `RequestHandle::for_token` + the poll `CancelToken` — both intact. The seam adoption forced
  **no** FFI-surface change: the new surfaces (`FlowSignal`/`FlowObserver`/`FlowSignals`,
  `ChunkSink`, `StreamingHttp`, `RequestHandle::with_signals`) are all **additive**.
- **For M3/M4:** the Apple/Android adapters still poll (their FFI bridge's `NativeAdapter` keeps its
  10 ms watcher thread) — deleting those is M3/M4, exactly as the plan says. They will implement
  `StreamingHttp` (delegate `didReceive data` / OkHttp source → JNI push → `deliver_chunk`) and route
  cancel/pause through the `FlowSignals` surface built here. The driver-side `DriverStream` in the
  harness is the reference shape for their rung-2 driver.

## Open questions (for planning)

None outstanding. One decision recorded for visibility (smallest-reversible, reversible):
`RequestHandle` now carries an optional `FlowSignals` and `cancel()` fires **both** the poll token
and the pushed signal — chosen so Linux can delete its poll-watcher *now* while the FFI bridges keep
polling *until* M3/M4, with no flag-day. If a future cleanup makes all adapters push, the token can
be dropped from `RequestHandle` then.
