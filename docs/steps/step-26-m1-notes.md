# Step 26 M1 — the Android base adapter (notes for M2+)

**Milestone:** M1 (the real `BoltedHttp.kt` OkHttp adapter). **Branch:** `step/26-android-adapter`.
Scope: the full C1 + C2 + extra-rows suite green on the real adapter **except** the M2 syntheses
(pinning + trust anchor, https→http refusal, file sink / `Io`) — those stay red and are M2. Every
M1-green row was watched red first. No FFI surface changed (it was already mirrored M1-ready from
Apple; strictly-additive requirement met with zero additions).

## Gate result

- `mise run check` — green (host, JDK-free; no Rust changed).
- `mise run test:android:http` — green on the headless `dev34` GMD (aosp_atd android-34 arm64):
  **`tests="8" failures="0" errors="0" skipped="0"`**, verified against the JUnit XML (not the wrapper
  exit code). 8 tests = 2 M0 bridge-gate + 3 N2 stream-probe (retained) + **3 new M1**:
  - `theM1RowsAreGreenExceptTheM2Syntheses` — the real adapter over C1 `rows()` + C2 `rows()` +
    `extra_rows()`; asserts every M1 row green and every M2 row red.
  - `theWatchedRedBaseline` — every M1-green row shown RED first under a broken adapter.
  - `theTotalDeadlineIsCallTimeoutNotPerIdle` — the sharp deadline red-watch (see the verdict below).

All row outcomes below are **observed** from the on-device per-test logcat
(`build/outputs/androidTest-results/managedDevice/debug/dev34/logcat-*.txt`), not derived.

## Row status table (real adapter — C1 rows() + C2 rows() + extra_rows())

| Row | Status | Watched-red evidence (observed message) |
|-----|--------|------------------------------------------|
| C1/rule-01 same-request-same-outcome | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-02 timeout-vs-cancel-distinct | **GREEN** | broken: `KeysNotDistinct { key: Transport }` |
| C1/rule-03 stalled-body-times-out | **GREEN** | broken: `WrongErrorKey { expected: Timeout, got: Transport }` |
| C1/rule-04 https-to-http-refused | RED → **M2** | `WrongErrorKey { expected: InsecureRedirect, got: Tls }` (good-https cert not yet trusted) |
| C1/rule-05 manual-if-none-match-304 | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-06 permitted-header-not-dropped | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-07 gzip-decoded-invariant | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-08 no-hidden-retry | **GREEN** | always-ok: `HiddenRetry { connections: 2 }` |
| C1/rule-09 cancel-completes-cancelled | **GREEN** | broken: `WrongErrorKey { expected: Cancelled, got: Transport }` |
| C1/rule-10 pin-mismatch-typed-error | RED → **M2** | `ExpectedSuccessGotError { got: Tls }` (good-https cert not yet trusted) |
| C1/rule-11 upload-progress-monotone | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/row-15 response-sink-correspondence | RED → **M2** | `WrongSink` (file sink ignored — memory only in M1) |
| C1/row-redirect-trace-final-url-and-hops | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/row-negotiated-version-observable | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/row-deadline-total-not-per-idle | **GREEN** | broken: `WrongErrorKey { expected: Timeout, got: Transport }` + the sharp per-idle red-watch below |
| C2/key-timeout | **GREEN** | broken: `WrongErrorKey { expected: Timeout, got: Transport }` |
| C2/key-cancelled | **GREEN** | broken: `WrongErrorKey { expected: Cancelled, got: Transport }` |
| C2/key-tls | **GREEN** | broken: `WrongErrorKey { expected: Tls, got: Transport }` |
| C2/key-name-resolution | **GREEN** | broken: `WrongErrorKey { expected: NameResolution, got: Transport }` |
| C2/key-connect | **GREEN** | broken: `WrongErrorKey { expected: Connect, got: Transport }` |
| C2/key-transport | **GREEN** | always-ok: `ExpectedErrorGotSuccess { expected: Transport, status: 200 }` |
| C2/key-too-many-redirects | **GREEN** | broken: `WrongErrorKey { expected: TooManyRedirects, got: Transport }` |
| C2/key-pin-mismatch | RED → **M2** | `WrongErrorKey { expected: PinMismatch, got: Tls }` |
| C2/key-insecure-redirect | RED → **M2** | `WrongErrorKey { expected: InsecureRedirect, got: Tls }` |
| C2/key-io | RED → **M2** | `ExpectedErrorGotSuccess { expected: Io, status: 200 }` (file sink ignored) |

