# Step 26 M3 — probes + sweeps (N5 HttpEngine, under-load stream, N4 residual)

**Milestone:** M3 (probe-grade sweeps; the mutation pass is M4, not this). **Branch:**
`step/26-android-adapter`. Scope: the N5 `HttpEngine` feature-detection verdict, the under-load
stream numbers completing M0's N2 evidence, and any N4 residual. **No shipped-adapter change** (all
new work lives in the `androidTest` target); **no FFI/Rust change**; the `bolted-http` contract is
untouched.

## Gate result

- `mise run check` — green (host, JDK-free; no Rust changed).
- `mise run test:android:http` — green on the headless `dev34` GMD (aosp_atd android-34 arm64):
  **`tests="14" failures="0" errors="0" skipped="0"`**, verified against the JUnit XML (not the
  wrapper exit code). 14 tests = the 12 M0/M1/M2 tests (all still green) **+ 2 new M3 probes**:
  - `HttpEngineProbe.httpEngineFeatureDetection` — the N5 verdict (below).
  - `StreamProbe.theStreamIsWholeUnderCpuLoad` — the under-load N2 sweep (below).

All observed evidence below is pulled from the on-device per-test logcat
(`build/outputs/androidTest-results/managedDevice/debug/dev34/logcat-*.txt`) — the GMD JUnit XML
carries no `<system-out>` (F-M1-6), so probe output lives in logcat, verbatim.

## N5 — `android.net.http.HttpEngine` feature detection (the verdict)

**Verdict (one sentence):** The OkHttp/HttpEngine engine matrix is **SPIKE-REAL** on the ART tier —
the platform Cronet engine is present, constructible, and drove a live request end-to-end — **but the
h3/h2 leg is paper against *this* conformance TestServer** (a raw HTTP/1.1 listener, no ALPN/QUIC) and
Cronet has no cheap anchor-install for the self-signed loopback, so an h3 conformance row is not cheap
here; the second engine path stays out of scope per the step doc.

**Observed facts (verbatim from logcat):**

```
N5 API facts: SDK_INT=34 RELEASE=14 device=emulator64_arm64 (dev34 GMD, aosp_atd android-34 arm64)
N5 presence: android.net.http.HttpEngine PRESENT (loader=java.lang.BootClassLoader@…)
N5 constructible: YES — HttpEngine version=114.0.5735.84
N5 HttpEngine GET http://127.0.0.1:PORT/ok (cleartext): status=200 negotiatedProtocol='unknown' bytes=2 error=null
N5 HttpEngine GET https://127.0.0.1:PORT/ok (self-signed TLS): status=-1 negotiatedProtocol=''
    error=NetworkExceptionImpl: … net::ERR_CERT_AUTHORITY_INVALID, ErrorCode=11, InternalErrorCode=-202, Retryable=false
```

What this establishes, in the probe's order:

1. **Present.** `Class.forName("android.net.http.HttpEngine")` resolves on the boot classloader of the
   aosp_atd android-34 image — the API-34 platform Cronet is genuinely on this tier, not just in the
   compile-time stubs. (SDK_INT=34, Android 14.)
2. **Constructible.** `HttpEngine.Builder(context).build()` succeeds and self-reports Cronet
   **version 114.0.5735.84** — the ATD image *does* ship the backing module (contrary to the a-priori
   worry that a stripped ATD would present the stub but fail to construct).
3. **One row driven through an HttpEngine execute path (cheap, done).** A rule-01-shaped
   `GET /ok` over the cleartext loopback base completed **status=200, error=null** through
   `HttpEngine.newUrlRequestBuilder(...).start()` with a `UrlRequest.Callback` draining the body — a
   real HttpEngine round-trip in the androidTest target (never the shipped adapter). Cronet reports
   `negotiatedProtocol='unknown'` for cleartext HTTP/1.1 (it classifies only the h2/h3 ALPN cases).
