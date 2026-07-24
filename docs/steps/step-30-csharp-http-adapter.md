# Step 30 — bolted-http V: the C# adapter (S-WIN part II, W1+W2)

**Phase 4 · status: ready · branches: `step-30/mX` off `origin/main`, PR per milestone.**
Spike-plan §5 (S-WIN), the two clusters that don't need Windows: **W1** (the .NET syntheses,
probed) and **W2** (the full conformance suite through the FFI). The gating tier is headless
`dotnet test` **on this Mac** (the step-14 decision: the seam is host-portable; the WinUI face
waits for Windows hardware). Everything the spike plan listed as blockers is gone: both C#
FFI bugs are fixed in released boltffi 0.28.0 and re-verified by execution (step 29), and the
contract is no longer a proposal but the **ruled, implemented** step-27 seam. The
`SkipReason` keep-or-delete ruling rides this step (freeze agenda: *"keep until S-WIN lands
(C# may need it); delete if still unused then"*) — the exit renders that verdict.

Read first: `crates/bolted-http/docs/feature-matrix.md` (§7 rules 1–14, the §5 row classes —
**§5 row numbers ≠ §7 rule numbers**, don't conflate), `crates/bolted-http/docs/spike-plan.md`
§5, `crates/bolted-http/docs/research/2026-07-18-windows.md` (the C1–C12 capability probes —
the syntheses inventory below cites it), `crates/bolted-http-ffi/src/lib.rs` (the one bridge
crate: `HttpAdapter` callback trait, `HttpHarness` exports, the parked-`ChunkSink` registry,
`live_streams()`), `apple/bolted-http-conformance/Tests/BoltedHttpConformanceTests/ConformanceTests.swift`
and `android/bolted-http-conformance/` (the two proven driver shapes — mirror, don't
rediscover), `docs/steps/step-27-report.md` (what the ruled contract shipped),
`docs/steps/step-29-report.md` §Open questions (the C#-tier facts that carry over), and the
`pack:csharp`/`test:csharp` mise tasks (the proven .NET packaging + TRX recipe).

## Decisions already taken (do not re-litigate)

- **The foreground API is `System.Net.Http.HttpClient` over `SocketsHttpHandler` — not
  WinRT.** Settled by the 2026-07-18 Windows research (§B: WinRT `Windows.Web.Http` is
  frozen; `BackgroundTransfer` is the separate background family = W3, out of scope).
  `architecture.md`'s diagram line still saying "WinRT HttpClient" is stale — fix it here.
- **The rows live in Rust; the platform drives, it never re-authors.** The C# driver calls
  the generated `HttpHarness` methods (`RunC1`/`RunExtraRows`/`RunC2`/`RunStreamRows`/
  `RunC3`) exactly as Swift/Kotlin do. The TestServer runs **in-process** — `StartServer()`
  returns `ServerInfo` with the three base URLs, the trust-anchor DER and both SPKI pins.
- **One bridge crate.** `bolted-http-ffi` is multi-target since step 27 M0; its
  `boltffi.toml` already carries `[targets.csharp] enabled = false`. This step flips it —
  no new bridge crate, no new Rust surface unless a gap is proven and recorded.
- **The ruled contract is law** (step 27): streaming = core-owned ring + seq/completeness
  gates + terminal-exactly-once, chunks re-enter synchronously via `DeliverChunk`/
  `FinishBody` (the parked-registry shape — no `ffi_stream` in the contract path); the one
  mid-flight signal is the pushed `FfiFlowSignal` (`Pause`/`Resume`/`Cancel`) — **no
  poll-watchers, ever**; `live_streams()` is row 14's hygiene observable.
- **Redirects: .NET follows manually** (`AllowAutoRedirect = false` + a follow loop — the
  only way .NET exposes hops). Consequence: unlike Apple/Android, the C# adapter counts
  hops itself, so ceiling exhaustion is classified structurally by its own loop (was the
  last hop a 3xx at the cap?) and the trace is recorded in traversal order (the step-26
  hop-*order* blind spot has a watcher — expect it to bite if the loop appends wrong).
- **Errors**: the adapter maps, it never invents keys. Timeout vs cancel are both
  `TaskCanceledException` on .NET — **classify by token, never by exception type** (rule 2).
- **Tier facts carried from step 29**: dotnet SDK `10.0.301` pinned per-task (never in
  `[tools]`); TRX is the source of truth for counts (`dotnet test` does propagate exit
  codes, but counts are quoted from the TRX); NuGet-cache eviction before test (the packed
  version is fixed at 0.1.0 across re-packs — evict `bolted_http_ffi` like `gen_profile_ffi`);
  the 0.28.0 IR backend names the binding namespace after the raw crate — expect
  `Bolted_http_ffi`, verify in M0, don't fight it.
- **Packaging convention** (step-25/26, C# edition): the pack output (`.nupkg`) is the
  consumable; the hand-written adapter and the driver live beside it —
  `csharp/bolted-http/` (classlib: `BoltedHttp.cs`, references the nupkg) +
  `csharp/bolted-http-conformance/` (NUnit driver project). If nupkg consumption forces a
  collapse into one project, that's a recorded deviation, not a redesign.

## Scope

Flip the target, write the adapter, drive the suite. The spike-plan clusters, updated:

- **W1 — the .NET syntheses, probed first** (spike-plan §5, amended: "no FFI needed" is
  moot — the cheapest server is `HttpHarness.StartServer()` with raw `HttpClient` pointed
  at `ServerInfo`'s URLs; the adapter is not yet in the loop). The inventory, each a
  focused NUnit probe with its failure mode demonstrated where the docs predict one:
  1. The **streamed-read timeout hole**: with `ResponseHeadersRead`, `HttpClient.Timeout`
     stops applying — prove it, then prove the re-armed `CancelAfter`-per-read synthesis
     closes it (rule 3's mechanics).
  2. **Timeout-vs-cancel disambiguation by token** (rule 2's mechanics): both throw
     `TaskCanceledException`; classify by which `CancellationToken` fired.
  3. **Manual-follow redirect loop**: hop trace in traversal order + verify modern .NET
     natively refuses https→http (rule 4 — expected native, verify, don't assume).
  4. **Decompression**: default is `None` — prove, then `DecompressionMethods.All` +
     rule 7's honesty (decoded bytes identical, `content_length` None-or-honest).
  5. **Pinning via `SslOptions.RemoteCertificateValidationCallback`**: install
     `ServerInfo.good_cert_der` as the trust anchor **in the callback** (cert trust is
     OS-delegated; the macOS trust store doesn't know the test CA), split trust-failure ⇒
     `Tls` from pin-mismatch ⇒ `PinMismatch` exactly as Linux/Apple/Android do.
  6. **Upload progress**: the naïve `HttpContent` wrapper buffers and jumps to 100% —
     prove it, then the flush-aware non-buffering wrapper (rule 11 + Q8's terminal total).
  7. **Cache-less/cookie-less defaults**: verify `.NET both-off` (the feature-matrix's
     "verify when the C# leg unparks" flags) — this is what makes rule 5's 304 *real*.
- **W2 — the full suite through the FFI**: 11 C1 rules + 4 extra rows + 10 C2 keys +
  rows 12/13 (streaming) + row 14 (subscription hygiene via `LiveStreams()`) + the C3
  column. Every row watched red first (broken-adapter twins, mirroring
  `BrokenHttp`/`AlwaysOkHttp` and the Apple/Android stream breaks).
- **Streaming mechanics** (.NET has no task-suspend — the Linux/OkHttp mechanics,
  mirrored): `ResponseHeadersRead`, the response stream read in the adapter's own loop,
  one `DeliverChunk` per read; `Pause`/`Resume` pace the loop with a lost-wake-safe wait;
  `Cancel` through the CTS. Terminal exactly once via `FinishBody`.
- **Total deadline**: .NET synthesizes (timer + linked CTS — `HttpClient.Timeout` is
  client-wide and doesn't survive `ResponseHeadersRead`); the `/drip` trickle row
  (`row-deadline-total-not-per-idle`) is the watcher that it's *total*, not per-idle.
- **C3 metrics column**: time-boxed probe of .NET's per-request phase surface
  (`HttpTelemetry`/Meters). If phase tiers aren't cheaply per-request, the column records
  `metrics | absent` — honest beats built. Don't build an OTel listener in this step.
- **`PermissionDenied`**: the step-25 treatment — positive control if reachable on the
  macOS dotnet tier, otherwise recorded platform-gated with evidence.
- **The `SkipReason` verdict** (ruled condition, this step evaluates it): if no C# row
  skips — and none should, since the adapter implements streaming — the variant's only
  remaining users are the non-streaming-factory guard in `stream.rs` and its test. Delete
  it **only if deletion stays mechanical** (rows 12/13 need a defensible non-skip shape
  for a streaming-less factory); if deletion turns structural, keep it, record why, and
  the verdict is "keep, with the reason". Either way the report states the verdict.
- **Doc updates**: feature-matrix Windows/.NET statuses (probed → verified per row), the
  stale `architecture.md` WinRT line, `conformance-mutation-table.md` extended.

## Milestones (one Opus sub-agent each; Fable reviews between; PR per milestone)

- **M0 — packaging + the bridge gate + the W1 probes.** Flip `[targets.csharp]`;
  `pack:csharp:http` + `test:csharp:http` mise verbs (per-task dotnet, TRX logger, NuGet
  eviction, fail-on-red verified); the project pair; walking-skeleton adapter (exactly one
  C1 row). **Gate 1:** one row green from `dotnet test` AND a deliberately-broken adapter
  showing the same row red with a legible message. **Gate 2:** a minimal
  `DeliverChunk`/`FinishBody` re-entry smoke (chunks through the C# callback boundary,
  ordered and complete — kill criterion 3's early look). **Gate 3:** the W1 probe suite
  green with each documented .NET failure mode demonstrated (the probes are keepers — they
  pin the syntheses the adapter is about to encode). Kill criteria 2/3 evaluated here.
- **M1 — the adapter, core rows.** Full `BoltedHttp.cs`: dispatch, one-shot completion,
  the 12-key C2 mapping, total-deadline synthesis, cancel (rule 9 — no
  `TaskCanceledException` leaking as a transport key), memory sink, upload progress,
  version observable (negotiated, not requested — the step-25 blind spot has a watcher),
  manual-follow loop with trace + ceiling. Target: C1 rules + extra rows green except the
  M2 syntheses. Every row watched red first.
- **M2 — syntheses + streaming.** File sink (row 15), pinning + the Tls/PinMismatch
  split, rule-4 verify, rule-5 real-304, gzip honesty, `PermissionDenied` treatment,
  rows 12/13 + row 14 with red twins (drop-first-chunk and skip-terminal breaks, the
  Apple/Android pattern), C3 column pinned. Full suite green.
- **M3 — the mutation pass.** Extend `conformance-mutation-table.md` with the C# adapter:
  mutations across the syntheses (per-read timer, token classification, pin callback,
  progress wrapper, follow-loop count/order, sink routing) + the bridge token routing;
  two-hypotheses discipline on every survivor; genuine blind spots get suite rows watched
  red on all implementors.
- **M4 — the verdicts + the docs + the report.** The `SkipReason` verdict executed or
  recorded; feature-matrix/architecture doc updates; `step-30-report.md` (built /
  deviations / friction / open questions — the W3 background family's open items named);
  ROADMAP row.

## Kill criteria (real; if hit, stop and report)

1. A C1 rule that cannot pass on `SocketsHttpHandler` **without contract change** → stop,
   report the rule; the row gets redesigned in a design session, not the adapter bent
   (spike-plan criterion, verbatim).
2. The second C# package inexpressible in BoltFFI's model (the nupkg + hand-written
   adapter + driver triangle doesn't assemble) → stop; that's a design session, not a
   workaround.
3. Chunk re-entry through the C# callback boundary stalls, reorders, or loses chunks →
   stop the streaming cluster, record shape + numbers (the S-FFI fallback rule,
   per-platform), continue the rest; the report says so honestly.

## Non-goals (→ elsewhere)

W3: `BackgroundTransfer`, package identity, the 200-op limit, reattach — needs Windows
hardware and the background family's contract is §9-open. WinUI anything. WinRT anything.
The harness-hardening track (tier-provided sink path, row hard-kill, ALPN TestServer).
Cookie capability. h3/MsQuic. Proxy behavior beyond C3 documentation. Upstream filings
(Henrik's, always). Glossary changes (propose-and-ask only).

## Inherited cautions

- Build/test only via `mise run …`; run tiers **synchronously in the foreground**, tee to
  the scratchpad, read the log on timeout — never relaunch a tier because it seems stuck.
- **TRX is the truth for counts**; quote it, never the wrapper output. The task must fail
  on test failure — verify the exit path once in M0 (planted red).
- Never edit anything under `dist/`; pack owns it. The generated C# bindings are pack
  output — a bug there is upstream evidence, not something to patch.
- Watch generated-name collisions (the Swift `FfiError` lesson): if a `#[data]` name
  collides in the C# emission, rename ours and record the lint candidate.
- C#: no constraint literals (deadlines/limits come from the effect); `Nullable=enable`;
  the adapter maps errors, it never invents keys.
- Rust: edition 2024, clippy `-D warnings`, no `unwrap`/`expect`/`panic!` in library code;
  `bolted-http` stays dependency-free on its default build; `check` stays dotnet-free.
- Stale IDE diagnostics are not the committed state — verify by building (the gates, not
  the squiggles, count).
- If a decision is missing here: smallest reversible choice, record it in the report. If
  structural (traits, invariants, ARCHITECTURE, contract surface): stop, record, leave it
  for a design session. Never resolve ARCHITECTURE §9 questions ad hoc.

## Exit checklist

- [ ] `mise run check` green (host, dotnet-free) · `test:csharp` still 53/53 ·
      `test:csharp:http` green **by TRX**, fail-path verified once.
- [ ] All 11 C1 rules + 4 extra rows + 10 C2 keys + rows 12/13/14 green on
      `BoltedHttp.cs`, each watched red first; C3 column pinned; `PermissionDenied`
      treatment recorded with evidence.
- [ ] W1 probe suite green, each documented .NET failure mode demonstrated (streamed-read
      hole, token disambiguation, naïve-wrapper jump, decompression default, cache/cookie
      defaults) — kept as regression tests.
- [ ] `SkipReason` verdict rendered and executed-or-recorded.
- [ ] Mutation table extended; survivors discharged with the two-hypotheses rule.
- [ ] Feature-matrix .NET statuses updated; the stale architecture.md WinRT line fixed;
      `step-30-report.md` + ROADMAP row done.
