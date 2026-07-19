# Step 25 M0 — packaging + the harness bridge (notes for M1+)

**Milestone:** M0 (packaging + the harness bridge). **Branch:** `step/25-apple-adapter`.
Scope was M0 only: the FFI crate, the walking-skeleton Swift adapter (one C1 row), the structured
driver, server lifecycle, mise wiring, and the fail-able gate. M1–M4 are untouched.

## Gate result — both halves

- **GREEN:** `C1/rule-01-same-request-same-outcome` passes on the real `BoltedHttp` URLSession
  adapter, end to end through the FFI (`swift test --package-path apple/bolted-http-conformance`,
  test `testC1Rule01IsGreenOnTheRealAdapter`).
- **RED:** the same row goes red under a deliberately-broken adapter variant, with a legible typed
  message (`testC1Rule01IsRedWithABrokenAdapter`).
  - **How it was broken:** a `BrokenHttp` class in the test target (isolated; the shipped
    `BoltedHttp` is untouched) whose `execute` never performs a request — it immediately calls
    `harness.completeErr(token:error: .transport(...))`. rule-01 expects a successful GET of `/ok`,
    so the blanket failure makes it red with the structured driver reporting
    `ExpectedSuccessGotError { got: Transport }`. Restoration is automatic: the green test uses the
    real adapter in the same suite run.

## Built

- **`crates/bolted-http-apple-ffi`** (workspace member, like `gen-profile-ffi`) — the harness
  bridge. Depends on `bolted-http` with the `conformance` feature, so it links the suite rows + the
  in-process TLS `TestServer`. NOT the shipped consumer adapter; it is the conformance/test bridge.
- **`apple/bolted-http`** — the BUNDLED SwiftPM package (pack output). Holds the hand-written
  `Sources/BoltedHttp/BoltedHttp.swift` + the generated `Sources/BoltedHttp/BoltFFI/*.swift` + the
  `BoltedHttpApple.xcframework`. Its `Package.swift` is pack-generated (module/product
  `BoltedHttpApple`).
- **`apple/bolted-http-conformance`** — the `swift test` consumer package (one path dependency on
  `../bolted-http`) with the XCTest gate.
- **mise:** `pack:apple:http`, `test:apple:http`; `test:apple` extended (`depends` now packs both
  dists, and runs the conformance package too). `mise run check` unchanged (host-only, Xcode-free) —
  the new crate is a plain Rust workspace member it clippies/tests, no pack in the graph.

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **Two Swift packages, not one.** The bundled layout regenerates `apple/bolted-http/Package.swift`
   on every pack, so a test target added there would be clobbered. The tests live in a sibling
   consumer package (`apple/bolted-http-conformance`) — exactly how the packaging report's open
   question ("where do app-added targets go?") resolves. This is the spike's proven `package/` +
   `consumer/` shape, with the consumer relocated under `apple/`.
