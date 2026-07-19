# Step 24 — report: bolted-http I (S-CONF + S-FFI + S-LX)

**Status: done — no kill criteria hit; both freeze gates cleared the good way.**
Second step under the Fable-orchestrates model; five Opus sub-agent milestones (M0, M1, an
inserted M1.5, M2 in a parallel worktree, M3, M4), report by the planning session. All work
on `step/24-http-harness` (M0 `9b2f79e` → M4 `34a157c`), merged to `design/bolted-http`.

## What was built

- **The contract types** (`bolted-http`, lib target dependency-free, `#![forbid(unsafe_code)]`):
  `HttpRequest` (method, https-only `Url` with `cleartext_dev` escape, guarded headers, body
  `Empty|Bytes|File(FileRef)`, required total deadline, `ResponseSink{Memory|File}`, `PinSet`,
  `Priority`), `HttpResponse` (status, headers, `BodyOutcome`, final URL, hop trace,
  non-optional `HttpVersion`, `content_length: Option<u64>`), `HttpError` (typed variants +
  `HttpErrorKey` stable strings), `Http`/`CompletionSink` (one-shot by construction —
  `complete(self: Box<Self>, …)`), `UploadProgressSink`, `CancelToken`/`RequestHandle`,
  optional `PriorityHint` + `Metrics` (tiered). The **`MaybeSend` seam** is the single
  target-conditional point (decided 2026-07-19); only the native arm compiles until a wasm
  target exists.
- **The conformance suite** (feature `conformance`, zero deps on the default build): the
  eleven §7 rules as C1 rows + two C1-adjacent rows (row-15 sink correspondence; redirect
  trace — M4's blind-spot fix), C2 with a positive control per reachable key, C3 generated
  from the capability types with pinned expectations, the scriptable socket-capable mock, and
  the local test server (three listeners: cleartext / good-TLS / untrusted-TLS; echo, delay,
  stall, truncate, flaky, etag, gzip, 401, insecure-redirect, chain, loop; rcgen certs,
  ring-only — no C toolchain in `check`). **Every row has a watched-red twin.**
- **The reference adapter** (`bolted-http-linux`, reqwest/rustls, owns its tokio runtime):
  full suite green (C1 13/13 incl. the M4 row, C2 10/10, C3 pinned). One deadline races the
  whole redirect chain; timeout-vs-cancel classified by cause; manual redirect follow for the
  hop trace; atomic temp-file+fsync+rename sinks; monotone upload progress; retry off
  (reqwest 0.13 protocol-NACK retry disabled explicitly).
- **The mutation table** (`crates/bolted-http/docs/conformance-mutation-table.md`): 26
  mutations across both implementors; 24 caught, 2 survivors both discharged as
  semantically-unobservable (hypothesis 2), 1 genuine blind spot fixed (redirect-trace
  observables had no positive control), 1 positive-control gap filled
  (`ProgressNotTerminal`).

## The two verdicts (the step's reason to exist)

1. **Row 16 — response streaming: CORE, mechanism `ffi_stream` async push (F1).** All three
   step-02 stream shapes deliver 100/100 with no stall at boltffi 0.27.5 inside a real http
   round-trip — F1 is the exact machinery that stalled at 15/100 on 0.27.3. F1 recommended
   over the ~100×-faster F2 callback push because bodies need ordered/lossless delivery and
   F1's built-in async hop keeps the consumer off the producer thread (the step-02 §4
   re-entrancy caution); F2 recorded as the perf alternative, F3 is the coalescing shape,
   wrong for bodies. Numbers in `crates/spike-http-ffi/docs/sffi-streaming-verdict.md`.
   Probes ran against the **registry** 0.27.5 CLI in a scratch root (the machine-global CLI
   is step-23's killed git build reporting the same version string — discriminated by
   `cargo install --list`).
2. **Row 19 — SPKI pinning on Linux: feasible; CORE(adapter) stands.** rustls expressed it
   cleanly: custom `ServerCertVerifier` = real `WebPkiServerVerifier` chain+hostname
   verification AND the request's SHA-256-SPKI pins; mismatch ⇒ `PinMismatch`, trust failure
   ⇒ `Tls`. No demote.

