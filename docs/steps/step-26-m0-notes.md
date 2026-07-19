# Step 26 M0 — packaging + the harness bridge + the N2 JNI stream probe (notes for M1+)

**Milestone:** M0 (packaging + the JNI harness bridge + the N2 stream probe). **Branch:**
`step/26-android-adapter`. Scope was M0 only: the FFI crate, the walking-skeleton OkHttp adapter
(one C1 row), the structured-result driver, server lifecycle, mise wiring, the fail-able gate, and
the N2 probe. M1–M4 untouched.

**Result: both gates concluded; `test:android:http` is GREEN** (`tests=5 failures=0 errors=0`,
verified against the JUnit XML, `mise_exit=0`). No kill criterion hit (1 not reached — rule-01
passes on OkHttp; 2 not hit — N1 packaging expressible; 3 not hit — no stall/reorder, though N2
surfaces a real drop-on-overflow finding, see Gate 2). `mise run check` stays green (host, JDK-free).

## Gate 1 (bridge legibility) — both halves, GREEN

- **GREEN:** `C1/rule-01-same-request-same-outcome` passes on the real `BoltedHttp` OkHttp adapter,
  end to end through the JNI bridge (`ConformanceProbe.theC1Rule01IsGreenOnTheRealAdapter`; logcat
  `M0 GREEN-HALF rule-01 passed=true skipped=false`).
- **RED:** the same row goes red under a deliberately-broken adapter, with a legible typed message
  (`ConformanceProbe.theC1Rule01IsRedWithABrokenAdapter`).
  - **How it was broken:** a `BrokenHttp` class in the androidTest target (isolated; the shipped
    `BoltedHttp` is untouched) whose `execute` never performs a request — it immediately calls
    `harness.completeErr(token, FfiHttpError.Transport(...))`. rule-01 expects a successful GET of
    `/ok`, so the blanket failure makes it red with the structured driver reporting the typed reason.
    Restoration is automatic: the green test uses the real adapter in the same suite run.
  - **RED-HALF message (verbatim from logcat):**
    `M0 RED-HALF rule-01 message: 'ExpectedSuccessGotError { got: Transport }'`

**The JUnit-XML gate is proven fail-able.** Before the assertions were finalised, three intermediate
GMD runs failed with real assertion errors (e.g. `delivered expected:<200> but was:<125>`), and
`test:android:http` exited **1** every time — the XML summary (`failures="3"`) drove the failure, NOT
gradle's exit code (which the memory note warns can mask failures). The masking gotcha is not
inherited: the gate greps `(failures|errors)="[1-9]…"` across `TEST-*.xml` and fails on any hit, on
no-XML, or on a nonzero gradle exit.

## Gate 2 (N2) — the JNI stream probe verdict

**Verdict (freeze input).** Across JNI, the F1 `ffi_stream` push is **lossless and in-order**: every
run ingests all 200 chunks into the Rust ring (`ingested=200/200`, or `199` when the control drops
one before crossing) and the consumer receives them in ascending seq with **no reorder** — step-02's
stall/reorder ghost does **not** reproduce, and the consumer always resumes **off the main thread**
(`consumerOffMain=true`, 2–3 distinct consumer threads). The re-delivery COMPLETENESS, however, is
**not guaranteed under burst**: the loss is localised entirely to the **generated Kotlin
`callbackFlow`**, whose `processItems` does `trySend(item)` into the default `BUFFERED` (64-capacity)
channel — a **drop-on-overflow** policy. When the native drain loop front-loads the 64-slot channel
faster than the collector drains it, `trySend` silently drops; a fast (O(1)) collector delivered
`200/200` on both pacings in the green run, but slower collectors and contended runs dropped to
`171`, `132`, even `125/200` — the completeness figure is **variance-prone**, so it is RECORDED, not
gated (kill-criterion-3 discipline: the streaming seam is deliberately unfrozen).

**Numbers (green run, aosp_atd android-34 arm64, `count=200`):**

