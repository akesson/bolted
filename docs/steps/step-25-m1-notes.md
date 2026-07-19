# Step 25 M1 — the adapter (notes for M2+)

**Milestone:** M1 (the real `BoltedHttp.swift` URLSession adapter). **Branch:** `step/25-apple-adapter`.
Scope: the full C1 + C2 suite green on the real adapter **except** the A2/A4 syntheses (file sink,
pinning, hop trace, https→http refusal, Io) — those stay red and are M2. Every M1-green row was
watched red first.

## Gate result

- `mise run check` — green (host, Xcode-free; the FFI crate clippies/tests clean).
- `mise run test:apple:http` — green: 4 XCTest methods, 0 failures.
  - `testC1Rule01IsGreenOnTheRealAdapter` / `…IsRedWithABrokenAdapter` — the M0 bridge fail-ability
    gate, retained.
  - `testM1RowsAreGreenExceptTheM2Syntheses` — the real adapter over C1 + C2; asserts every M1 row
    green and every M2 row red.
  - `testWatchedRedBaseline` — every M1-green row shown RED first under a broken adapter.

## Row status table (real adapter, C1 rows() + C2 rows())

| Row | Status | Recorded RED (watched-red evidence) |
|-----|--------|-------------------------------------|
| C1/rule-01 same-request-same-outcome | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-02 timeout-vs-cancel-distinct | **GREEN** | broken: `KeysNotDistinct { key: Transport }` |
| C1/rule-03 stalled-body-times-out | **GREEN** | broken: `WrongErrorKey { expected: Timeout, got: Transport }` |
| C1/rule-04 https-to-http-refused | RED → **M2** | `ExpectedErrorGotSuccess { expected: InsecureRedirect, status: 200 }` (URLSession auto-followed the downgrade) |
| C1/rule-05 manual-if-none-match-304 | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-06 permitted-header-not-dropped | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-07 gzip-decoded-invariant | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/rule-08 no-hidden-retry | **GREEN** | always-ok: `HiddenRetry { connections: 2 }` |
| C1/rule-09 cancel-completes-cancelled | **GREEN** | broken: `WrongErrorKey { expected: Cancelled, got: Transport }` |
| C1/rule-10 pin-mismatch-typed-error | RED → **M2** | `ExpectedErrorGotSuccess { expected: PinMismatch, status: 200 }` (no pin enforcement yet) |
| C1/rule-11 upload-progress-monotone | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C2/key-timeout | **GREEN** | broken: `WrongErrorKey { expected: Timeout, got: Transport }` |
| C2/key-cancelled | **GREEN** | broken: `WrongErrorKey { expected: Cancelled, got: Transport }` |
| C2/key-tls | **GREEN** | broken: `WrongErrorKey { expected: Tls, got: Transport }` |
| C2/key-name-resolution | **GREEN** | broken: `WrongErrorKey { expected: NameResolution, got: Transport }` |
| C2/key-connect | **GREEN** | broken: `WrongErrorKey { expected: Connect, got: Transport }` |
| C2/key-transport | **GREEN** | always-ok: `ExpectedErrorGotSuccess { expected: Transport, status: 200 }` |
| C2/key-too-many-redirects | **GREEN** | broken: `WrongErrorKey { expected: TooManyRedirects, got: Transport }` |
| C2/key-pin-mismatch | RED → **M2** | `ExpectedErrorGotSuccess { expected: PinMismatch, status: 200 }` |
| C2/key-insecure-redirect | RED → **M2** | `ExpectedErrorGotSuccess { expected: InsecureRedirect, status: 200 }` |
| C2/key-io | RED → **M2** | `ExpectedErrorGotSuccess { expected: Io, status: 200 }` |

Watched-red mechanism: `BrokenHttp` (always `Transport`) reds every row except the two that *expect*
Transport (rule-08, key-transport); `AlwaysOkHttp` (always `200`) reds those two. The Rust suite's own
per-row red-twins (in `c1.rs`/`c2.rs`, green under `mise run check`) are the independent proof each
row can fail against a mutated mock.

**`C2/key-permission-denied` is not a driver row** — `c2::reachability` marks it `AdapterOnly` (no host
control), so it is absent from `c2::rows()`. Its positive control is an M2 deliverable (App-Sandbox /
ATS-style denial), per the step doc.

**`c1::extra_rows()` (row-15 response-sink correspondence, redirect-trace final-url/hops) is
deliberately NOT wired into the driver** — both need M2 machinery (file sink, hop trace). M2 wires
`run_extra_rows` and turns them green.