**19 rows green, 6 rows deliberately red (M2).** Watched-red mechanism (mirrors Apple's step-25 M1):
`BrokenHttp` (always `Transport`) reds every green row except the two that *expect* `Transport`
(rule-08, key-transport); `AlwaysOkHttp` (always `200`) reds those two. Extra-rows are wired into the
M1 driver test (M0/Apple-M1 left them out); M1 turns the redirect-trace, negotiated-version, and
total-deadline extra rows green, leaving only the file-sink `row-15`.

**The deadline verdict (the headline — `/drip` evidence).** `callTimeout` is the **honest TOTAL
deadline; no timer synthesis is needed** — the opposite disposition to Apple (URLSession's
`timeoutInterval` is per-idle, so Apple synthesized a `DispatchSourceTimer`). Proof, both legs
observed on `C1/row-deadline-total-not-per-idle` driving `/drip?count=40&interval_ms=50` (≈2 s of
trickle) with a 300 ms deadline:

- **PerIdle mode** (a bare OkHttp `readTimeout`, the red-watch): `passed=false`,
  `ExpectedErrorGotSuccess { expected: Timeout, status: 200 }` — the idle-timer is reset by every
  dribbled byte, never fires, and the trickle runs to a `200` where the total deadline required
  `Timeout`. **Watched red.**
- **Total mode** (OkHttp `callTimeout`, the shipped default): `passed=true`. `callTimeout` is a
  wall-clock budget over the whole call (DNS→connect→TLS→request→response→redirects) and fires
  mid-trickle at the deadline. **Green.**

`DeadlineMode` is one construction flag on the adapter (`Total` default / `PerIdle` for the
conformance red-watch) — the adapter is not forked (the step-25 `classicLoading` pattern).

## What M1 built

**`android/bolted-http/src/main/kotlin/dev/bolted/http/BoltedHttp.kt`** — rewritten from the M0
one-row skeleton to the full base adapter:

- **C2 taxonomy classified by CAUSE, never by exception text.** A caller cancel is recorded on the
  per-request `Ctx.callerCancelled` **before** `Call.cancel()`, so it maps to `Cancelled` regardless
  of the opaque `IOException("Canceled")` the cancelled call throws; a `callTimeout` expiry is an
  `InterruptedIOException` → `Timeout`. Then by exception TYPE: `UnknownHostException` →
  `NameResolution`, `ConnectException` → `Connect`, any `SSLException` → `Tls`, everything else
  (post-connection) → `Transport`. The `Cancelled`/`Timeout` disambiguation (rule 9 / N6) is the
  reason the cause is recorded up front — the two are otherwise both `IOException`s on the same
  callback.
- **Total deadline = `callTimeout`** (see verdict). No constraint literal: the value is
  `FfiRequest.deadlineMs`.
- **Caller cancellation** (rule 9): `cancel(token)` from the bridge's (non-call) watcher thread sets
  the cause and cancels the `Call`.
- **Real negotiated version** from `Response.protocol` (`mapVersion`) — the M0 `HTTP1_1` placeholder
  is gone (row 11). Test server speaks HTTP/1.1 → `HTTP1_1`.
- **Redirect hop trace** from the `priorResponse` chain (`redirectHops`): walk final→prior, collect
  each prior's `request.url`, reverse to first-hop-first, exclude the final URL. `/redirect-chain?n=2`
  → hops `[n=2, n=1]`, final `n=0` (observed green).
- **Upload progress** (rule 11, N4): `ProgressBody` wraps the request body in a counting
  `ForwardingSink` and reports the **monotone cumulative bytes actually flushed** via
  `reportProgress`; a terminal `(total,total)` sample is emitted on success if the flush stopped
  short. Reporting real flushed bytes (not the content-length up front) is what avoids the
  buffer-jump-to-100% failure mode.
- **`retryOnConnectionFailure(false)`** (rule 8): no hidden request-level retry. `followRedirects` /
  `followSslRedirects` stay on (they feed the `priorResponse` hop trace and OkHttp's own follow-up
  cap → `TooManyRedirects`).

**`android/bolted-http-conformance/.../Adapters.kt`** — added `AlwaysOkHttp` (always `200`) alongside
the M0 `BrokenHttp`, so the two Transport-expecting rows can be watched red.

**`android/bolted-http-conformance/.../M1Conformance.kt`** — the three M1 tests.

**No FFI/Rust change.** `crates/bolted-http-android-ffi/src/lib.rs` was already mirrored M1-ready from
Apple (the `FfiHttpVersion` enum, `FfiResponse.http_version`/`hops`, `FfiHttpError.TooManyRedirects`,
`HttpAdapter::cancel`, `HttpHarness::report_progress`, `run_c2`, `run_extra_rows`). The
"strictly-additive; prefer none" rule is met with **zero** additions.

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **rule-05 (manual 304) is an M1 green.** OkHttp with no `Cache` installed returns a conditional
   `304` straight through (no cache synthesis) — same free-304 as Apple's ephemeral session. One
   fewer M2 row. Observed green.
2. **rule-07 (gzip) is an M1 green.** OkHttp's transparent gzip decodes the body and strips
   `Content-Encoding`/`Content-Length`; the adapter forwards the decoded bytes, and the Rust bridge
   computes `content_length = Some(body.len())` from the **decoded** body — honest (decoded length,
   never the compressed figure). Nothing in the adapter can ship a lying content-length for a memory
   sink, so "report None-or-honest" holds by construction. Observed green.
3. **`TooManyRedirects.limit` is the sentinel `0`** (mirrors Apple's F-M1-1): OkHttp enforces its own
   internal follow-up cap; the request carries no redirect limit and the delegate-driven policy is
   M2, so there is no honest request-side value. No row inspects it, only the key.
4. **Too-many-redirects is matched by the `ProtocolException` message prefix** — the one unavoidable
   text match; see friction F-M1-2.
5. **The M2 rows fail as `Tls` on Android M1, not `ExpectedErrorGotSuccess` (Apple's shape).** M1
   installs no trust anchor, so the good-https endpoint (rule-04, rule-10, key-pin-mismatch,
   key-insecure-redirect all target it) is rejected by the default Android `TrustManager` →
   `SSLHandshakeException` → `Tls`. This is the honest M1 state (the endpoint genuinely is untrusted
   until M2 installs `ServerInfo.goodCertDer`), and the rows are still red — just for a different
   reason than Apple. **M2 must install the anchor first**, then the pinning/redirect syntheses become
   the operative failure/refusal. Recorded so M2 does not mistake the `Tls` message for a bug.
6. **`Metrics` tier stays `Phase`, no live `EventListener` wired.** The factory already reports
   `MetricsTier::Phase` and it is honest: OkHttp's `EventListener` demonstrably exposes
   dns/connect/TLS/first-byte phase timings. No M1 row consumes metric *values* (C3 is M2), and a live
   listener would need a metrics re-entry FFI surface to be testably honest — additive FFI work, out
   of M1's "prefer no FFI change" scope. Left as-is and recorded for M2/M3 (the "leave the tier claim
   as-is" branch the step doc allows).

## Rows deliberately left for M2

`rule-04` (https→http refusal), `rule-10` + `key-pin-mismatch` (SPKI pinning), `key-insecure-redirect`
(refusal), `key-io` + `row-15` (file sink). All four https-targeting rows first need the **trust
anchor** installed from `ServerInfo.goodCertDer` (decision 5). `PermissionDenied` is not a driver row
(`c2::reachability` marks it `AdapterOnly`) — its control is an M2 deliverable. C3 Android column is
M2.

## Friction log (freeze-agenda input — friction matters more than usual this step)

- **F-M1-1 — the redirect limit has no honest source on OkHttp** (same shape as Apple F-M1-1).
  `HttpError::TooManyRedirects` carries `limit: u32`, but OkHttp's follow-up cap is internal and the
  request carries no redirect limit. Android reports the sentinel `0`. **Freeze question (sharpened):**
  should the redirect ceiling be a composition-root CFG rather than request data, and should the key
  carry `limit` at all if neither URLSession nor OkHttp can report it?
- **F-M1-2 — OkHttp signals too-many-redirects ONLY via a `ProtocolException` message.** There is no
  typed code (Apple had `URLError.httpTooManyRedirects`). Worse, `/truncate`'s premature-EOF also
  throws `ProtocolException` — so type alone cannot tell the two apart; the classifier matches the
  `"Too many follow-up requests"` prefix. This is the single unavoidable text match in the adapter
  (the timeout-vs-cancel disambiguation the rule actually targets is fully by-cause). Fragile across
  OkHttp versions / locales. **Freeze input:** the redirect-ceiling-as-CFG question (F-M1-1) would let
  the adapter enforce the cap itself and emit a typed cause, removing the text match entirely.
- **F-M1-3 — `content_length` honesty is a memory-sink accident** (inherited from Apple F-M1-3, still
  true on Android). `Some(body.len())` is honest only because the whole decoded body is buffered. The
  file sink (M2) and streaming (row 16) have no in-memory body; content-length honesty under decoding
  becomes a real question there. The FFI carries no `content_length` field.
- **F-M1-4 — poll-based cancellation ⇒ a thread per request** (inherited; the bridge's 10 ms watcher).
  Fine for a conformance harness, a smell for a shipped adapter. **Freeze question:** a
  push/registration cancellation seam on the contract removes the poll for every native adapter.
- **F-M1-5 — the good-https endpoint is untrusted until M2.** Every https row is red as `Tls` in M1
  (decision 5). Not a bug, but it means M1 exercises no *successful* TLS path at all — the first real
  TLS success lands in M2 with the anchor. Recorded so M2 sequences anchor-install before pinning.
- **F-M1-6 — GMD JUnit XML captures no `<system-out>`.** Unlike the SwiftPM tier, the Android GMD's
  `TEST-*.xml` has no system-out element, so `println` evidence is not in the gated XML. The observed
  row messages live in the retained per-test logcat files
  (`build/outputs/androidTest-results/.../logcat-*.txt`) — that is where this report's messages come
  from. The XML gate still works (it reads `failures`/`errors` attributes); only the human-readable
  detail moved. Recorded so M2+ pulls evidence from logcat, not the XML.

## M2 hand-off (the red rows + their machinery)

- **Install the trust anchor FIRST** (`ServerInfo.goodCertDer`): a custom `X509TrustManager` /
  `SSLSocketFactory` (or OkHttp `CertificatePinner` sits on top of trust, not instead of it) so the
  good-https endpoint verifies. This flips rule-04/rule-10/key-pin-mismatch/key-insecure-redirect from
  `Tls` to their real M2 outcomes. The untrusted endpoint must stay rejected (the `key-tls` control).
- **rule-10 / key-pin-mismatch** — enforce `FfiRequest.pins` (already crossing) with the Linux/Apple
  split: chain+hostname fail ⇒ `Tls`, declarative SPKI mismatch on a *passing* chain ⇒ `PinMismatch`.
  N3 also requires the two fragility controls (NSC `<pin-set>` evidence; the 2-arg
  `checkServerTrusted` hostname-less landmine).
- **rule-04 / key-insecure-redirect** — `followRedirects(false)` + a manual redirect loop (or an
  interceptor) that refuses `https→http` with `InsecureRedirect`; the hop trace already works via
  `priorResponse`, so decide whether the manual loop supersedes it or coexists.
- **key-io / row-15** — `FfiRequest.sink == File{path}` (already crossing): stream the body to the
  path, a write failure ⇒ `Io`, report `sink_path` so the core builds a `File` outcome.
- **N4 gzip edge** — rule-07 already green; M2/M3 should confirm content-length honesty survives the
  file sink (F-M1-3).
- **C3 Android column** — `PriorityHint` absent, `Metrics` tier `Phase`.