| pacing | ingested | delivered | ordered | consumer off-main | p50 | p99 |
|---|---|---|---|---|---|---|
| burst (`delay_us=0`)  | 200/200 | 200/200 | yes | yes (2 threads) | 2272.9µs | 4352.0µs |
| paced (`delay_us=200`)| 200/200 | 200/200 | yes | yes (2 threads) |  505.9µs | 2783.9µs |

Per-chunk latency is the cross-JNI + ring + Flow delivery time on one `System.nanoTime()` clock
(producer stamps `tSendNs` immediately before `deliverChunk`). It is ~1–2 orders of magnitude higher
than Apple's (Apple p50≈25µs): the callbackFlow batch-poll (batch 16) + coroutine-dispatcher hop +
JNI adds real latency — a recorded observation, not a gate. **Corruption control** (drop seq=100
before crossing): `ingested=199` — the probe detects the loss cleanly at the ingest counter,
independent of the burst channel drops, so the completeness measure is non-vacuous.

**F-M3-1 lifecycle observation on ART (the headline freeze input).** An abandoned consumer — a
`chunkStream()` collection whose scope is leaked with **no** `closeChunkStream()` and **no** cancel —
does **NOT** starve the next run's **cross-FFI ingest** (run 2 ingest-delta = 200/200 on the SAME
harness), but it **severely degrades the next consumer's callbackFlow re-delivery**: run 2's fresh
consumer received only **0–90 of 200** across observations (`0/200` in the green run, `90/200`
earlier), with `stallPoint=0` — the leaked run-1 subscription monopolises the shared
`EventSubscription`'s re-delivery. **Shape vs Apple:** on Apple a dead subscription *starved* the
next run outright; on **ART the cross-FFI ingest survives but the abandoned subscription starves the
next consumer's Kotlin re-delivery** — a shape-changed reproduction, not a disappearance. **GC
control (ReferenceQueue, per the ART-GC-probes lesson — never a polled `WeakReference.get()`):** the
abandoned `CoroutineScope` **was collected** (`enqueued=true`, `weakrefCleared=true`), yet the
degradation persisted — so the lingering subscription lives **native-side**, outliving the Kotlin
scope's GC. This is direct evidence for the freeze's streaming-seam question: **the subscription
lifecycle must be scope-bound / `Drop`-bound at the native seam** (the Kotlin `awaitClose`
unsubscribe only fires on an explicit cancel/close, which an abandoned consumer never does). The
shipped mitigation the probe uses between healthy runs — `closeChunkStream()` + cancel — keeps
back-to-back runs whole; it is the same explicit-teardown requirement Apple's F-M3-1 flagged.

## Built

- **`crates/bolted-http-android-ffi`** (workspace member) — the Android harness bridge. Depends on
  `bolted-http` with the `conformance` feature (suite rows + the in-process TLS `TestServer`). It is
  a **near-verbatim mirror** of `bolted-http-apple-ffi` — the FFI surface is the decided topology
  (step 26 "Decisions already taken"); the two crates diverge only in (a) `boltffi.toml` target
  (android vs apple), (b) the crate/package name, (c) doc language, and (d) **one behavioural line**:
  `PriorityHint` is ABSENT on Android (see decision 5).
- **`android/bolted-http`** — the consumable Android library (the "bundled package" analog): the
  hand-written `BoltedHttp.kt` (OkHttp adapter) compiled together with the generated Kotlin/JNI
  bindings + the packed `.so`, sourced IN PLACE from `crates/bolted-http-android-ffi/dist/android`
  (the proven `pack:android` / profile-probe layout — nothing vendored).
- **`android/bolted-http-conformance`** — the sibling instrumented-test project. Drives the suite
  through the JNI `HttpHarness` on the headless `dev34` GMD (aosp_atd android-34 arm64), same recipe
  as `test:android`. Depends on `:bolted-http` as its ONE dependency.
- **mise:** `pack:android:http` (mirror of `pack:android`, repointed at the http bridge crate) and
  `test:android:http` (GMD run **gated on the JUnit XML**, not the wrapper exit code — see below).
  `mise run check` unchanged (host-only, JDK-free): the new crate is a plain workspace member.