## Deviations from the step doc

- **M1.5 inserted** (design session, 2026-07-19): M1 surfaced two contract gaps honestly
  (rule 11 had no progress surface; `Io` unreachable without a request-side sink) — resolved
  as matrix-backed CORE shapes (`ResponseSink`, `UploadProgressSink` as a fourth `send`
  parameter, `content_length` observable), not ad hoc §9 resolutions.
- The eleven §7 rows stay pristine; matrix-row and blind-spot rows live in `extra_rows()`
  (13 C1 rows total). The M4 redirect-trace row is suite strengthening, not contract change.
- Two harness-mechanical M3 fixes: test certs gained a `127.0.0.1` IP SAN (real hostname
  verification must accept loopback); the server exposes its cert DER as a trust anchor
  (WebPKI cannot trust by SPKI allowlist). Mock semantics unchanged.

## Decisions recorded by the milestones (review at the freeze)

- **`HttpError` is a typed enum with `key()`**, deliberately diverging from bolted-core's
  `ErrorData{key, params}` — closed adapter-mapped taxonomy buys exhaustiveness + typed
  params; `as_str()` keeps the stable vocabulary. **Freeze-session agenda item.**
- `QuotaExceeded` deliberately omitted (background-family key; unreachable = a permanently
  green needle). Pinning is request-carried data; config-side placement stays open.
- Reserved-header compile guard via const-eval assert (the `http`-crate idiom) — recorded as
  a justified reading of the no-panic rule; runtime path (`parse`) is fallible.
- Mock completes via worker thread (cancel needs the handle back), still runtime-free;
  `Instant::now` carries scoped allows in executor-side code only (the clippy ban targets
  the sans-io core).
- Adapter: client-per-request (pin data lives in the TLS config; also precludes pooled
  retry); explicit trust-roots config at the composition root (CFG row 25), no webpki-roots
  dep; gzip only (brotli one line away; zstd skipped — needs a C compiler).

## Friction log (aggregated from the sub-agent reports)

- **reqwest 0.13 retries protocol NACKs by default** — found because rule 8's control is a
  server-side hit counter, not a client-side belief. `retry::never()` +
  `pool_max_idle_per_host(0)`; note for any future pooling adapter: the suite structurally
  cannot exercise the pooled-retry condition under client-per-request (defense-in-depth,
  unverified by construction — M4's A6 analysis).
- reqwest exposes no typed DNS error — the DNS-vs-connect split walks the error `source`
  chain with message markers. Brittle; revisit if reqwest gains typed connect errors.
- `content_length` honesty needed contract surface before rule 7 could pin it (M1.5) — a
  rule is only as checkable as the observables the contract exposes.
- The stale-diagnostics lesson repeats: mid-agent rust-analyzer snapshots are not the
  committed state; verify by building before reacting.

## Open questions (→ the contract-freeze design session, next)

1. `HttpError` enum vs core `ErrorData` shape (above).
2. The streaming body core seam: how a chunk re-enters the core as a typed input,
   back-pressure, end-of-body signal (M2 deliberately left this; row 16's mechanism is
   decided, its contract surface is not — `BodyOutcome` still carries the named seam).
3. The JNI edition of the stall question (spike-plan §4 N2) — S-AN's first item, not
   answered by the host probe.
4. `PermissionDenied`'s positive control lands in the platform suites (steps 25/26).
5. C3 covers capability traits only; proxy/env-var divergence (row 25) is doc-recorded —
   decide whether C3 grows a CFG column or the docs stay the record.
6. `SkipReason` is currently unused harness API (kept for platform-only rows); remove later
   if it stays unused.
7. The `MaybeSend` "compile-checked both ways" claim awaits any wasm target.

## Next

Contract-freeze design session (both gates cleared, agenda above), then step 25 (S-AP,
Apple adapter) and step 26 (S-AN, Android — opens with the JNI stream probe). S-WIN.W1 fits
any open VM session; W2 still rides the parked step-23 pin (upstream finding 07).
