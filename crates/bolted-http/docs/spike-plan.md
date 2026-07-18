# bolted-http — the verification spike plan

**Status:** proposal, 2026-07-18. The verification basis for the contract proposed in
[feature-matrix.md](feature-matrix.md). This is *not* a scheduled step — D38's scheduling rule
stands (the crate ships no feature until one needs HTTP); when a feature does, this plan is the
raw material for the step docs. **Revised the same day:** web left the platform set
(feature-matrix §9), so the S-WEB cluster is gone; the plan covers the four adapter surfaces.
Prior evidence it builds on: the packaging spike
([spike-packaging-report.md](spike-packaging-report.md) — packaging, capability round-trip,
overhead, error taxonomy all passed on Apple) and both step-02 probes (callback traits cheap
and reentrancy-safe; stream push modes stalled at 0.27.3, machinery converges at 0.27.5).

## 0. Shape of the spike

One shared artifact plus per-platform probe clusters:

- **The conformance harness** (S-CONF) is the deliverable that outlives the spike — the
  per-adapter suite of prior-art lesson 2, seeded with feature-matrix §7's eleven fixed rules.
  Everything platform-specific below is expressed as harness rows wherever possible, so the
  spike's output is not a report but a *runnable suite* each future adapter must pass. The
  CORE(adapter) rows (feature-matrix §4) get particular weight: they are adapter code, not
  platform behavior, and only the suite makes them true.
- A local test server the harness owns (echo, delay, stall-mid-body, redirect chains incl.
  https→http, 304/ETag, gzip/brotli, 401, TLS-failure endpoints, pin-mismatch cert) — one
  server, all platforms point at it. The C2 error-taxonomy probe already did this by hand for
  three URLSession failures; S-CONF systematizes it.
- Per-platform clusters run in dependency order (§7); each has kill criteria in the project's
  usual sense — hit one, stop and report, don't work around.

## 1. S-CONF — the conformance harness (host, Rust)

Build the suite skeleton against a mock adapter first (the suite must fail correctly before it
can pass correctly — the forbidding-test lesson):

- C1: the eleven fixed rules from feature-matrix §7 as parameterized rows.
- C2: the error-taxonomy matrix — every typed error key reachable via the test server; a
  positive control per key (a needle that can never match is green forever).
- C3: the divergence matrix **generated from the capability types** (rung 3): the harness
  emits, per adapter, the table of capabilities present/absent — hand-written prose matrices
  are the prior-art failure mode. Smaller now (metrics tiers, background availability) since
  the CORE(adapter) upgrades moved three former capabilities into the core.

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
- A3: conformance rows C1/C2 on the real adapter — especially rule 3 (stalled body vs the
  **synthesized** per-request deadline; URLSession's idle timer must not mask it), rule 5
  (ephemeral session ⇒ real 304 for manual `If-None-Match`), and rule 4 (the delegate's
  https→http refusal — row 6's synthesis).
- A4: the two remaining Apple syntheses: pinning in the trust-evaluation delegate (rule 10)
  and the redirect hop trace via `willPerformHTTPRedirection` (row 7).
- A5: priority hint acceptance, *if the row survives Henrik's review*: set `task.priority`
  from the effect's hint; assert acceptance only (the RFC 9218 wire observation is FLAGGED
  lore — do *not* conformance-test the wire).
- A6: regression guard: run the whole cluster with `usesClassicLoadingMode = false` — Apple
  says the default will flip; find out now whether the adapter cares.
- A7 (deferred with the background family): minimal background-session + relaunch rehydration
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
  `BridgeInterceptor`'s header stripping; upload-progress sink wrapping (rule 11 — row 14's
  OkHttp synthesis).
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
  the S-CONF test server — this cluster grew, because .NET carries the most syntheses:
  the streamed-read timeout synthesis (re-armed `CancelAfter` per read; rule 3),
  timeout-vs-cancel disambiguation by token (both are `TaskCanceledException`; rule 2),
  the manual-follow redirect loop that synthesizes the hop trace (row 7),
  `DecompressionMethods.All` + rule 7, redirect rule 4 (verify modern .NET refuses
  https→http natively), pinning via `SslOptions.RemoteCertificateValidationCallback`
  (rule 10), and upload-progress content-stream wrapping (rule 11 — the naïve wrapper jumps
  to 100%; prove the flush-aware one doesn't). This de-risks the adapter design without
  touching boltffi.
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
  the contract's pin data; pin-mismatch ⇒ typed error. This gates matrix row 19's
  CORE(adapter) status — it is the one place the Linux adapter has real work no crate does
  for it.
- L3: retry-off verification: confirm the adapter config (no `retry()`, connection-level
  recovery only) satisfies rule 8's positive control.
- L4: document-don't-promise: proxy env-vars-only behavior recorded into the divergence matrix
  output (C3), not worked around.

Kill criterion: L2 infeasible (rustls verifier API can't express SPKI pinning cleanly) →
matrix row 19 demotes back to CAP with Linux absent; report, don't hack.

## 7. Ordering and effort

```
S-CONF ──► S-FFI ──► S-AP (host)          host-only, first
   │                    └─► S-AP device    when convenient
   ├──────► S-LX                           host, cheap, reference adapter
   ├──────► S-AN                           Pixel 8a, after S-FFI verdict
   └──────► S-WIN.W1                       VM, anytime (no FFI)
            S-WIN.W2+                      blocked upstream
```

S-CONF + S-FFI + S-LX is one working session's shape; S-AP a second; S-AN a third (device
tier); S-WIN.W1 fits wherever a VM session is open. Suggested step granularity when
scheduled: one step for S-CONF+S-FFI+S-LX (the harness exists, one real adapter passes, the
streaming and pinning-on-Linux verdicts are in), one for S-AP, one for S-AN, S-WIN riding the
upstream fix.

## 8. Out of scope, recorded

- The background family end-to-end (A7/W3 deferred; the family's contract is §9-open). The
  Linux detached-helper mechanics (feature-matrix §6.5 — detachment method, logout survival,
  Flatpak Background portal) join that deferred cluster as the family's L-probe.
- WebSocket anything (parked family).
- Cookie capability probes (shape undesigned; probing before design is backwards).
- Web adapter probes — web is out of the platform set (feature-matrix §9); if it ever joins,
  its cluster is drafted from the 2026-07-18 web sweep (real browsers, all three engines,
  never a green wasm build — the standing rule).
- Perf tuning beyond the measurements named (S-FFI latency) — envelopes, not optimization.