## The bridge shape M1 must build on (exact generated API)

The FFI surface is identical to Apple's (same Rust), rendered into Kotlin by `boltffi pack android`
(package `dev.bolted.http.ffi`, `error_style = "throwing"`, `factory_style = "constructors"`):

**Callback trait the Kotlin adapter implements:**

```kotlin
interface HttpAdapter {
    fun execute(request: FfiRequest)
    fun cancel(token: ULong)
}
```

**Exported harness** (`class HttpHarness : AutoCloseable`, public `constructor(adapter: HttpAdapter)`):

- `startServer(): ServerInfo` (three base URLs + good-cert DER + good/untrusted SPKI) / `stopServer()`
- `completeOk(response: FfiResponse)` / `completeErr(token: ULong, error: FfiHttpError)` — completion re-entry
- `reportProgress(token: ULong, sent: ULong, total: ULong?)` — upload-progress re-entry (rule 11)
- `runC1(): List<RowReport>` / `runExtraRows()` / `runC2()` / `runC3(): String`
- **N2 probe surface:** `deliverChunk(chunk: Chunk)`, `chunkIngested(): ULong`, `closeChunkStream()`,
  and the top-level extension `fun HttpHarness.chunkStream(): Flow<Chunk>` (a `callbackFlow`, batch
  size 16, `awaitClose { unsubscribe }`).

**Data** (`#[data]` → Kotlin `data class` / `sealed class` / `enum class`): `FfiHeader{name,value}`,
`FfiPin{hash:ByteArray}`, `FfiRequest{token:ULong, method, url, headers, body:ByteArray, deadlineMs:ULong,
pins, sink:FfiResponseSink, priority:FfiPriority}`, `FfiResponse{token, status:UShort, headers, body,
finalUrl, httpVersion:FfiHttpVersion, hops:List<String>, sinkPath}`, `ServerInfo{httpBase, httpsBase,
httpsUntrustedBase, goodCertDer, goodSpki, untrustedSpki}`, `RowReport{id, passed, skipped, message}`,
`Chunk{seq:ULong, bytes:ByteArray, tSendNs:ULong, last:Boolean}`, `FfiResponseSink{Memory | File(path)}`,
`FfiPriority{THROTTLED..CRITICAL}`, `FfiHttpVersion{HTTP1_0..HTTP3}`, `FfiHttpError{Timeout | Cancelled |
NameResolution | Connect | Tls | PinMismatch | InsecureRedirect(to) | Io | PermissionDenied |
TooManyRedirects(limit:UInt) | Transport(message)}`.

**How Kotlin registers the adapter** (the composition-root dance, in the androidTest):

```kotlin
val adapter = BoltedHttp()             // 1. adapter first
val harness = HttpHarness(adapter)     // 2. harness second (takes the adapter)
adapter.harness = harness              // 3. back-reference so completions re-enter
```