## What M1 built

**FFI crate (`crates/bolted-http-apple-ffi/src/lib.rs`)** — additive mirror growth only:
- `FfiHttpVersion` enum + `FfiResponse.http_version` field — drops the M0 `Http1_1` placeholder; the
  adapter reads `URLSessionTaskMetrics.networkProtocolName`.
- `FfiHttpError::TooManyRedirects { limit: u32 }` — the new reachable key; maps to
  `HttpError::TooManyRedirects`.
- `ServerInfo` gains `good_cert_der`, `good_spki`, `untrusted_spki` (the TLS material; `good_cert_der`
  is consumed now as the trust anchor, the SPKI hashes cross for M2 pin enforcement).
- `HttpAdapter::cancel(token)` — the new callback-trait entry point for caller cancellation.
- `HttpHarness::report_progress(token, sent, total)` — upload-progress re-entry (does **not** consume
  the pending entry; progress is repeatable).
- `HttpHarness::run_c2()` alongside `run_c1()` (shared `run_rows`).
- `SwiftAdapter::send` now bridges the poll-based `CancelToken` to `adapter.cancel(token)` via a
  detached 10 ms watcher that self-terminates when the request completes (the Linux adapter's poll,
  mirrored).
- `to_http_response` sets `content_length = Some(body.len())` (honest for a `Memory` sink) and maps the
  real version.

**Swift adapter (`apple/bolted-http/Sources/BoltedHttp/BoltedHttp.swift`)** — rewritten from the M0
completion-handler skeleton to a delegate-driven `URLSession` (`NSObject` + `URLSessionDataDelegate`):
- **Total-deadline synthesis** (rule 3, A3 hazard): a `DispatchSourceTimer` over the whole request;
  `timeoutInterval` is deliberately *not* derived from the deadline (it is per-idle).
- **Cancel-vs-timeout by cause** (rule 2/9): a per-request `Termination` (`.deadline` / `.callerCancel`)
  set before `task.cancel()`; `URLError.cancelled` classifies on that cause, never on error shape.
- **Full C2 error mapping** — `URLError` → the typed key set (DNS, connect, TLS, timeout, cancelled,
  too-many-redirects, transport).
- **Upload progress** (rule 11) — `didSendBodyData` forwarded through `reportProgress`; a terminal
  `(total,total)` sample is emitted on success if the OS-fed stream stopped short (monotone, honest).
- **Server trust** — anchor-only `SecTrust` evaluation against `ServerInfo.goodCertDer`; the untrusted
  endpoint falls through to default handling → a real TLS rejection (the `key-tls` control).
- **Real HTTP version** from metrics; token↔task correlation via `taskDescription`.

