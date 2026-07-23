# bolted-http — the homogenized surface: feature matrix and contract proposal

**Status:** design study, 2026-07-18 — the second investigation round, and a **proposal**: nothing
here is frozen (the D38 shape is decided; the contract itself stays §9-open until a feature
needs HTTP). **2026-07-21:** the contract-review session ruled on every open contract
question after all three reachable adapters shipped (steps 24–26) — rulings are annotated
inline per row/section ("Ruled 2026-07-21"); the decision record is
[`docs/design/contract-freeze-agenda.md`](../../../docs/design/contract-freeze-agenda.md).
These are working decisions (unreleased, own-use software), expected to evolve as we learn.
Builds on [platform-surfaces.md](platform-surfaces.md) and
[prior-art.md](prior-art.md) (both 2026-07-09); where this doc conflicts with them, this doc
wins (§1 lists the corrections). **Revised the same day:** web is **out of the platform set**
(Henrik: it was never part of the asked surface — win/lin/mac/android/ios). The matrix now
covers the five platforms / four adapter surfaces (Apple, Android, Windows/.NET, Linux); §9
records how web would fit if it ever joins. Same revision added the **CORE(adapter)** class
(§3) and reclassified every row that web had been holding down.

**Method:** five parallel research sweeps (Apple, Android, Windows, Linux/Rust, Web), each
resolving the §8 verification flags of the earlier study and inventorying the feature
dimensions it skipped (WebSockets, SSE, auth, compression, priorities, progress, pause/resume,
multipart, conditional requests, and 2024–2026 platform additions). Evidence classes: official
docs, SDK headers and library source (AOSP/Conscrypt/Chromium/OkHttp/hyper-util read directly),
and **live probes** on this macOS 26 host. Claims resting on third-party material or
absence-of-documentation are marked **FLAGGED**, as before. The web sweep's raw evidence is
kept at [research/2026-07-18-web.md](research/2026-07-18-web.md); its findings now inform only
§9.

---

## 1. What changed since the 2026-07-09 studies

Corrections and floor movements the earlier docs should be read with:

1. **The "iOS ≤ 4 h" background cap is lore.** No Apple doc states any wall-clock cap; the only
   documented bound is `timeoutIntervalForResource` (7 days), which *does* apply to background
   sessions. prior-art §5.3 quotes `background_downloader`'s "iOS ≤4h" envelope — keep it
   attributed to that package, not to the platform.
2. **Android's `<pin-set>` question is answered** (§5.14): binds OkHttp by architecture (the
   enforcement lives in Conscrypt's handshake via the platform trust manager, SOURCE-VERIFIED),
   dies silently under a custom `TrustManager`, undocumented for Cronet (which delegates chain
   verification to the hostname-aware platform path but officially points at its own
   `addPublicKeyPins`). NSC explicitly does not cover websockets.
3. **Android gained two stack-level facts the study missed**: `android.net.http.HttpEngine`
   (API 34+, Cronet bundled in the Connectivity Mainline module — HTTP/3 + Brotli + pinning at
   zero APK cost, no Play Services; "the recommended default network stack on Android from
   API 34") and **User-Initiated Data Transfer jobs** (API 34+, JobScheduler-only, quota-exempt,
   mandatory notification with a user Stop button, **schedulable only while the app is
   visible**). Embedded Cronet is dead (~19–20 MB/ABI, FLAGGED community figure; the Maven
   embedded artifact is abandoned).
4. **Windows' foreground stack is settled by Microsoft's own cross-references**: .NET
   `HttpClient`/SocketsHttpHandler for foreground, `Windows.Networking.BackgroundTransfer` for
   large/background (package identity required — but "packaging with external location"
   (sparse packages) grants identity without full MSIX, so *unpackaged ⇒ no background
   transfer* is not a hard equivalence). `Windows.Web.Http` is alive but frozen at h2 with
   2017-era docs. Separately: platform-surfaces §4's WinINet framing is wrong — Microsoft's
   current comparison page recommends WinINet *over* WinHTTP for non-service client code
   (irrelevant to our adapter, but the study should not call it legacy).
5. **§7.9's "timing metrics implementable on every native surface" was too generous**: reqwest
   exposes **no per-phase timing at all** (no DNS/connect/TLS durations; the only seam is a
   tower `connector_layer` you time yourself). The Linux adapter's honest metrics tier is
   coarse. Meanwhile .NET 8+ ships rich built-in meters + OTel spans down to TLS handshake —
   but only on the .NET side; WinRT/BackgroundTransfer expose nothing.
6. **Small but load-bearing**: .NET decompression is **off** by default
   (`DecompressionMethods.None`); OkHttp's transparent gzip **strips `Content-Length`** from
   the response you see; Apple's real shared `URLCache` is ~512 KB memory at runtime (the
   header's "4 MB" is stale lore) — every adapter must set cache/decompression behavior
   explicitly rather than inherit defaults.