**Internal wiring** M1 extends (unchanged from Apple): the suite calls `factory.new_adapter()` → a
`KotlinAdapter` shim whose `Http::send` mints a token, parks the row's `CompletionSink` (+ any
`UploadProgressSink`) in a token-keyed `Mutex<HashMap>`, converts the request, and calls
`adapter.execute`. The Kotlin completion re-enters `complete_ok`/`complete_err`, which look up the
token, convert back, and deliver to the parked sink. Blocking model: the row parks on the driver
thread; OkHttp completions arrive on a dispatcher thread — no deadlock (confirmed green, same as
Apple's URLSession model).

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **The FFI crate is a near-verbatim mirror of `bolted-http-apple-ffi`, NOT an extracted shared
   crate.** The step doc permits a shared `bolted-http-ffi-core` "only if it stays mechanical." It
   does not: BoltFFI's bindgen reads the packed crate's **source text** (memory:
   `boltffi-bindgen-reads-source-text`), so the `#[export]`/`#[data]`/`#[ffi_stream]` items must live
   in the crate being packed — an extracted crate re-exported would not be seen by bindgen. And the
   crate name drives the native symbol names, so the two crates cannot share one. Duplication is the
   correct call; the drift risk is noted for the freeze (the two lib.rs files are identical bar docs +
   decision 5). **Structural extraction would force a bindgen change — kill-criterion territory, not
   done.**
2. **Two Gradle projects, sibling-subproject wiring.** `android/bolted-http` is the consumable
   library (adapter + generated bindings + `.so`); `android/bolted-http-conformance` is the sibling
   test project that includes it via `include(":bolted-http"); project(":bolted-http").projectDir =
   file("../bolted-http")`. This is the Gradle analog of the Apple conformance package's single path
   dependency on `../bolted-http`. `android/bolted-http` has no `settings.gradle.kts` of its own (it
   is a subproject of the conformance build) — standalone-consumability is an M-later packaging
   detail, out of M0 scope.
3. **No bundled "pack wrapper".** Unlike Apple's `bundled` SPM layout (`wrapper_sources` inside the
   pack output), `boltffi pack android` emits only `dist/android/{kotlin,jniLibs}`; the hand-written
   adapter lives in the Gradle library's own `src/main/kotlin` and the dist is sourced in place. This
   is the proven `pack:android`/profile-probe shape — Android has no `wrapper_sources` knob. **N1
   packaging is expressible (kill-criterion 2 NOT hit).**
4. **`test:android:http` gates on the JUnit XML, never the wrapper exit code** (memory:
   `test-android exit code masks failures`). The task clears `build/outputs/androidTest-results`,
   runs the GMD, then fails if any `TEST-*.xml` reports `failures>0`/`errors>0`, or if no XML is
   produced, or if gradle exited nonzero. Proven fail-able in M0 (see friction F-M0-3).
