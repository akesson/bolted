# bolted-http — the five-platform verification spike plan

**Status:** proposal, 2026-07-18. The verification basis for the contract proposed in
[feature-matrix.md](feature-matrix.md). This is *not* a scheduled step — D38's scheduling rule
stands (the crate ships no feature until one needs HTTP); when a feature does, this plan is the
raw material for the step docs. Prior evidence it builds on: the packaging spike
([spike-packaging-report.md](spike-packaging-report.md) — packaging, capability round-trip,
overhead, error taxonomy all passed on Apple) and both step-02 probes (callback traits cheap
and reentrancy-safe; stream push modes stalled at 0.27.3, machinery converges at 0.27.5).

## 0. Shape of the spike

One shared artifact plus per-platform probe clusters:

- **The conformance harness** (S-CONF) is the deliverable that outlives the spike — the
  per-adapter suite of prior-art lesson 2, seeded with feature-matrix §7's ten fixed rules.
  Everything platform-specific below is expressed as harness rows wherever possible, so the
  spike's output is not a report but a *runnable suite* each future adapter must pass.
- A local test server the harness owns (echo, delay, stall-mid-body, redirect chains incl.
  https→http, 304/ETag, gzip/brotli, 401, TLS-failure endpoints, pin-mismatch cert) — one
  server, all platforms point at it. The C2 error-taxonomy probe already did this by hand for
  three URLSession failures; S-CONF systematizes it.
- Per-platform clusters run in dependency order (§8); each has kill criteria in the project's
  usual sense — hit one, stop and report, don't work around.

## 1. S-CONF — the conformance harness (host, Rust)

Build the suite skeleton against a mock adapter first (the suite must fail correctly before it
can pass correctly — the forbidding-test lesson):

- C1: the ten fixed rules from feature-matrix §7 as parameterized rows.
- C2: the error-taxonomy matrix — every typed error key reachable via the test server; a
  positive control per key (a needle that can never match is green forever).
- C3: the divergence matrix **generated from the capability types** (rung 3): the harness
  emits, per adapter, the table of capabilities present/absent — hand-written prose matrices
  are the prior-art failure mode.

Kill criterion: none — this cluster cannot fail, only be incomplete.

## 2. S-FFI — the streaming mechanism decision (host, boltffi ≥0.27.5)

The one probe gating a contract row (feature-matrix §5.11). Re-run the step-02 stream shapes
*inside the http round-trip* on 0.27.5+:

- F1: 100-chunk response body via `ffi_stream` push mode, live consumer — the exact shape that
  stalled at 15/100 on 0.27.3. Measure delivery completeness and latency.
- F2: the same body via callback-trait push (the capability machinery already proven at 8 ns).
- F3: the same via wake-and-read batch pull (`snapshot()` getter).
- Decision output: mechanism choice on measurements, or —

**Kill criterion:** if all push shapes still stall at 0.27.5 in the http context, response
streaming (matrix row 16) drops to `Memory | File` sinks; record it, don't engineer around it.
(The fallback is explicitly acceptable — §5.11 notes it parks SSE with WebSocket.)

## 3. S-AP — Apple (macOS host, then iOS device tier)

The packaging/round-trip/error clusters already passed; what remains:

- A1: streamed response through the S-FFI-chosen mechanism, URLSession `bytes(for:)` →
  chunks across the boundary → core input. (The old plan's blocked half, unblocked.)
- A2: download-to-file row: `downloadTask` + synchronous move inside the delegate callback →
  `FileRef` completion. Verify the temp-file-lifetime rule under the adapter's threading.
- A3: conformance rows C1/C2 on the real adapter — especially rule 3 (stalled body vs
  deadline; URLSession's idle timer must not mask the total deadline) and rule 5 (ephemeral
  session ⇒ real 304 for manual `If-None-Match`).
- A4: priority hint acceptance: set `task.priority` from the effect's hint; assert acceptance
  only (the RFC 9218 wire observation is FLAGGED lore — do *not* conformance-test the wire).
- A5: regression guard: run the whole cluster with `usesClassicLoadingMode = false` — Apple
  says the default will flip; find out now whether the adapter cares.
- A6 (deferred with the background family): minimal background-session + relaunch rehydration
  probe on the real device — stays out of this spike per D38 scheduling.

Kill criterion: a C1 rule that cannot pass on URLSession without contract change → stop,
report the rule, redesign the row (not the adapter).

## 4. S-AN — Android (Pixel 8a, device tier authorized)

The Kotlin analog of the proven Apple packaging story plus the platform's open questions:

- N1: `boltffi pack` bundled-layout equivalent for a Kotlin/AAR package — hand-written
  `BoltedHttp.kt` next to generated bindings, one consumable artifact. (Step-05's analog
  probe, scoped to http.)
- N2: capability round-trip on OkHttp with the conformance rows; JNI edition of the S-FFI
  stream check (the step-02 report's explicit forward pointer: does the stall reproduce or
  change shape on JNI?).
- N3: pinning, both controls: (a) adapter `CertificatePinner` pins → mismatch cert ⇒ typed
  error; (b) the fragility check — install a custom `TrustManager` and prove NSC `<pin-set>`
  stops enforcing (the suite must never accidentally depend on NSC), plus the hostname-less
  trust-check landmine (per-domain NSC + 2-arg `checkServerTrusted` throws).
- N4: transparent-gzip normalization: `content_length` honesty (rule 7) given
  `BridgeInterceptor`'s header stripping.
- N5: `HttpEngine` feature-detection on the device (API 34+): present? h3 negotiated against
  a test endpoint? Same conformance rows through the engine — this decides whether the
  adapter's engine matrix (OkHttp / HttpEngine) is spike-real or paper.
- N6: cancellation semantics: `Call.cancel()` from a non-call thread ⇒ `Cancelled` completion
  (rule 9), no `IOException("Canceled")` leaking as a network-error key.

Kill criteria: N1 packaging inexpressible in BoltFFI's model → design session before any
Kotlin adapter work (same criterion the Apple spike had). N2 stream stall on JNI → same
fallback as S-FFI, recorded per-platform.

## 5. S-WIN — Windows (blocked; scope accordingly)

The C# check driver is still broken upstream (MarshalAs(I1) on FfiBuf returns — killed
step 14, unresolved at 0.27.5). Until that clears, S-WIN is **paper-scoped**:

- W1 (now, no FFI needed): a standalone .NET console probe of the adapter's hard parts against
  the S-CONF test server — the streamed-read timeout synthesis (re-armed `CancelAfter` per
  read; rule 3), `DecompressionMethods.All` + rule 7, redirect rule 4 (verify modern .NET
  refuses https→http), pinning via `SslOptions.RemoteCertificateValidationCallback`.
  This de-risks the adapter design without touching boltffi.
- W2 (after the upstream fix): full conformance rows through the FFI round-trip.
- W3 (with the background family, later): BackgroundTransfer behind package identity — sparse
  package on the Windows VM, 200-op limit behavior, reattach ceremony, pause/resume.

Kill criterion: none new — the operative one is the standing upstream dependency; W1 has no
kill shape, it's reconnaissance.

## 6. S-LX — Linux (host or VM)

- L1: reqwest adapter through the same conformance rows (this is nearly free — the harness and
  adapter share a language; it's also the reference adapter the suite is debugged against,
  *after* the mock — remember the one-implementor lesson: mock first, then reqwest, then
  mutate both).
- L2: SPKI pinning feasibility: `tls_backend_preconfigured` + custom rustls verifier carrying
  the contract's pin data; pin-mismatch ⇒ typed error. This is the one place the Linux adapter
  has real work no crate does for it.
- L3: retry-off verification: confirm the adapter config (no `retry()`, connection-level
  recovery only) satisfies rule 8's positive control.
- L4: document-don't-promise: proxy env-vars-only behavior recorded into the divergence matrix
  output (C3), not worked around.

Kill criterion: L2 infeasible (rustls verifier API can't express SPKI pinning cleanly) →
pinning capability drops Linux from its adapter list; report, don't hack.

## 7. S-WEB — Web (wasm, real browsers)

Per the standing rule: verified in real browsers (all three engines), not by a green wasm
build.

- B1: fetch adapter (web-sys) through the conformance rows in Chromium + Firefox + WebKit —
  especially rule 2 (timeout-vs-cancel via `signal.reason`; the WebKit `AbortError` quirk is
  the reason this rule exists), rule 5 (`no-store` + `If-None-Match` ⇒ raw 304 to wasm), and
  rule 6 (reserved-header drop behavior surfaced as types, not silence).
- B2: download-to-file row on OPFS: `pipeTo(createWritable())` from wasm in all three engines
  (the Baseline claim is fresh — Safari 26.0; verify, don't trust), behind the `FileRef`
  abstraction.
- B3: streamed response consumption (ReadableStream → wasm chunks): measure the
  per-chunk JS→wasm copy cost at realistic chunk sizes — this is the web-side analog of the
  S-FFI measurement, and the number the contract's streaming row should quote.
- B4: capability typing check: upload progress, pinning, metrics tiers — assert they are
  *compile-time absent* on the wasm adapter (the reqwest-wasm anti-pattern is silent
  presence).
- B5: `Send`-bound check: the contract traits compile on wasm32 with the conditional-bound
  pattern (futures are `!Send` on wasm — this bites the trait design, find out in the spike,
  not in the framework).

Kill criteria: B2 fails on any engine → row 15 demotes to CAP (native + Chromium), contract
adjusted. B3 cost is pathological (≫ the 30 µs/MB FFI figure) → streaming row notes a web
perf envelope.

## 8. Ordering and effort

```
S-CONF ──► S-FFI ──► S-AP (host)          host-only, first
   │                    └─► S-AP device    when convenient
   ├──────► S-LX                           host, cheap, reference adapter
   ├──────► S-WEB                          browsers on host
   ├──────► S-AN                           Pixel 8a, after S-FFI verdict
   └──────► S-WIN.W1                       VM, anytime (no FFI)
            S-WIN.W2+                      blocked upstream
```

S-CONF + S-FFI + S-LX is one working session's shape; S-AP/S-WEB a second; S-AN a third
(device tier); S-WIN.W1 fits wherever a VM session is open. Suggested step granularity when
scheduled: one step for S-CONF+S-FFI+S-LX (the harness exists, one real adapter passes, the
streaming verdict is in), one for S-AP+S-WEB, one for S-AN, S-WIN riding the upstream fix.

## 9. Out of scope, recorded

- The background family end-to-end (A6/W3 deferred; the family's contract is §9-open).
- WebSocket anything (parked family).
- Cookie capability probes (shape undesigned; probing before design is backwards).
- Perf tuning beyond the measurements named (B3, S-FFI latency) — envelopes, not optimization.