4. **h3/h2 not negotiable against this server (recorded, leg stopped, time-boxed).** The good HTTPS
   base fails `net::ERR_CERT_AUTHORITY_INVALID`: Cronet validates against the system trust store and
   has no cheap public anchor-install for the TestServer's self-signed loopback cert (unlike OkHttp's
   `sslSocketFactory(...)`, which M2 used). **And even a trusted engine could reach only HTTP/1.1**:
   the conformance TestServer is a hand-rolled HTTP/1.1 listener (rustls for TLS but raw `HTTP/1.1` on
   the wire — `crates/bolted-http/src/conformance/server.rs`), with **no ALPN and no QUIC**, so h2
   (needs ALPN) and h3 (needs QUIC) are structurally unreachable against it regardless of the engine.
   Driving a genuine h3 row would need an h3-capable test server — not cheap, not this step.

**N5 finding (freeze / follow-up input): HttpEngine needs `ACCESS_NETWORK_STATE`, and its absence
crashes the process uncatchably.** The first probe run crashed the *whole instrumentation* with
`SecurityException: ConnectivityService: Neither user … nor current process has
android.permission.ACCESS_NETWORK_STATE` — thrown on Cronet's internal network thread during engine
init, so a `try/catch(Throwable)` at the caller does **not** contain it (it took down 11 unrelated
tests). Granting `ACCESS_NETWORK_STATE` in the conformance manifest (test-tier only; decision 1) fixed
it. **Implication for a real HttpEngine adapter:** the engine path needs `ACCESS_NETWORK_STATE` beyond
`INTERNET`, and engine-init failures are async/uncatchable — a shipped engine matrix must gate engine
selection on permission presence *before* construction, not defensively around it.

## Under-load stream numbers (completing M0's N2 evidence, the A1 way)

Re-ran the N2 chunk probe with the **FAST O(1) collector** while background threads **saturate the
CPU** (one busy-spin daemon per core; the aosp_atd emulator has **2 cores** → 2 spinners), both
pacings, `count=200`. **Gated** invariants (kill-criterion-3): cross-FFI **ingest whole + ordered +
off-main**. **Recorded, not gated**: re-delivery completeness (`delivered`) — the generated
`callbackFlow`'s `trySend`-into-bounded-channel variance, deliberately unfrozen.

| pacing | ingested | delivered | ordered | off-main | p50 | p99 |
|---|---|---|---|---|---|---|
| burst (`delay_us=0`)  | **200/200** | 130/200 (stall@65) | yes | yes (1 thread) | 2664.6µs | 3109.7µs |
| paced (`delay_us=200`)| **200/200** | **200/200** (stall@200) | yes | yes (2 threads) | 166.4µs | 811.4µs |

**Verdict (two sentences).** Under CPU saturation the cross-FFI **ingest stays 200/200 and strictly
ordered on both pacings, and the consumer always resumes off the main thread** — the load-bearing seam
is sound under load, so kill-criterion-3 is NOT triggered (no stall, no reorder at ingest). **Re-delivery
completeness behaves exactly as M0 predicted**: burst + contention drops the `callbackFlow` to 130/200
(the `trySend` drop-on-overflow, now provoked by real CPU pressure rather than a slow collector), while
a 200µs pacing keeps delivery whole at 200/200 even under the same load — confirming the loss is the
Kotlin binding's overflow policy, not the native push.

Comparison to M0 (unloaded, fast collector): M0 delivered 200/200 on both pacings; under 2-core
saturation burst falls to 130/200 while paced holds. Latency is comparable (burst p50 2664µs under load
vs 2272µs unloaded; paced p50 is noise-level lower). The `delivered==200` gate remains correctly
**ungated** — it is flaky under load exactly as the M0 hand-off warned.

## N4 residual — nothing left (checked, not invented)

Per the M1 hand-off ("N4 gzip edge — rule-07 already green; M2/M3 should confirm content-length
honesty survives the file sink") and the M2 notes (gzip honesty confirmed for **both** the memory sink
— `content_length = Some(decoded len)` — and the file sink — `content_length = None`; upload progress
rule-11 green): **no N4 edge remains open for M3.** Both N4 obligations (transparent-gzip
content-length honesty, monotone upload progress) are green in `theFullSuiteIsGreenOnTheRealAdapter`
and were re-verified green in this run (14/14, failures=0). No new N4 work was needed or invented.

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **`ACCESS_NETWORK_STATE` granted in the conformance manifest (test-tier only).** Required by the N5
   HttpEngine probe — the platform Cronet engine queries ConnectivityService at init and crashes the
   process uncatchably without it (see the N5 finding). Test-tier only; the shipped OkHttp
   `android/bolted-http` needs neither this nor the existing cleartext policy. Smallest change that
   lets the probe reach a real verdict rather than a crash.