5. **`PriorityHint` is ABSENT on Android (behavioural divergence from Apple).** Decided in the step
   doc (row 12 CAP; OkHttp legally ignores the hint — no per-`Call` priority knob). So
   `KotlinFactory` does NOT override `priority_hint` (falls through to the trait default `None`) and
   there is no `impl PriorityHint for KotlinAdapter`. The hint *data* still rides every request. C3
   Android column will read `priority-hint absent` (vs Apple's `present`) — the divergence the C3
   table exists to record. `Metrics` stays `Phase` (OkHttp `EventListener`, honest, same tier as
   Apple). This is a one-line correctness choice, not re-litigation.
6. **Package `dev.bolted.http.ffi`** for the generated bindings; adapter in `dev.bolted.http`;
   conformance in `dev.bolted.http.conformance`. Clean namespaces (vs profile's `com.example.*`).
7. **HTTP version reported as `HTTP1_1` unconditionally, total deadline via OkHttp `callTimeout`.**
   Both are M0 placeholders (real `Response.protocol` and the total-vs-per-idle deadline verification
   are M1). No constraint literals: the deadline comes from `FfiRequest.deadlineMs`.

## What M1 must add (from the driver's red rows — the skeleton passes only rule-01)

- **Full C2 taxonomy** — the skeleton maps every failure to `Transport`. M1 classifies by cause:
  timeout (`SocketTimeoutException`/`callTimeout`), cancel (`IOException("Canceled")` — must NOT leak
  as a network key, N6), DNS (`UnknownHostException`), connect, TLS. See the step doc N6/C2.
- **Total-deadline honesty** — `callTimeout` is believed to bound the whole call incl. redirects
  (N6); the `/drip` trickle row (step-25 MA6) must confirm "total, not per-idle". Watch it red first.
- **Cancellation wired to `Call.cancel()`** from a non-call thread (N6, rule 9).
- **Real negotiated version** from `Response.protocol` (row 11); `Metrics` phase timings via
  `EventListener` (row 18).
- **Upload progress** via a request-body sink wrapper (N4, rule 11) — watch the buffer-jump-to-100%
  failure mode.
- **HTTPS/pinning/redirect/sink** (M2): trust anchor from `ServerInfo.goodCertDer`; SPKI pinning with
  the trust-vs-pin split (N3); https→http refusal + hop trace; file sink; gzip honesty (N4).

## What M1's stream work must know (N2 hand-off)

- The cross-FFI push is trustworthy (lossless + ordered); the **weak seam is the generated Kotlin
  `callbackFlow` re-delivery** (`trySend` into a bounded `BUFFERED` channel → drop-on-overflow). Any
  M-later assertion of stream *completeness* must either pace the producer or accept variance — a
  hard `delivered==N` gate on burst is flaky. This is BoltFFI codegen, not the adapter; the streaming
  seam is a freeze question, so M1 does not "fix" it.
- The **subscription lifecycle is native-side and not scope-bound**: an abandoned consumer degrades
  the next consumer even after ART GC's the Kotlin scope. Explicit `closeChunkStream()` + cancel is
  mandatory between runs (the probe does this). Freeze Q1 (streaming seam) inherits this directly.
- `chunkIngested()` is **cumulative** across a harness's life — per-run figures need a delta (bit us
  once; the F-M3-1 assertion uses `run2.ingested - run1.ingested`).

## Friction log (freeze-agenda input — friction matters more than usual this step)

- **F-M0-4 — the JNI→Flow re-delivery drops under burst (`trySend` into a bounded channel).** The
  headline N2 finding. BoltFFI's generated `chunkStream()` callbackFlow does `trySend` into a
  `BUFFERED`(64) channel; the native drain loop front-loads it faster than a lagging collector drains,
  so chunks are silently dropped. The Rust ring holds 1024 losslessly — the loss is purely the Kotlin
  binding's overflow policy. Ordered and lossless-at-ingest always; complete only when the consumer
  keeps up. → **freeze Q1 (streaming seam): the Kotlin binding needs a specified overflow/back-pressure
  policy** (suspend vs drop-oldest vs drop-newest vs unbounded), not silent `trySend` drop.
- **F-M0-5 — the abandoned-subscription lifecycle is native-side, not GC-bound (F-M3-1 on ART).** See
  the Gate 2 verdict. Apple: a dead subscription starved the next run. ART: the next run's cross-FFI
  ingest survives, but the leaked subscription starves the next consumer's re-delivery (0–90/200), and
  ART GC-collecting the Kotlin scope does NOT release it. → **freeze Q1: the subscription must be
  scope-/`Drop`-bound at the native seam**; `awaitClose`-unsubscribe is insufficient because an
  abandoned consumer never triggers it. This is the sharpest streaming-seam input from S-AN.

- **F-M0-1 — the FFI crate must be duplicated, not shared (bindgen reads source text).** See
  decision 1. Not a blocker, but it means every native FFI bridge crate is a full copy of the
  contract mirror; the two http-*-ffi `lib.rs` files now differ only in docs + the one PriorityHint
  line. A drift check (or a genuinely mechanical shared *macro-input* file) is a freeze/tooling
  candidate — the homogenization map already flags this shape.
- **F-M0-2 — no `FfiError` collision under Kotlin throwing style.** The Swift lesson (F-M0-1 in
  step 25: `FfiError` is reserved by Swift's throwing error style) did NOT reproduce: Kotlin's
  `error_style = "throwing"` generates `FfiException` (not `FfiError`), so our `FfiHttpError` name is
  safe and — usefully — so would a plain `FfiError` have been. The Swift-specific reservation is
  Swift-specific. Recorded so the lint candidate is scoped to Swift, not "all native targets".
- **F-M0-3 — cleartext-to-loopback needs an explicit test-tier allowance.** OkHttp against the
  in-process `127.0.0.1` `TestServer` is cleartext; ART blocks it by default (API 28+). Fixed with
  `android:usesCleartextTraffic="true"` + `INTERNET` in the conformance manifest (test-tier only; the
  shipped `android/bolted-http` carries INTERNET but no cleartext policy). A shipped adapter talks
  HTTPS, so this is a conformance-harness quirk, not a product concern — but worth a scaffolding note.