7. **Dropping web from the platform set raised the floor** (the same-day revision): redirect
   hop traces, upload progress, and SPKI pinning stop being optional capabilities — every
   remaining surface can honor them, natively or via adapter code (§3's CORE(adapter));
   the negotiated-version observable stops being `Option`; and the `FileRef` indirection is
   no longer forced by OPFS (a file is a path on all four surfaces). Details per row in §5;
   what web would take back, in §9.

## 2. The effect families

The investigation confirms the two-family shape and parks a third:

- **`HttpRequest`** — the foreground request effect: bounded, cancellable, completion re-enters
  the core as one typed input. §4/§5 define its homogenized surface.
- **`BackgroundTransfer`** — a separate effect family (decided in D38; still §9-open in full):
  durable, serializable, file-based descriptors with stable identities, handed over entirely,
  completion delivered to a possibly-new core instance. New evidence sharpens it (§6).
- **Realtime (WebSocket / SSE)** — *parked, deliberately*. SSE needs no family of its own: it
  is a streamed response body plus an app-side parser on every platform (reconnection and
  Last-Event-ID are caller-owned everywhere — even OkHttp's own SSE module is officially
  "experimental" in stable 5.x). WebSocket would be a genuine third family with its own honest
  contract (no delivery acknowledgment anywhere: Apple queues without flow control, OkHttp
  closes the socket at a hard 16 MiB queue overflow; compression is uncontrollable on Apple;
  Cronet has none; background sessions forbid it). Record it as a protected possibility;
  design it only when a feature needs it.

## 3. Classification vocabulary

Every dimension lands in exactly one bucket (prior-art lesson 1 — the intersection in the
contract, the rest in types):

- **CORE** — portable, every adapter honors it identically; conformance-tested.
- **CORE(adapter)** — portable and conformance-tested identically, **but on the named surfaces
  the native stack lacks the primitive and the shipped adapter synthesizes it in custom
  code**. Same contract strength as CORE; the difference is where the behavior lives — and
  that the conformance suite carries the burden of proving each synthesis honest (it is
  adapter code, not platform behavior, that the rule pins).
- **CAP** — typed optional capability: an adapter that cannot honor it does not compile against
  it (or reports it absent at runtime where availability is a runtime fact — Play-Services
  Cronet, package identity); never a silent no-op.
- **CFG** — adapter/composition-root configuration; the core never sees it.
- **OUT** — excluded: no honest portable semantics exist, or excluded by design.

## 4. The matrix

| # | Dimension | Class | The one-line reason |
|---|---|---|---|
| 1 | Method, URL, typed headers | CORE | Everywhere; reserved headers are adapter-owned (§5.1) |
| 2 | Body: `Bytes` \| `File` \| `Multipart` | CORE(adapter) | File bodies stream from disk everywhere; multipart hand-built on Apple/.NET (§5.2) |
| 3 | Streaming request bodies | OUT (by design) | Platform floor no longer blocks it — but an effect carries complete data; `File` is the large-upload primitive (§5.3) |
| 4 | One total deadline | CORE(adapter) | Apple/.NET synthesize the per-request deadline (timer + cancel); OkHttp/reqwest native (§5.4) |
| 5 | Fine timeouts (connect/read/write) | CFG | Client-wide where they exist at all — not per-request (§5.4) |
| 6 | Redirects: auto-follow, final URL, count | CORE(adapter) | Follow + final URL native everywhere; the https→http refusal is adapter-enforced outside .NET (§5.5) |
| 7 | Redirect hop trace | CORE(adapter) | Upgraded from CAP: Apple/OkHttp/reqwest have hooks; .NET synthesizes by manual follow (§5.5) |
| 8 | Cookie-less, cache-less default | CORE | Confirmed; defaults conflict per platform, so the contract picks |
| 9 | Conditional requests (ETag/304 app-owned) | CORE | Portable everywhere; on Apple the adapter must run cache-disabled (§5.6) |
| 10 | HTTPS-only; cleartext dev-gated | CORE | ATS / API 28 enforce natively; core-checkable before dispatch on Windows/Linux (§5.15) |
| 11 | Negotiated version observable | CORE | Upgraded: all four surfaces always report it — the `Option` was web's (§5.7) |
| 12 | Priority hint | CORE (hint) — re-decided | Re-ruled 2026-07-21 (contract review Q10): **uniform hint, no-op where the engine can't honor it** — the CAP marker trait goes; ignoring was already legal per the row's own contract, and the apple-only capability was the sole surface divergence forcing two FFI bridge crates (§5.8, §8) |
| 13 | Download progress (total = `Option`) | CORE | Portable, but totals lie under compression — total is always optional (§5.9) |
| 14 | Upload progress | CORE(adapter) | Upgraded from CAP: OS-fed on Apple; sink/stream wrapping on OkHttp/.NET/reqwest (§5.9) |
| 15 | Response body sink: `Memory` \| `File` | CORE(adapter) | Native `downloadTask` on Apple; stream-copy synthesis on the other three (§5.10) |
| 16 | Response streaming (chunked delivery) | CORE — decided | Gate cleared (step 24 S-FFI, 2026-07-19): all three shapes 100/100 at 0.27.5 in the http round-trip; mechanism = `ffi_stream` async push (F1), callback push (F2) recorded as perf alternative — `spikes/http-ffi/docs/sffi-streaming-verdict.md`. Core seam **ruled 2026-07-21**: typed-input chunk re-entry, bounded ring + fail-loud, `BodyEnd` terminal, driver-owned lifecycle — [`docs/design/streaming-seam.md`](../../../docs/design/streaming-seam.md) (§5.11) |
| 17 | Decoded bodies; `content_length` advisory | CORE | Adapters must normalize (gzip/brotli/zstd transport-owned) (§5.12) |
| 18 | Metrics (phase timings, TLS detail) | CAP (tiered) | Rich Apple/.NET/OkHttp; coarse Linux — reqwest has no phase seam to synthesize from (§5.13) |
| 19 | Pinning (declarative SPKI) | CORE(adapter) — decided | Linux gate cleared (step 24 L2, 2026-07-19): rustls custom verifier = real WebPKI chain+hostname AND SHA-256-SPKI pins, mismatch ⇒ typed `PinMismatch`; proven in `bolted-http-linux` against real certs. Native on Android; adapter code on Apple/.NET (§5.14) |
| 20 | Errors as typed keys | CORE | Taxonomy grows: permission-denied, cancelled-vs-timeout (§5.15) |
| 21 | Cancellation of in-flight effects | CORE | Everywhere; pause/resume of foreground calls exists nowhere (§5.16) |
| 22 | Retry | split | Connection-level recovery = CFG; request-level retry = the core's job (§5.17) |
| 23 | Auth: 401/407 as data; ambient OS auth | CORE / CFG | Challenge callbacks unportable; NTLM/Negotiate impossible on Android (§5.18) |
| 24 | Client certificates | CFG | Composition-root concern on all four surfaces |
| 25 | Proxy, trust roots | CFG | Unchanged; Linux = env-vars-only asterisk (§5.19) |
| 26 | Cookies as values (capability) | §9-open | Direction recorded 2026-07-21: opt-in, core-owned jar; per-hop re-entry designed with the streaming seam (§5.20) |
| 27 | Trailers, 1xx/103, server push | OUT | Apple/reqwest expose no trailer API; 103 unexposed; push is dead everywhere (§5.15 n/a) |
| 28 | WebSocket | parked family | §2 — honest contract needs its own design pass |
| 29 | Enterprise auth (NTLM/Kerberos), WPAD | OUT | Windows/Apple-only; Android has no built-in NTLM |
| 30 | Background transfer | separate family | Sharpened, still §9 (§6) |

**The CORE(adapter) count: seven rows** (2, 4, 6, 7, 14, 15, 19) — dimensions where at least
one native stack lacks the feature and the shipped adapter compensates in custom code. Three
of them (7, 14, 19) were capabilities while web was in the set; with four synthesizing
surfaces they are portable. platform-surfaces §9 gives the per-surface synthesis table.

**Apple / URLSession leg proven (step 25 S-AP, 2026-07-19).** The hand-written URLSession adapter
(`apple/bolted-http/BoltedHttp.swift`) passes the full C1/C2/C3 conformance suite, and the step-25
M4 mutation pass confirms each synthesis is genuinely pinned on this surface (not vacuously green):

- **CORE(adapter) syntheses proven on Apple** — row 4 (total-deadline `DispatchSource` timer, cancel
  by cause; the *per-idle vs total* distinction is now pinned by the new `/drip` row —
  `timeoutInterval`-substitution mutation caught), row 6 (`willPerformHTTPRedirection` https→http
  refusal — follow-the-downgrade mutation caught), row 7 (hop trace — drop-a-hop and
  misreport-`final_url` mutations caught), row 14 (OS-fed `didSendBodyData` progress — non-monotone
  and non-terminal mutations caught), row 15 (`downloadTask` file sink with synchronous
  temp-then-rename — skip-rename and Memory-for-File mutations caught), row 19 (SPKI pinning split in
  the trust delegate — bypass, wrong-SPKI, and `PinMismatch`-vs-`Tls` conflation mutations caught).
  Row 2 (bodies) exercised via the row-11 POST upload.
- **Row 16 (response streaming)** — Apple evidence: the A1 probe (F1 `ffi_stream` async push) delivers
  ordered/lossless/complete over a real URLSession round-trip; probe-grade, no contract surface added.
- **Row 12 (priority hint, CAP)** — Apple acceptance proven: the hint maps to `URLSessionTask.priority`
  (five contract levels onto URLSession's three named buckets) and the task carries it; the
  swap-the-buckets mutation is caught by the A5 acceptance assertion. RFC 9218 wire behaviour stays
  FLAGGED lore, not conformance-tested.
- **Row 11 (negotiated version)** — now has a positive control on every implementor: the step-25 M4
  pass found that *no* row read `version()` (a blind spot) and added `C1/row-negotiated-version-observable`;
  Apple reports the real `URLSessionTaskMetrics` protocol.

**Android / OkHttp leg proven (step 26 S-AN, 2026-07-20).** The hand-written OkHttp adapter
(`android/bolted-http/.../BoltedHttp.kt`) passes the full C1/C2/C3 conformance suite on the headless
ART tier (aosp_atd android-34 GMD), and the step-26 M4 mutation pass confirms each synthesis is
genuinely pinned on this surface (19 of 22 behavioural mutations caught, 1 structural guarantee
compile-enforced, 3 survivors dispositioned):

- **CORE(adapter) syntheses proven on Android** — row 4 (total-deadline via OkHttp `callTimeout`, cancel
  by recorded cause; the *per-idle vs total* distinction pinned by the `/drip` row — the
  `callTimeout⇒readTimeout` mutation caught), row 6 (`followSslRedirects(false)` + downgrade-refusal —
  follow-the-downgrade / broken-too-many-redirects mutations caught), row 7 (`priorResponse` hop trace —
  drop-a-hop, **reorder-the-hops** (a new blind spot), and misreport-`final_url` mutations caught), row
  14 (`ForwardingSink` upload progress — wrong-token-routing and terminal mutations caught), row 15
  (Okio file sink with temp-then-`renameTo` — skip-rename, Memory-for-File, and swallow-`Io` mutations
  caught), row 19 (SPKI pinning split in the custom `X509TrustManager` — corrupt-SPKI, require-all, and
  both directions of the `PinMismatch`-vs-`Tls` conflation caught). Row 2 (bodies) exercised via the
  row-11 POST upload.
- **Row 11 (negotiated version)** — Android reports the real OkHttp `Response.protocol`; the
  fixed-wrong-version mutation is caught by `C1/row-negotiated-version-observable` (the step-25 M4 row).
- **Row 12 (priority hint, CAP)** — **absent** on the C3 Android column (the decided Apple/Android
  divergence): OkHttp exposes no per-`Call` priority knob, so the adapter legally ignores the hint.
- **Row 16 (response streaming)** — the A1 probe (F1 `ffi_stream` async push) delivers ordered/lossless/
  complete over a real OkHttp round-trip, whole + ordered even under CPU saturation (M3); probe-grade.
- **The M4 blind spot (redirect hop *order*)** — the redirect-trace row asserted hop *count* + `final_url`
  but not *order*; the Android `redirectHops` (which reverses OkHttp's last-first `priorResponse` chain)
  drop-the-reversal mutation survived the whole suite. Fixed on every implementor:
  `C1/row-redirect-trace-final-url-and-hops` now asserts traversal order → new `FailureReason::WrongHopOrder`
  + mock `honest_redirect_hop_order` knob.
- **Bridge single-flight** — the FFI completion registry is token-keyed and single-flight; misrouting a
  completion/progress to the wrong token is caught (`NoCompletion` / `ProgressNotTerminal`), and
  **double-completion is structurally impossible** (`CompletionSink::complete(self: Box<Self>)` +
  remove-and-return `take_pending` ⇒ a second completion fails to compile).

## 5. Dimension notes — the evidence behind each row

### 5.1 Headers (row 1)
Every stack owns a slice of the header space: .NET's restricted-header model rejects or
redirects headers like `Host` and `Content-Length` (content headers live on the content
object); OkHttp's `BridgeInterceptor` writes `Host`, `Content-Length`/`Transfer-Encoding`,
`Accept-Encoding`, and `Connection` itself; URLSession similarly reserves
connection-management headers. The contract therefore declares a **reserved-header list the
adapter owns**: `Accept-Encoding`, `Cookie`, `Host`, `Content-Length`, connection-management
headers. Core-set reserved headers are a **type error, not a runtime drop**. `Authorization`
is settable everywhere, with one portable rule adopted from .NET's enforced behavior: it is
stripped on cross-origin redirects (free to enforce in the other adapters, and it closes a
real credential-leak class).

### 5.2 Bodies (row 2)
`Bytes | File | Multipart{parts: Bytes|File}`. `File` is honestly portable: Apple
`uploadTask(fromFile:)`; OkHttp file `RequestBody`; .NET stream content; reqwest file streams.
Multipart is first-class only on OkHttp (`MultipartBody`) — the Apple and .NET adapters
construct the body manually (the CORE(adapter) synthesis), which is fine because the
**boundary string must come from the core anyway** (derived from the effect id —
deterministic, replayable; an adapter-generated random boundary would make the recorded input
stream non-reproducible).

### 5.3 Streaming request bodies (row 3)
Reclassified: the old OUT verdict rested on the web floor (fetch upload streaming is
Chromium-only). All four remaining surfaces *can* stream request bodies (Apple stream tasks,
OkHttp streaming `RequestBody`, .NET stream content, reqwest `Body::wrap_stream`). Row 3
stays OUT anyway, now **by design**: an effect is complete data handed over once — a streamed
request body would be an open channel from the core to the adapter, which breaks
one-effect-one-completion and replay. The portable "large upload" primitives are `File`
bodies (§5.2) and the background family (§6). If a feature ever genuinely needs
core-generated streaming uploads, that is a design session, and the FFI gate of §5.11 applies
doubly.

### 5.4 Timeouts (rows 4–5)
The deadline-only core survives contact with all new evidence — but it is a **synthesis** on
half the surfaces, hence CORE(adapter): Apple has idle + session-resource timers and **no
per-request wall-clock deadline** (the adapter arms a timer and cancels the task; the
documented interaction that `waitsForConnectivity` suspends the idle timer but not the total
is SDK-header-verified); .NET's `HttpClient.Timeout` is client-wide and its 100 s default
**silently stops governing the body once you stream (`ResponseHeadersRead`)** — the
streamed-read timeout hole is real (runtime#36822, FLAGGED GitHub-only) and the adapter must
synthesize per-read deadlines with re-armed cancellation. OkHttp (`Call.timeout()`) and
reqwest (`RequestBuilder.timeout()`) are per-request natively. Fine timeouts are **CFG**, not
CAP: per-request connect/read timeouts are not honestly expressible anywhere — reqwest's
`connect_timeout`/`read_timeout` are client-wide; .NET's `ConnectTimeout` is handler-wide.
So: **deadline per request in the core; everything finer is adapter construction detail**
configured at the composition root, with the conformance suite pinning observable behavior
(a stalled server must produce `timeout` before deadline+ε on every adapter, however the
adapter achieves it).

### 5.5 Redirects (rows 6–7)
Core: follow, report final URL + hop count; **https→http is never followed**. That refusal is
.NET's enforced behavior on modern .NET; on Apple the delegate refuses, on OkHttp
`followSslRedirects(false)` covers it (harmless to http→https since the contract is
HTTPS-only anyway), on reqwest a custom policy — adopting .NET's constraint as the contract
rule makes it everyone's guarantee, synthesized where not native. The hop **trace** (row 7)
upgrades from CAP to CORE(adapter): Apple's `willPerformHTTPRedirection` sees every hop,
OkHttp network interceptors see each wire request, reqwest's custom policy sees the full
chain per hop (sync closure — so the trace is *recorded*, never an async veto); .NET has no
observation hook for auto-followed hops, so its adapter **synthesizes the trace by disabling
auto-redirect and following manually** — legitimate here precisely because the contract is
cookie-less and strips `Authorization` cross-origin itself (§5.1), so manual following loses
no stack behavior we rely on. Background transfer never exposes hops on any platform — the
trace is a foreground-only promise. WinRT has no redirect-count knob at all ("set internally
by the system") — one more reason the C# adapter is .NET, not WinRT.

**Ruled 2026-07-21 (contract review Q2): the redirect ceiling is CFG** — a core-owned value
set at the composition root, with **core-counted exhaustion**: the adapter's native limit is
set above the ceiling, the core counts hops from the trace (row 7) and emits the typed
`TooManyRedirects` itself. This removes the classifier's one unavoidable exception-text match
(OkHttp's `ProtocolException` message) and closes the honest-limit gap — no platform
documents its native ceiling as contract.

### 5.6 Cookies, cache, conditional requests (rows 8–9)
Cookie-less/cache-less default confirmed from both directions (URLSession both-on vs
OkHttp/.NET both-off). Row 9: **app-owned conditional requests are portable**.
`If-None-Match`/`If-Modified-Since` are settable on every surface, and a real 304 (empty
body) reaches the adapter as long as no platform cache intercepts — on Apple the adapter
uses ephemeral/no-store configuration so the URLCache never replays a 200 for a manual
`If-None-Match`; OkHttp/.NET have no cache unless configured; reqwest has none. Consequence:
**an ETag-revalidation facet flow needs no cache capability at all** — 304 is just a typed
response. Adapter rule: foreground requests run cache-disabled, so protocol caching never
silently changes replay behavior.

### 5.7 Version observability (row 11)
Upgraded: `HttpVersion` is a plain (non-`Option`) response field — every surface always
reports it (URLSession `networkProtocolName`, OkHttp `Response.protocol`, .NET `Version`,
reqwest `version()`). The `Option` in the first draft existed solely for web's
TAO-gated Resource Timing (§9). HTTP/3 remains a hint nowhere promised: OkHttp caps at h2
(HttpEngine covers h3 on API 34+), Windows 11+ only with silent fallback, reqwest's h3 still
behind `reqwest_unstable` (quinn).

### 5.8 Priority (row 12)
The CORE-as-hint proposal, **weakened by web's departure** — web (tri-engine fetch
`priority`) was its strongest supporter. What remains: Apple `URLSessionTask.priority`
(0–1 float; empirically emits RFC 9218 `Priority: u=N` on the wire — FLAGGED
observed-not-documented, so the *wire mapping* stays out of the contract) and
Cronet/HttpEngine (five levels). Legally ignored by OkHttp (no API, FIFO dispatcher) and .NET
(closed-wontfix upstream) — so on the default Android engine (OkHttp) the hint does nothing.
Two honoring surfaces out of four is thin for CORE even as a hint; the row stays as proposed
only because "hint" means acceptance-only conformance, but the recommendation now leans CAP
— **Henrik's call either way** (§8). No adapter synthesis is possible: there is no knob to
write custom code against, and faking wire priority is not compensation.

**Re-ruled 2026-07-21 (contract review Q10): uniform CORE hint after all.** The 2026-07-19
CAP call was made before upstream note 08 established that bindgen evaluates no `#[cfg]`
(the union of items lands in every target's bindings), which made the apple-only capability
the sole reason `bolted-http-apple-ffi` / `bolted-http-android-ffi` were two crates. Since
ignoring the hint is *legal per the row's own contract* (acceptance-only conformance — OkHttp
already ignores it), uniform-with-no-op costs nothing the CAP shape was protecting, and the
bridge crates merged into one multi-target crate (`bolted-http-ffi`, step-27 M0). Precedent
stated with the ruling:
uniform-with-no-op is preferred **only** when ignoring is legal per the capability's own
contract; otherwise a divergence is real and gets a real seam.

### 5.9 Progress (rows 13–14)
Download progress is CORE with contract-defined byte semantics: **bytes are as observed by the
adapter after transport decoding; the total is always `Option`; counters may restart** (the
platforms force all three: OkHttp's transparent gzip strips `Content-Length`; Windows' two
APIs disagree on whether bytes include headers and both may regress on restart). Upload
progress (row 14) upgrades from CAP to CORE(adapter): Apple's `didSendBodyData` is OS-fed;
OkHttp, .NET, and reqwest adapters synthesize by wrapping the sink/stream (the OkHttp recipe
wraps the `RequestBody` sink; naïve .NET wrappers jump to 100% — the adapter must wrap the
content stream with flush-aware counting; reqwest wraps the `Body` stream). Synthesized
figures measure buffer hand-off, not wire bytes — so the contract text must say
**"indicative, monotone per attempt, not wire-truth"**, and the conformance suite pins
exactly that (rule 11), no more.

### 5.10 Download-to-file (row 15)
Every surface can sink a response to a file without buffering it in memory: Apple
`downloadTask` natively (temp file, move-synchronously rule); OkHttp sink-to-file, .NET
stream copy, and reqwest `bytes_stream`-to-disk as adapter synthesis — hence CORE(adapter).
With web out of the set, the destination is **a path on every surface** — the opaque
`FileRef` indirection the first draft introduced for OPFS is no longer forced. Keep the
newtype anyway (`FileRef` wrapping a path): it costs nothing, the background family wants the
same type, and it is the seam a web adapter would reinterpret if web ever joins (§9). Its
home (bolted-core? bolted-http?) is still the §8 structural question.

### 5.11 Response streaming (row 16)
Platform-side: portable, full stop (AsyncBytes/delegate on Apple with a FLAGGED
no-flush-guarantee caveat for latency-critical SSE; OkHttp source streams; .NET
`ResponseHeadersRead`; reqwest `bytes_stream`). FFI-side: triage T1 found both step-02
probes' stream machinery converges at boltffi 0.27.5, which clears the old kill criterion —
but the *mechanism* (callback-trait push vs wake-and-read vs ffi_stream) was explicitly
deferred by the stall report ("decide there, not here"). Proposal: response streaming enters
the portable core **conditioned on one spike probe** (S-FFI in [spike-plan.md](spike-plan.md))
re-running the stream shapes at ≥0.27.5 inside the http round-trip, choosing the mechanism on
measurements. If it stalls again, row 16 falls back to `Memory | File` sinks only — which,
note, already cover most facet needs; that fallback would park SSE with WebSocket.

**Both halves now decided.** The mechanism gate cleared (step 24 S-FFI: F1 `ffi_stream`
push, re-proven on Apple A1 and ART N2 at 200/200); the core seam was **ruled 2026-07-21**
(contract review Q1, adopted as proposed): chunk re-entry as a typed input
(token-keyed, seq-verified), bounded core-side ring + fail-loud `StreamOverflow`,
back-pressure as a capability extension, `BodyEnd { Complete { total } | Failed }` terminal
with a completeness gate, driver-owned subscription lifecycle. Full design + the three new
conformance rows + the upstream-RFC re-evaluation trigger:
[`docs/design/streaming-seam.md`](../../../docs/design/streaming-seam.md).

### 5.12 Compression (row 17)
Adapters normalize: .NET must set `DecompressionMethods.All` (default is **None**); OkHttp
default transparent gzip is kept but the adapter must surface `content_length = None` honestly
(gzip strips it) and add the brotli/zstd modules; Apple sends `gzip, deflate, br` (empirical;
doc-silent, FLAGGED) and cannot disable decoding except by owning `Accept-Encoding`; reqwest
enables gzip/brotli/zstd via features. Contract: bodies are always decoded; `content_length`
advisory `Option`; no "raw body" promise exists.

**Ruled 2026-07-21 (contract review Q3), after re-verifying that reliability is impossible in
principle, not a platform gap**: `Content-Length` frames the *encoded* content (RFC 9110), so
the decoded length of a compressed or chunked response is unknowable up front on any client
(live control: wire `Content-Length: 94760`, decoded 611 471 bytes — 6.5×). Wording adopted:
always advisory `Option`; the **file sink reports verified bytes-written on completion** —
the one place a trustworthy total exists, because the adapter counted it.

### 5.13 Metrics (row 18)
Tiered capability, corrected from the earlier study: **Tier A** (phase timings + TLS detail):
Apple TaskMetrics, .NET 8+ meters/OTel spans (connection-level spans experimental), OkHttp
EventListener (timing only, no TLS metadata, events repeat under retries — and OkHttp 5's
default Happy-Eyeballs `fastFallback` makes "connect time" per-*attempt*, races included).
**Tier B** (whole-request only): Linux/reqwest (no phase API; `connector_layer` self-timing at
best), WinRT/BackgroundTransfer (nothing). This is the clearest example of a row that **cannot
be promoted to CORE(adapter)**: there is no seam inside reqwest from which adapter code could
recover DNS/TLS phase timings — synthesis requires a hook, and none exists. The capability
type should expose tier, not pretend uniformity.

### 5.14 Pinning (row 19)
Upgraded from CAP to CORE(adapter), gated on one spike probe. Declarative SPKI data in the
core, mapped per adapter: **Android is the native case** (`CertificatePinner` on OkHttp /
`addPublicKeyPins` on HttpEngine/Cronet); **Apple synthesizes** in the trust-evaluation
delegate (SecTrust + pin comparison — shipped adapter code, not app code); **.NET
synthesizes** via `SslOptions.RemoteCertificateValidationCallback`; **Linux synthesizes**
with `tls_backend_preconfigured` + a hand-built rustls verifier carrying the pin data —
feasible on paper, but it is real adapter code the conformance suite must cover, and the
spike (S-LX2) gates the row: if the rustls verifier cannot express SPKI pinning cleanly, the
row demotes back to CAP with Linux absent. NSC `<pin-set>` on Android is defense-in-depth
only (Conscrypt-enforced, custom-TrustManager-fragile, Cronet-undocumented, websocket-blind)
— the suite tests the *adapter's* pins, never NSC's. One more Android landmine: with
per-domain NSC configs present, the platform's `RootTrustManager` *throws* on hostname-less
trust checks — the adapter must never route through a plain 2-arg `checkServerTrusted` path.

### 5.15 Error taxonomy (row 20)
Typed keys, growing from the research: **`PermissionDenied`** — Android 16→17 local-network
permission (`EPERM` on LAN HTTP for targeting-17 apps) and Apple's Local Network privacy
prompt make "the platform asked the user and the user said no" a first-class outcome,
distinct from network failure; **timeout vs cancel** — one key each, and the operative trap
is .NET, where **both surface as `TaskCanceledException`** — the adapter classifies by which
token fired, never by exception type; **`QuotaExceeded`** reserved for the background family
(Windows' 200-op queue, Android quotas). The RN-2026 lesson from prior-art stands: the
native-failure → key mapping is conformance-tested per adapter, never judgment.
**`PermissionDenied` ruled 2026-07-21 (contract review Q5)**: the contract states it as an
inherently **device/app-bundle-tier** outcome (the OS prompt needs a real bundle identity and
a user), so the shared suite never asserts it; each adapter owes a **unit-proven cause
mapping with negative controls** instead. HTTPS-only
(row 10) is enforced natively by ATS and Android API 28+; on Windows/Linux nothing forbids
cleartext, but the check needs no adapter at all — the sans-io core can refuse a non-https
URL before the effect is ever emitted, which makes the rule uniform for free.

### 5.16 Cancellation (row 21)
CORE: any in-flight effect is cancellable; completion arrives as the `Cancelled` typed input
(one effect, one completion, always — cancellation is not a silent drop). **Ruled 2026-07-21
(contract review Q4): cancellation is push-delivered** — a core→adapter mid-flight signal on
the capability trait, replacing the poll-watcher thread all three adapters currently pay;
designed together with the streaming back-pressure signal (streaming-seam §3b) as **one**
core→adapter mid-flight surface. Pause/resume of
foreground calls exists on no platform (OkHttp/Cronet/reqwest: confirmed none) — OUT.
Range-based resumption is an app-level pattern over rows 1/9 (`Range` + `If-Range` are just
headers); OS-managed resume (Apple resume data with its five-condition validity list, iOS 17
resumable uploads speaking the IETF draft) belongs to the background family.

### 5.17 Retry (row 22)
New explicit split, previously implicit: **connection-level recovery** (stale pooled
connections, Happy Eyeballs races, alternate IPs — OkHttp `retryOnConnectionFailure`,
`fastFallback`) is transport detail the adapters keep at platform defaults; **request-level
retry is the core's** — adapters must not re-send a request that reached the wire (reqwest's
new built-in `retry()` stays OFF in the Linux adapter). Rationale: one effect = one completion
is what keeps HTTP inside replay/determinism; a policy that quietly re-POSTs breaks it. The
conformance suite needs a positive control here (a request that fails mid-flight must surface
the typed error, not a hidden retry).

### 5.18 Auth (rows 23–24, 29)
Portable core: 401/407 are ordinary typed responses; the core decides and re-emits with
credentials (preemptive `Authorization`). Challenge *callbacks* stay unportable (Apple's
two-tier delegate routing — session-level for NTLM/Negotiate/ClientCert/ServerTrust,
task-level for Basic/Digest, a documented trap; .NET buries them in handler config). Ambient
OS auth (Kerberos/NTLM via logged-in identity) is CFG on Windows/Apple, impossible on Android
(no built-in NTLM; Digest is a FLAGGED third-party crate situation on both Android and Rust)
— hence row 29 OUT for the contract, and no adapter synthesis is honest here (reimplementing
NTLM in adapter code is a security liability, not a compensation). Client certs: CFG at the
composition root (Apple `SecIdentity`, .NET `SslOptions`, OkHttp KeyManager+KeyChain user
grant, reqwest `identity()`); absent on Cronet (FLAGGED by-absence).

### 5.19 Proxy and trust (row 25)
Unchanged CFG, with the Linux asterisk now source-verified: reqwest 0.13's default
`system-proxy` reads the Windows registry and macOS SCDynamicStore but is **env-vars-only on
Linux** — no gsettings, no kioslaverc, no PAC, no portal (the XDG ProxyResolver portal exists
but only GLib consumes it). A GNOME/KDE user's GUI proxy settings are invisible to the Linux
adapter unless exported as env vars; `Proxy::custom` is the seam if this ever matters.
Document as adapter behavior; do not promise "system proxy" on Linux.

### 5.20 Cookies as values (row 26 — still §9; direction recorded 2026-07-21)
Evidence gathered for the eventual design session, and the shape got simpler without web:
per-request participation is expressible on every surface (Apple
`httpShouldHandleCookies`, OkHttp jar choice, .NET `CookieContainer` per handler), and cookie
*values* are readable on every surface — the "participate but never read" split that web's
browser-owned jar forced is gone.

**Direction (Henrik, 2026-07-21) — opt-in, core-owned jar; evaluate in a future design
session** (implement when a feature needs it; additive as a capability, so not freeze-gated):

- **Opt-in at the composition root, never default-on** — an ambient jar breaks same-request-
  same-outcome and re-opens the row-8 divergence. Platform jars stay disabled *forever*; the
  capability is a core-owned `CookieJar` value (serializable, replayable, mock-testable) with
  the adapter as transport. `"cookie"` stays user-reserved — only the jar path attaches it.
- **Typed `Cookie` values at the seam, not raw header lines** — forced by Apple:
  `allHeaderFields` comma-coalesces repeated `Set-Cookie`, so the Apple adapter must go
  through `HTTPCookie.cookies(withResponseHeaderFields:)`; a gnarly-fixture conformance row
  (quoted commas, odd dates) must pin parser agreement.
- **The open design problem: per-hop consultation.** A cookie set on a redirect hop
  (`POST /login → 302 + Set-Cookie → GET /home`) must be ingested and re-attached *mid-flight*,
  inside one `send()` — a new adapter→core re-entry at each hop boundary. Hooks exist
  everywhere (Apple `willPerformHTTPRedirection`, OkHttp network interceptor; .NET/Linux
  follow manually anyway). This is the same mid-flight re-entry shape as the streaming seam —
  design the two together so one pattern serves both. A minimal v1 may defer it with a
  documented "redirect-set cookies apply from the next request" caveat. **Ruled 2026-07-21
  (contract review Q9): the mid-flight adapter→core re-entry shape is defined once**
  (streaming-seam §4 — naming, token discipline, ordering, replay semantics) **and
  instantiated twice** (chunks now, cookie per-hop when the capability lands); the capability
  itself stays deferred.
- Smaller session items: expiry needs injected time (sans-io); skip the Public Suffix List
  (native app chooses its hosts — document the threat-model call); `Secure` enforced in core;
  `SameSite`/`HttpOnly` meaningless client-side; web (if it ever joins) is participate-only
  via `credentials: 'include'` — the split returns, quarantined as a CAP demotion.
- **Verify when the C# leg unparks:** the "both-off" claim above for .NET — `UseCookies`
  may default *true* (fresh per-handler container); the adapter likely needs an explicit
  `UseCookies = false` to honor the cookie-less contract.

## 6. The background family, sharpened

Still §9-open; the research moves four things from guess to fact:

1. **The intersection contract holds** (file-based, durable descriptors, handover, completion
   to a possibly-new instance, force-quit loss legal) — no platform contradicted it, and the
   "iOS ≤4h" pseudo-bound is gone (§1.1).
2. **Availability is a runtime fact with platform-specific *preconditions*, and the contract
   must carry them**: Android UIDT is schedulable **only while the app is visible** (else
   `RESULT_FAILURE`), API 34+, with a user-facing Stop that kills the process without
   `onStopJob`; Windows needs package identity (sparse packages count) and caps at 200
   operations with mandatory reattach ceremony; iOS relaunch is rate-limited with
   force-discretionary-when-backgrounded; Linux has no OS service (item 5 is the adapter's
   answer). So the family's capability surface is
   `availability() -> {Available, NeedsForeground, NoIdentity, Unsupported, …}` —
   queried, not assumed, and the *scheduling precondition* is part of the type.
3. **Android's adapter is a three-way dispatch** (UIDT on 34+ / WorkManager / DownloadManager
   for plain downloads), all implementing the iOS-shaped contract; DownloadManager transfers
   survive even force-stop (separate system process; FLAGGED undocumented) but completion
   *broadcasts* to a stopped app do not — reattach-on-launch is the portable ceremony on every
   platform (Apple identifier re-attach, Windows `GetCurrentDownloads/UploadsAsync` +
   `AttachAsync`, Android query-on-start).
4. **Progress byte semantics must be self-defined** (§5.9's rule doubly so here: Windows'
   BackgroundTransfer excludes headers where WinRT foreground includes them; both regress on
   restart; `HasRestarted` is a field, not an anomaly).
5. **Linux joins the family via a detached helper** (proposed 2026-07-18): the adapter
   re-execs the app's own binary with a worker flag (or starts a `systemd-run --user`
   transient unit — never an embedded second binary, which is pure packaging burden);
   descriptors, progress, and results are files in the XDG state directory (which *is* the
   family's required durable-descriptor shape — nothing extra to invent); a
   pidfile/liveness check lets reattach-on-launch classify a crashed helper as lost, which
   the contract already legalizes. Uploads come free — file-based bodies, better than
   DownloadManager's downloads-only. `availability()` reports the honest limits: no reboot
   survival (note the contract should not promise reboot survival *globally* either — iOS's
   behavior there is unverified, and force-quit loss is already legal), logout survival
   environment-dependent, no cost/scheduling policies (metered via NetworkManager at best),
   and Flatpak/snap mechanics unresolved (Background portal — a probe question deferred
   with the family, alongside A7/W3). Like Android's WorkManager path, this is app code
   running with more freedom than iOS allows — it implements the iOS-shaped contract,
   never widens it.

## 7. Conformance rules the research forces

Rules that go into the suite's fixed rows regardless of final contract shape. Note how many
of them exist to pin an **adapter synthesis** (the CORE(adapter) rows) — that is the class's
cost, and the suite is where it is paid:

1. Same request ⇒ same typed response/error on every adapter (unchanged foundation).
2. Timeout-vs-cancel classified identically everywhere — on .NET both are
   `TaskCanceledException`; the adapter must classify by token, never exception type.
3. A stalled-body server yields `timeout` ≤ deadline+ε on every adapter (kills the .NET
   streamed-read hole and any idle-timer surprise; pins the row-4 synthesis).
4. https→http redirect: refused identically everywhere (natively only on .NET; the rule
   proves the other three syntheses).
5. Manual `If-None-Match` yields a real 304 (not a cache-replayed 200) on every adapter.
6. Reserved headers: core-set is a compile error; adapter never silently drops a permitted one.
7. Decoded-body invariant: a gzip/brotli response yields identical bytes + `content_length:
   None`-or-honest on every adapter.
8. One effect, one completion: mid-flight failure surfaces the typed error — no hidden
   request-level retry (positive control required).
9. Cancellation always completes the effect with `Cancelled` (never silence).
10. Pin mismatch ⇒ the same typed pinning error on all four adapters. Android additionally:
    pinning survives a custom `TrustManager` being absent — i.e., the suite tests the
    *adapter's* pins, never NSC's.
11. Upload progress is monotone per attempt and terminally consistent with the completion —
    pinned on the three synthesized surfaces (OkHttp/.NET/reqwest) against Apple's OS-fed
    baseline; the suite never asserts wire-truth (the contract doesn't promise it).
    **Amended 2026-07-21 (contract review Q8)**: when the upload's `content_length` is known
    (it always is — bodies are `Bytes | File`), the row also asserts the terminal `total`
    (F-M4-3 closed: `total` was unasserted on every implementor).
12. *(Added 2026-07-21, streaming-seam §5.)* Slow-consumer completeness: delivered ==
    ingested, or the typed overflow error — green by silent drop is impossible.
13. *(Added 2026-07-21.)* Terminal-exactly-once: chunks then exactly one `BodyEnd`;
    truncation ⇒ failure (completeness gate `total == ingested`).
14. *(Added 2026-07-21.)* Subscription hygiene: after N streamed responses, the
    live-subscription count is back to baseline (platform tiers — the F-M3-1/F-M0-5 leak is
    the red case).

## 8. Still open after this round

*(2026-07-21: the contract-review session ruled on every open contract question — the
decision record is [`docs/design/contract-freeze-agenda.md`](../../../docs/design/contract-freeze-agenda.md);
strikethroughs below updated accordingly.)*

- ~~**FFI streaming mechanism** (§5.11)~~ — **decided by S-FFI (step 24, 2026-07-19): F1
  `ffi_stream` async push**; row 16 is CORE. ~~The *core seam* (chunk re-entry, back-pressure,
  end-of-body) is the freeze session's.~~ **Seam ruled 2026-07-21** —
  [`streaming-seam.md`](../../../docs/design/streaming-seam.md).
- ~~**Pinning-on-Linux feasibility** (§5.14)~~ — **decided by S-LX2 (step 24, 2026-07-19):
  feasible**; row 19 stands as CORE(adapter).
- **Cookie capability shape** (§5.20) — design session, when a feature needs it; the
  mid-flight re-entry shape it needs is already defined (Q9, streaming-seam §4).
- **WebSocket family** (§2) — protected possibility, undesigned.
- ~~**`FileRef`** (§5.10) — its home (bolted-core? bolted-http?)~~ — **decided 2026-07-19
  (design session, step-24 authoring): `FileRef` lives in `bolted-http`**, a newtype over a
  path, kept opaque-ready (§9's OPFS case is why). Rationale: bolted-http is its only
  consumer; if the durable-effects family (background transfer, replay) ever needs it
  core-side, lifting a newtype is a re-export exercise, not a break.
- **Background family full contract** (§6) — unchanged §9 status, better-informed.
- ~~Whether the priority hint (§5.8) survives Henrik's review as CORE~~ — decided 2026-07-19
  (Henrik): CAP — then **re-ruled 2026-07-21 (Q10): uniform CORE hint with legal no-op**,
  once upstream note 08 showed the CAP shape was forcing two FFI bridge crates for nothing
  the row's own contract protects (§5.8 has the full reversal rationale and the precedent).
- Whether web ever joins the platform set — still open, but the early decision it forced is
  **taken 2026-07-19 (Henrik): the contract traits adopt the conditional `Send`-bound pattern
  from day one** (§9), so a later web adapter is never locked out at the type level.

## 9. Web — out of the platform set: how it would fit

Removed 2026-07-18: the asked surface is win/lin/mac/android/ios. Bolted does have a Rust-web
shell target (zero FFI), so the question may return; this section records what the web sweep
established so the door has a map on it. Raw evidence:
[research/2026-07-18-web.md](research/2026-07-18-web.md).

If a feature on the web shell ever needs HTTP, the web adapter would be a fifth conformance
implementor (fetch via web-sys, zero FFI — no BoltFFI machinery involved), and these rows
would move:

- **Row 4 (deadline)**: honored via `AbortSignal.timeout` — but classification of timeout vs
  cancel must read `signal.reason`, never the rejection's `.name` (probed: WebKit rejects
  with `AbortError` even on timeout). Rule 2 would grow a web clause.
- **Row 7 (hop trace)**: demotes — fetch never exposes hops.
- **Row 11 (version)**: regains `Option` — observable only same-origin/TAO via Resource
  Timing `nextHopProtocol`.
- **Row 14 (upload progress)**: demotes — fetch has none; XHR is the only mechanism.
- **Row 15 (file sink)**: survives via OPFS `createWritable` (Baseline since Sept 2025,
  probe-confirmed tri-engine) — but the destination is origin-private storage, not a path, so
  `FileRef` becomes genuinely opaque again (the reason the newtype is kept, §5.10).
- **Row 19 (pinning)**: demotes — impossible on web, full stop.
- **Rows 1/6**: the browser silently drops forbidden headers and never exposes redirect
  interception; the reserved-header type guard already in the contract absorbs most of it.
- **Rows 18/26**: metrics only TAO-gated; cookies participate-but-never-read.

Two things worth taking from the sweep *now*:

1. **Trait bounds**: wasm futures are `!Send`. If the contract traits hard-require `Send`
   when they are written, a later web adapter is locked out at the type level — adopt the
   conditional-bound pattern from day one if web joining is at all plausible. This is the
   only web-related decision that is cheap now and expensive later. **Decided 2026-07-19
   (Henrik): adopted — conditional bounds from the first trait written.**
2. The alternative shape is also legitimate: the web shell keeps calling `fetch` directly
   (gloo/web-sys) outside bolted-http, and the contract never grows a fifth adapter. Nothing
   in D38 forces either answer.