2. **Pack `output = "../../apple/bolted-http"`.** A `..` in the bundled `output` works (verified — the
   package landed at the repo's `apple/` root, adapter source preserved). This lets the shippable
   package live under `apple/` (repo convention) rather than inside the crate dir.
3. **The data enum is `FfiHttpError`, not `FfiError`.** BoltFFI's `error_style = "throwing"` reserves
   the Swift type name `FfiError` (its thrown-error wrapper); a `#[data] enum FfiError` collides
   ("ambiguous for type lookup"). Renamed to `FfiHttpError`. **M1+ and future FFI crates: never name
   a `#[data]` type `FfiError` under the throwing error style.**
4. **Driver runs the eleven C1 rows only** (`c1::rows()`), not C1 extra-rows / C2 / C3. M0 needs one
   green C1 row; wiring C2/C3 and the C1 sink/redirect rows is M1+. The driver mechanism is general —
   adding row sets is a one-line change in `run_c1`.
5. **HTTP version reported as `Http1_1` unconditionally, deadline via URLSession `timeoutInterval`.**
   Both are M0 placeholders (the version observable and a real synthesized total deadline are M1).

## The bridge shape M1 must build on (exact API)

**Rust FFI surface** (`bolted_http_apple_ffi`):

- Callback trait Swift implements: `#[export] trait HttpAdapter { fn execute(&self, request: FfiRequest); }`.
- Exported harness `HttpHarness`:
  - `new(adapter: Arc<dyn HttpAdapter>) -> Self`
  - `start_server() -> ServerInfo` (three base URLs) / `stop_server()`
  - `complete_ok(response: FfiResponse)` / `complete_err(token: u64, error: FfiHttpError)` — the
    completion re-entry points
  - `run_c1() -> Vec<RowReport>` (id / passed / skipped / message)
- Data (`#[data]`): `FfiRequest{token,method,url,headers:[FfiHeader],body,deadline_ms}`,
  `FfiResponse{token,status,headers,body,final_url}`, `FfiHeader{name,value}`,
  `FfiHttpError{Timeout|Cancelled|NameResolution|Connect|Tls|Transport{message}}`,
  `ServerInfo`, `RowReport`.

**How Swift registers the adapter** (the composition-root dance, in the XCTest):

```swift
let adapter = BoltedHttp()            // 1. adapter first
let harness = HttpHarness(adapter: adapter)  // 2. harness second (takes the adapter)
adapter.harness = harness             // 3. weak back-reference so completions re-enter
```

**The internal wiring** M1 extends: the suite calls `factory.new_adapter()` → a `SwiftAdapter` shim
whose `Http::send` mints a token, parks the row's `CompletionSink` (and any `UploadProgressSink`) in
a token-keyed `Mutex<HashMap>`, converts the request, and calls `adapter.execute`. The Swift
completion re-enters `complete_ok`/`complete_err`, which look up the token, convert back, and deliver
to the parked sink. Blocking model: the row parks on `recv_timeout` on the driver thread; URLSession
completions arrive on a background thread (no deadlock — confirmed by the green run).

## What M1 must add (from the driver's red rows today)

Running `run_c1` on the M0 skeleton, only rule-01 is green. The reds are the M1 work list:

- **Cancellation not wired to Swift** → rule-02 (cancel path) and rule-09 report `NoCompletion` after
  the budget. `send` returns a fresh `CancelToken` that nothing observes; M1 must forward cancel
  across the FFI (a token → `URLSessionTask.cancel()` map) and map `URLError.cancelled → Cancelled`.
- **No deadline synthesis** → rule-03 leans on URLSession's per-idle `timeoutInterval`, which is not
  the contract's total deadline (A3 hazard). M1 synthesizes the total deadline.
- **Progress never reported** → rule-11 is `ProgressNotTerminal` (the parked `UploadProgressSink` is
  unused in M0).
- **Trust anchors / pinning / https→http / 304 / gzip / header-echo** → the HTTPS rows fail on the
  self-signed test cert (no trust anchor wired), and the C2 error keys need the full mapping. All M1+.
- `ServerInfo` exposes the three base URLs but M1 will also need the good-cert DER + SPKI pins across
  the FFI for the HTTPS/pinning rows (the harness has them via `Endpoints`; not yet exported).

## Friction log (freeze-agenda input — friction matters more than usual this step)

- **F-M0-1 — `FfiError` is a reserved Swift name under throwing error style.** See decision 3. A
  `#[data]` type named `FfiError` compiles in Rust and packs cleanly, then fails only at `swift test`
  with "ambiguous for type lookup". Cost: one pack+build cycle. Worth a `bolted new` / BoltFFI lint.
- **F-M0-2 — bundled `output` with `..` is undocumented but works.** The packaging report only showed
  an in-crate `output`. Pointing it at `../../apple/bolted-http` (outside the crate) succeeded and
  preserved the wrapper source. Recorded so it is not re-discovered.
- **F-M0-3 — the bundled layout cannot host its own test target** (decision 1). Not a blocker, but it
  means every shipped adapter package needs a sibling test package; worth a scaffolding convention.
- **F-M0-4 — `setup:boltffi` cannot detect a git-vs-registry CLI.** On this machine
  `cargo install --list` showed `boltffi_cli v0.27.5` from the killed step-23 **git** `?rev=`, but
  `setup:boltffi` early-exits on the version-string match and would never correct it. Fixed manually
  with a forced registry reinstall (`CARGO_HOME=<canonical> cargo install boltffi_cli --version
  0.27.5 --force`). The task's version-only guard is a real gap for anyone with a git build installed.