**Tooling (`mise.toml`, F-M0-4 fix)** — `setup:boltffi`'s early-exit guard now inspects
`cargo install --list` for a `?rev=` on the `boltffi_cli` line and force-reinstalls from the registry
when a git build is found (a git build reports the same `0.27.5` version string). Verified: on the
clean registry install it early-exits "already installed (registry)".

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **rule-05 (manual 304) is an M1 green, not M2.** The step doc listed "304" under M2, but the
   ephemeral `URLSession` produces a real 304 for a manual `If-None-Match` with no extra work (exactly
   the A3 note's prediction). Left green; one fewer M2 row.
2. **`content_length` is computed Rust-side as `Some(body.len())`, not carried over the FFI.** Honest
   for a `Memory` sink (`Some(n)` promises `n` decoded bytes), and it satisfies rule 7's decoded-gzip
   length check without ever reporting the compressed transport figure. When the file/streaming sink
   lands (M2/streaming), the body is no longer in memory and this needs revisiting → friction F-M1-3.
3. **`TooManyRedirects.limit` is the sentinel `0`.** URLSession enforces its own internal redirect cap;
   the contract request carries no redirect limit and the delegate-driven policy is M2, so there is no
   honest request-side value. `0` = "adapter-internal cap fired". No row inspects it. → friction F-M1-1.
4. **`FfiHttpError` grew only the M1-reachable keys.** Pin / insecure-redirect / permission / Io keys
   attach in M2 (additive) — a key with no reachable control now would be a green needle.
5. **Cancel bridged by a per-request poll thread**, mirroring the Linux adapter's 10 ms poll — the
   contract's `CancelToken` is poll-only. → friction F-M1-4.

## Friction log (freeze-agenda input — friction matters more than usual this step)

- **F-M1-1 — the redirect limit has no honest source on URLSession.** `HttpError::TooManyRedirects`
  carries `limit: u32`, but URLSession's cap is internal/unexposed and the request carries no redirect
  limit. Linux gets it from `LinuxHttpConfig.redirect_limit` (a CFG). **Freeze question:** should the
  redirect ceiling be a composition-root CFG (not request data), and should `TooManyRedirects` even
  carry `limit` if a platform can't report it? The Apple adapter reports the sentinel `0`.
- **F-M1-2 — the M2 list was one row pessimistic (rule-05 / 304).** The ephemeral-session 304 is free.
  Positive finding; the A3 note was right.
- **F-M1-3 — `content_length` honesty is a memory-sink accident.** Reporting `Some(body.len())` is only
  honest because the whole decoded body is buffered. The file-sink (M2) and streaming (row 16) outcomes
  have no in-memory body, so content-length honesty under decoding becomes a real question there. The
  FFI carries no `content_length` field yet — M2 may need to add one (from `expectedContentLength`
  filtered by `Content-Encoding`), or keep reporting `None`.
- **F-M1-4 — poll-based cancellation ⇒ a thread per request.** The `CancelToken` is poll-only, so the
  bridge spawns a 10 ms watcher per request (self-terminating). Fine for a conformance harness; a smell
  for a shipped adapter. **Freeze question:** a push/registration cancellation seam on the contract
  (a `Waker`-like callback) would remove the poll for every native adapter, not just Apple.
- **F-M1-5 — header fidelity is `String(describing:)` over `allHeaderFields`.** Works for the ASCII
  test headers; multi-value and non-UTF-8 header fidelity is untested and unspecified. No row exercises
  it. Worth a contract note on header representation across the FFI.
- **F-M1-6 — URLSession auto-decodes gzip transparently.** rule 7 passes with no manual inflate; the
  adapter never sees the compressed bytes. Recorded so M2/streaming does not re-discover it.
- **F-M1-7 — URLSession retains its delegate.** `BoltedHttp` ↔ its `URLSession` form a retain cycle
  (URLSession strongly holds the delegate until invalidated), so the adapter never deallocs. Acceptable
  for a whole-run test bridge; a shipped adapter must `finishTasksAndInvalidate`. The FFI-owned adapter
  lifetime hides this today.
- **F-M1-8 — link-time deployment-target warnings.** `ld` warns the static lib objects were built for
  macOS 26.5 but linked at 14.0 (`ring`/crypto objects). Cosmetic; the test links and runs. Comes from
  the pack's `deployment_target = 16.0` vs the SwiftPM host slice. Not a blocker; recorded.
- **F-M1-9 — cleartext to 127.0.0.1 works under the test host's ATS.** The SwiftPM XCTest bundle on
  macOS loads `http://127.0.0.1` with no `NSAppTransportSecurity` plist (rule-01 was already green in
  M0). Relevant when the iOS device tier / a real app bundle lands — those may need an ATS exception
  for the loopback cleartext endpoints.

## M2 hand-off (the red rows + their machinery)

- **rule-04 / C2 key-insecure-redirect** — implement `urlSession(_:task:willPerformHTTPRedirection:…)`:
  refuse `https→http` with `InsecureRedirect`, capture the hop trace + final URL (the redirect-trace
  `extra_rows` row), and cap the chain. Needs a new `FfiHttpError::InsecureRedirect { to }` (+ hop/
  final-url fields on `FfiResponse`).
- **rule-10 / C2 key-pin-mismatch** — enforce the request's SPKI pins in the trust delegate on top of
  the anchor evaluation (mirror the Linux `PinningVerifier` split: pin fail ⇒ `PinMismatch`, trust fail
  ⇒ `Tls`). The pins already cross via `ServerInfo.good_spki`/`untrusted_spki`, but the **request's**
  `PinSet` is not yet mirrored into `FfiRequest` — M2 adds it.
- **C2 key-io** — `ResponseSink::File` via `downloadTask` (A2), write failure ⇒ `Io`. Needs the sink
  selector mirrored into `FfiRequest` and `FfiResponse::File`, plus wiring `c1::extra_rows()` (row-15).
- **C2 key-permission-denied** — the App-Sandbox / ATS positive control, watched red first.
- **C3 Apple column** — `PriorityHint` present, `Metrics` tier `Phase` (URLSessionTaskMetrics).