2. **N5 probe isolates `android.net.http.*` behind a `Class.forName` presence gate** (in `probeEngine`,
   annotated `@RequiresApi(34)`), so an absent class reads as a clean ABSENT and ART's lazy method
   verification never throws `NoClassDefFoundError` at load. Reflection is used only for presence; once
   present, direct typed calls (constructibility, the request) give richer facts than reflection would.
3. **The N5 h3/h2 leg is recorded and stopped, not forced.** Rather than sink >1h into installing a
   test anchor into Cronet (no cheap public API) and/or teaching the TestServer ALPN/QUIC (out of
   scope), the probe records the two concrete barriers (cert-authority rejection; server speaks only
   HTTP/1.1) and stops — the time-boxed discipline the step doc mandates.
4. **The under-load sweep gates ingest/order/off-main, records `delivered`.** A `delivered==200` gate
   under burst+load would be flaky (the M0 streaming-seam finding); gating it would contradict
   kill-criterion-3's "streaming seam deliberately unfrozen". Ingest wholeness + ordering is the
   un-droppable measure and IS gated.

## Friction log (freeze-agenda input)

- **F-M3-1 (this milestone) — HttpEngine init failure is async and uncatchable, and crashes the whole
  instrumentation.** A missing `ACCESS_NETWORK_STATE` surfaced as a `SecurityException` on Cronet's
  internal network thread, not at the `build()` call site — a `try/catch(Throwable)` around
  construction did nothing and 11 unrelated tests went down with it. **Freeze input:** if the engine
  matrix ever becomes real, engine selection must be gated on capability/permission *presence checks
  before construction* — a native adapter cannot defend against HttpEngine init the way it defends
  against a per-request `IOException`.
- **F-M3-2 — the conformance TestServer cannot exercise h2/h3 at all.** It is a raw HTTP/1.1 listener
  (no ALPN, no QUIC). This bounds what *any* Android engine probe (or the row-11 negotiated-version
  row) can assert about h2/h3 on this tier. **Freeze input:** a genuine multi-protocol conformance
  story (h2/h3 negotiation, the OkHttp-vs-HttpEngine engine matrix as more than paper) needs an
  h2/h3-capable test server; today the negotiated-version row can only ever observe HTTP/1.1.
- **F-M3-3 — Cronet reports `negotiatedProtocol='unknown'` for cleartext HTTP/1.1.** Not `'http/1.1'`.
  If an HttpEngine adapter ever mapped `Response.protocol` for row-11, it would need to treat
  `'unknown'`/`''` as HTTP/1.1 (or refuse to claim a version) rather than mis-report — a mapping gap
  the OkHttp path does not have (`Response.protocol` is precise there).

## M4 hand-off (the mutation pass — what M4 must know)

- **M3 added no shipped-adapter or FFI surface** — the M4 mutation targets are unchanged from the M2
  hand-off (pin split, downgrade refusal, file-sink atomic finalize, trace, cancel, progress, bridge
  token routing). The two new tests are androidTest-only probes.
- **The N5 probe is NOT a mutation target** and asserts only `SDK_INT>=34` as a hard gate (everything
  else is recorded); do not try to red-watch it — it is a detection probe, not a conformance row.
- **The under-load stream test gates only ingest-whole + ordered + off-main**; `delivered` is recorded.
  If M4 mutates the streaming/ingest path, the gated invariants here (and the M0 corruption control)
  are the live tripwires — a mutation that drops or reorders *ingest* should red both this test and
  `StreamProbe.theStreamIsOrderedLosslessComplete`.
- **Suite size is now 14** (`tests="14"` in the JUnit XML) — M4's baseline before it adds any blind-spot
  rows.
