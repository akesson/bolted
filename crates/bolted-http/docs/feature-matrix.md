# bolted-http — the homogenized surface: feature matrix and contract proposal

**Status:** design study, 2026-07-18 — the second investigation round, and a **proposal**: nothing
here is frozen (the D38 shape is decided; the contract itself stays §9-open until a feature
needs HTTP). Builds on [platform-surfaces.md](platform-surfaces.md) and
[prior-art.md](prior-art.md) (both 2026-07-09); where this doc conflicts with them, this doc
wins (§1 lists the corrections).

**Method:** five parallel research sweeps (Apple, Android, Windows, Linux/Rust, Web), each
resolving the §8 verification flags of the earlier study and inventorying the feature
dimensions it skipped (WebSockets, SSE, auth, compression, priorities, progress, pause/resume,
multipart, conditional requests, and 2024–2026 platform additions). Evidence classes: official
docs, SDK headers and library source (AOSP/Conscrypt/Chromium/OkHttp/hyper-util read directly),
and **live probes** — empirical checks on this macOS 26 host and feature-detection probes run
in headless Chromium 150 / Firefox 152 / WebKit 26.5. Claims resting on third-party material
or absence-of-documentation are marked **FLAGGED**, as before.

---

## 1. What changed since the 2026-07-09 studies

Corrections and floor movements the earlier docs should be read with:

1. **The "iOS ≤ 4 h" background cap is lore.** No Apple doc states any wall-clock cap; the only
   documented bound is `timeoutIntervalForResource` (7 days), which *does* apply to background
   sessions. prior-art §5.3 quotes `background_downloader`'s "iOS ≤4h" envelope — keep it
   attributed to that package, not to the platform.
2. **The web floor ROSE in four places** since the study's baseline: OPFS
   `FileSystemFileHandle.createWritable()` is Baseline since Sept 2025 (streaming
   download-to-disk without memory buffering is now portable, into origin-private storage);
   `keepalive` is tri-engine (Firefox 133, Nov 2024); fetch `priority` is tri-engine
   (Firefox 132 / Safari 17.2); `AbortSignal.timeout`/`any` are universal. Also
   `nextHopProtocol` (Resource Timing) gives real h2/h3 observability in all three engines —
   TAO-gated cross-origin.
3. **The web floor stayed capped where it matters**: fetch upload streaming is still
   Chromium-only (and web-sys doesn't even bind `duplex` — from Rust it needs a
   `js_sys::Reflect` hack); upload progress is still XHR-only, everywhere, July 2026; response
   trailers are permanently unobservable (removed from the spec in 2019).
4. **Android's `<pin-set>` question is answered** (§5.14): binds OkHttp by architecture (the
   enforcement lives in Conscrypt's handshake via the platform trust manager, SOURCE-VERIFIED),
   dies silently under a custom `TrustManager`, undocumented for Cronet (which delegates chain
   verification to the hostname-aware platform path but officially points at its own
   `addPublicKeyPins`). NSC explicitly does not cover websockets.
5. **Android gained two stack-level facts the study missed**: `android.net.http.HttpEngine`
   (API 34+, Cronet bundled in the Connectivity Mainline module — HTTP/3 + Brotli + pinning at
   zero APK cost, no Play Services; "the recommended default network stack on Android from
   API 34") and **User-Initiated Data Transfer jobs** (API 34+, JobScheduler-only, quota-exempt,
   mandatory notification with a user Stop button, **schedulable only while the app is
   visible**). Embedded Cronet is dead (~19–20 MB/ABI, FLAGGED community figure; the Maven
   embedded artifact is abandoned).
6. **Windows' foreground stack is settled by Microsoft's own cross-references**: .NET
   `HttpClient`/SocketsHttpHandler for foreground, `Windows.Networking.BackgroundTransfer` for
   large/background (package identity required — but "packaging with external location"
   (sparse packages) grants identity without full MSIX, so *unpackaged ⇒ no background
   transfer* is not a hard equivalence). `Windows.Web.Http` is alive but frozen at h2 with
   2017-era docs. Separately: platform-surfaces §4's WinINet framing is wrong — Microsoft's
   current comparison page recommends WinINet *over* WinHTTP for non-service client code
   (irrelevant to our adapter, but the study should not call it legacy).
7. **§7.9's "timing metrics implementable on every native surface" was too generous**: reqwest
   exposes **no per-phase timing at all** (no DNS/connect/TLS durations; the only seam is a
   tower `connector_layer` you time yourself). The Linux adapter's honest metrics tier is
   coarse. Meanwhile .NET 8+ ships rich built-in meters + OTel spans down to TLS handshake —
   but only on the .NET side; WinRT/BackgroundTransfer expose nothing.
8. **Background Fetch (web) is shipped, not experimental** — but Chromium-only since Chrome 74;
   Mozilla is formally *negative* on Background Sync/Periodic Sync. `fetchLater()` shipped in
   Chrome 135 with positive Mozilla/WebKit signals — the only deferred-send API with a
   plausible cross-engine future. None of this enters a portable contract.
9. **Small but load-bearing**: .NET decompression is **off** by default
   (`DecompressionMethods.None`); OkHttp's transparent gzip **strips `Content-Length`** from
   the response you see; Apple's real shared `URLCache` is ~512 KB memory at runtime (the
   header's "4 MB" is stale lore) — every adapter must set cache/decompression behavior
   explicitly rather than inherit defaults.

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
  closes the socket at a hard 16 MiB queue overflow, web exposes only `bufferedAmount`;
  compression is uncontrollable on Apple/web; Cronet has none; background sessions forbid it).
  Record it as a protected possibility; design it only when a feature needs it.

## 3. Classification vocabulary

Every dimension lands in exactly one bucket (prior-art lesson 1 — the intersection in the
contract, the rest in types):

- **CORE** — portable, every adapter honors it identically; conformance-tested.
- **CAP** — typed optional capability: an adapter that cannot honor it does not compile against
  it (or reports it absent at runtime where availability is a runtime fact — Play-Services
  Cronet, package identity, OPFS); never a silent no-op.
- **CFG** — adapter/composition-root configuration; the core never sees it.
- **OUT** — excluded: no honest portable semantics exist.

## 4. The matrix

| # | Dimension | Class | The one-line reason |
|---|---|---|---|
| 1 | Method, URL, typed headers | CORE | Everywhere; reserved headers are adapter-owned (§5.1) |
| 2 | Body: `Bytes` \| `File` \| `Multipart` | CORE | File/Blob bodies stream from disk on all five surfaces (§5.2) |
| 3 | Streaming request bodies | OUT | Chromium-only on web; unchanged verdict, stronger evidence |
| 4 | One total deadline | CORE | The only timeout all five honor; classification rule in §7 |
| 5 | Fine timeouts (connect/read/write) | CFG | Client-wide where they exist at all — not per-request (§5.4) |
| 6 | Redirects: auto-follow, final URL, count | CORE | Hop *interception* impossible on web/.NET/background |
| 7 | Redirect hop trace | CAP | OkHttp/Cronet/reqwest yes; fetch/.NET/background never |
| 8 | Cookie-less, cache-less default | CORE | Confirmed; defaults conflict per platform, so the contract picks |
| 9 | Conditional requests (ETag/304 app-owned) | CORE | New result: portable, incl. web via `no-store` + raw 304 (§5.6) |
| 10 | HTTPS-only; cleartext dev-gated | CORE | Unchanged; plus new local-network permission error keys (§5.15) |
| 11 | Negotiated version observable | CORE (as `Option`) | Native yes; web only same-origin/TAO via Resource Timing |
| 12 | Priority hint | CORE (hint) | New: tri-engine web, Apple (RFC 9218 on wire, FLAGGED), Cronet/HttpEngine; legally ignored by OkHttp/.NET (§5.8) |
| 13 | Download progress (total = `Option`) | CORE | Portable, but totals lie under compression — total is always optional (§5.9) |
| 14 | Upload progress | CAP | XHR-only on web; DIY-and-dishonest on OkHttp/.NET (§5.9) |
| 15 | Response body sink: `Memory` \| `File` | CORE | New: OPFS `createWritable` made download-to-file portable (§5.10) |
| 16 | Response streaming (chunked delivery) | CORE, gated | Platform-portable everywhere; the remaining gate is FFI mechanism at boltffi ≥0.27.5 (§5.11) |
| 17 | Decoded bodies; `content_length` advisory | CORE | Adapters must normalize (gzip/brotli/zstd transport-owned) (§5.12) |
| 18 | Metrics (phase timings, TLS detail) | CAP (tiered) | Rich Apple/.NET/OkHttp; coarse Linux; TAO-gated web (§5.13) |
| 19 | Pinning (declarative SPKI) | CAP | All four native (with per-adapter work); impossible on web (§5.14) |
| 20 | Errors as typed keys | CORE | Taxonomy grows: permission-denied, cancelled-vs-timeout (§5.15) |
| 21 | Cancellation of in-flight effects | CORE | Everywhere; pause/resume of foreground calls exists nowhere (§5.16) |
| 22 | Retry | split | Connection-level recovery = CFG; request-level retry = the core's job (§5.17) |
| 23 | Auth: 401/407 as data; ambient OS auth | CORE / CFG | Challenge callbacks unportable; NTLM/Negotiate impossible on Android/web (§5.18) |
| 24 | Client certificates | CFG | Native-only, composition-root concern; absent web |
| 25 | Proxy, trust roots | CFG | Unchanged; Linux = env-vars-only asterisk (§5.19) |
| 26 | Cookies as values (capability) | §9-open | Evidence gathered, shape still a design session away (§5.20) |
| 27 | Trailers, 1xx/103, server push | OUT | Web floor: trailers removed from fetch spec; 103 script-invisible |
| 28 | WebSocket | parked family | §2 — honest contract needs its own design pass |
| 29 | Enterprise auth (NTLM/Kerberos), WPAD | OUT | Windows/Apple-only; Android has no built-in NTLM, web none |
| 30 | Background transfer | separate family | Sharpened, still §9 (§6) |

## 5. Dimension notes — the evidence behind each row

### 5.1 Headers (row 1)
The web silently drops forbidden headers (`Accept-Encoding` confirmed by probe in all three
engines — set it and it vanishes; `Cookie`, `Host`, `Origin` likewise). The contract therefore
declares a **reserved-header list the adapter owns**: `Accept-Encoding`, `Cookie`, `Host`,
`Content-Length`, connection-management headers. Core-set reserved headers are a **type error,
not a runtime drop**. `Authorization` is settable everywhere (probe-confirmed surviving on
web), with one portable rule: it is stripped on cross-origin redirects (now consistent across
all three engines per spec; .NET clears it on redirect too — so the rule is *free*).

### 5.2 Bodies (row 2)
`Bytes | File | Multipart{parts: Bytes|File}`. Evidence that `File` is honestly portable: web
FormData/Blob bodies compute `Content-Length` upfront from Blob metadata and stream file parts
from disk at transmission time (probed against an echo server, all engines; FLAGGED as
consensus-not-normative on the memory claim); Apple `uploadTask(fromFile:)`; OkHttp file
`RequestBody`; .NET stream content; reqwest file streams. Multipart is first-class only on
OkHttp (`MultipartBody`) and web (`FormData`) — Apple and .NET adapters construct the body
manually, which is fine because the **boundary string must come from the core anyway**
(derived from the effect id — deterministic, replayable; an adapter-generated random boundary
would make the recorded input stream non-reproducible). On web, `File` means an OPFS/Blob
handle, not a path — see §5.10's `FileRef` note.

### 5.3 Streaming request bodies (row 3)
Unchanged OUT, harder evidence: Chromium-only (105+, h2+, half-duplex, CORS preflight always);
Firefox meta-bug unresourced; WebKit accepts-but-never-sends (probed: `duplex` getter absent in
Firefox 152/WebKit 26.5). From Rust it is worse — web-sys has no `duplex` binding. The portable
"large upload" primitives are `File` bodies (§5.2) and the background family (§6).

### 5.4 Timeouts (rows 4–5)
The deadline-only core survives contact with all new evidence, and the *capability* framing of
fine timeouts from the earlier study is **downgraded to CFG**: per-request connect/read
timeouts are not honestly expressible anywhere — reqwest's `connect_timeout`/`read_timeout`
are client-wide (only the total is per-request); .NET's `ConnectTimeout` is handler-wide and
its 100 s default timeout **silently stops governing the body once you stream
(`ResponseHeadersRead`)** — the streamed-read timeout hole is real (runtime#36822, FLAGGED
GitHub-only) and the adapter must synthesize per-read deadlines with re-armed cancellation;
Apple has idle+total with the documented interaction that `waitsForConnectivity` suspends the
idle timer but not the total (SDK-header-verified). So: **deadline per request in the core;
everything finer is adapter construction detail** configured at the composition root, with the
conformance suite pinning observable behavior (a stalled server must produce `timeout` before
deadline+ε on every adapter, however the adapter achieves it).

### 5.5 Redirects (rows 6–7)
Core: follow, report final URL + hop count; **https→http is never followed** (this is .NET's
enforced behavior on modern .NET, the browsers' effective posture, and trivially enforceable
in the other adapters — adopting it as the contract rule makes .NET's constraint everyone's
guarantee). Hop trace stays a capability: reqwest's custom policy sees the full chain per hop
(sync closure — so the capability is a *recorded trace*, not an async veto), OkHttp network
interceptors see each wire hop, Cronet requires explicit consent per hop; fetch never, .NET
only by disabling auto-redirect and looping, background never. WinRT has no redirect-count
knob at all ("set internally by the system") — one more reason the C# adapter is .NET, not
WinRT.

### 5.6 Cookies, cache, conditional requests (rows 8–9)
Cookie-less/cache-less default confirmed from both directions (URLSession both-on vs
OkHttp/.NET both-off vs browser-owned). The new result is row 9: **app-owned conditional
requests are portable**. `If-None-Match`/`If-Modified-Since` are settable on every surface
including web, and with `cache: 'no-store'` the raw 304 (empty body) reaches script —
probe-confirmed; the browser-cache-replays-200 trap only exists when the browser's own cache
initiated the conditional. On Apple the adapter uses ephemeral/no-store configuration so the
URLCache never replays a 200 for a manual `If-None-Match`. Consequence: **an ETag-revalidation
facet flow needs no cache capability at all** — 304 is just a typed response. Adapter rule:
foreground requests run cache-disabled (`no-store` / ephemeral / no `Cache` configured / .NET
has no cache anyway), so protocol caching never silently changes replay behavior.

### 5.7 Version observability (row 11)
`HttpVersion` becomes `Option<HttpVersion>` in the response: native adapters always report it
(URLSession `networkProtocolName`, OkHttp `Response.protocol`, .NET `Version`, reqwest
`version()`); the web adapter reports it only when Resource Timing yields `nextHopProtocol`
(same-origin or TAO-blessed; probed working in all three engines, `""` cross-origin without
TAO). HTTP/3 remains a hint nowhere promised: OkHttp caps at h2 (HttpEngine covers h3 on
API 34+), Windows 11+ only with silent fallback, reqwest's h3 still behind `reqwest_unstable`
(quinn), browsers invisible.

### 5.8 Priority (row 12)
**New CORE-as-hint proposal.** A three-level hint (`low | normal | high`) on the request
effect: honored by fetch `priority` (tri-engine since Firefox 132/Safari 17.2),
Cronet/HttpEngine (five levels), Apple `URLSessionTask.priority` (0–1 float; empirically emits
RFC 9218 `Priority: u=N` on the wire — FLAGGED observed-not-documented, so the *wire mapping*
stays out of the contract); legally ignored by OkHttp (no API, FIFO dispatcher) and .NET
(closed-wontfix upstream). "Hint" means: never conformance-tested for effect, only for
acceptance. Anything stronger (ordering guarantees) belongs to the core's own effect
scheduling, which is where .NET's wontfix pushes it anyway.

### 5.9 Progress (rows 13–14)
Download progress is CORE with contract-defined byte semantics: **bytes are as observed by the
adapter after transport decoding; the total is always `Option`; counters may restart** (the
platforms force all three: OkHttp's transparent gzip strips `Content-Length`; web chunk counts
are decoded bytes against an encoded `Content-Length` denominator; Windows' two APIs disagree
on whether bytes include headers and both may regress on restart). Upload progress stays CAP:
web = XHR only (probed: still no fetch upload progress anywhere, July 2026; `FetchObserver`
never shipped); OkHttp/.NET measure buffer hand-off, not wire bytes (the OkHttp recipe wraps
the sink; naïve .NET wrappers jump to 100%); only Apple (`didSendBodyData`) and
BackgroundTransfer give OS-fed figures. The capability's contract text must say "indicative,
monotone per attempt, not wire-truth".

### 5.10 Download-to-file (row 15)
**New CORE row — the floor moved.** Every surface can now sink a response to a file without
buffering it in memory: Apple `downloadTask` (temp file, move-synchronously rule), OkHttp
sink-to-file, .NET stream copy, reqwest `bytes_stream` to disk, and on web
`response.body.pipeTo(await opfsHandle.createWritable())` — Baseline since Safari 26.0
(Sept 2025), probe-confirmed in all three engines. The catch that shapes the contract type:
on web the destination is **origin-private storage, not a user-visible path** (pickers are
Chromium-only, Mozilla formally negative). So the contract's file type is an opaque
**`FileRef`** the shell resolves (path on native, OPFS handle on web) — which is the same
abstraction the background family needs anyway, and it keeps filesystem semantics out of the
sans-io core.

### 5.11 Response streaming (row 16)
Platform-side: portable, full stop (AsyncBytes/delegate on Apple with a FLAGGED
no-flush-guarantee caveat for latency-critical SSE; OkHttp source streams; .NET
`ResponseHeadersRead`; reqwest `bytes_stream`; web ReadableStream). FFI-side: triage T1 found
both step-02 probes' stream machinery converges at boltffi 0.27.5, which clears the old kill
criterion — but the *mechanism* (callback-trait push vs wake-and-read vs ffi_stream) was
explicitly deferred by the stall report ("decide there, not here"). Proposal: response
streaming enters the portable core **conditioned on one spike probe** (S-FFI in
[spike-plan.md](spike-plan.md)) re-running the stream shapes at ≥0.27.5 inside the http
round-trip, choosing the mechanism on measurements. If it stalls again, row 16 falls back to
`Memory | File` sinks only — which, note, already cover most facet needs including SSE-via-File
never and SSE-via-chunks gone; that fallback would park SSE with WebSocket.

### 5.12 Compression (row 17)
Adapters normalize: .NET must set `DecompressionMethods.All` (default is **None**); OkHttp
default transparent gzip is kept but the adapter must surface `content_length = None` honestly
(gzip strips it) and add the brotli/zstd modules; Apple sends `gzip, deflate, br` (empirical;
doc-silent, FLAGGED) and cannot disable decoding except by owning `Accept-Encoding`; browsers
are opaque (zstd now cross-engine as a transfer detail). Contract: bodies are always decoded;
`content_length` advisory `Option`; no "raw body" promise exists (web cannot).

### 5.13 Metrics (row 18)
Tiered capability, corrected from the earlier study: **Tier A** (phase timings + TLS detail):
Apple TaskMetrics, .NET 8+ meters/OTel spans (connection-level spans experimental), OkHttp
EventListener (timing only, no TLS metadata, events repeat under retries — and OkHttp 5's
default Happy-Eyeballs `fastFallback` makes "connect time" per-*attempt*, races included).
**Tier B** (whole-request only): Linux/reqwest (no phase API; `connector_layer` self-timing at
best), WinRT/BackgroundTransfer (nothing). **Tier C** (optional, TAO-gated): web Resource
Timing (full phases + `nextHopProtocol` same-origin; zeros cross-origin without TAO;
per-field engine gaps — Safari lacks `responseStatus`, FLAGGED-fresh fields from June 2026
Baseline). The capability type should expose tier, not pretend uniformity.

### 5.14 Pinning (row 19)
Declarative SPKI data, per-adapter mapping — now with the Android answer (§1.4): the adapter
carries pins into `CertificatePinner` (OkHttp) / `addPublicKeyPins` (HttpEngine/Cronet); NSC
`<pin-set>` is defense-in-depth only (Conscrypt-enforced, custom-TrustManager-fragile,
Cronet-undocumented, websocket-blind). New Linux cost surfaced: reqwest has **no SPKI hook** —
CA-level `tls_certs_only()` is easy, leaf/SPKI pinning requires `tls_backend_preconfigured`
with a hand-built rustls verifier; feasible, but it is real adapter code the conformance suite
must cover (spike probe S-LX2). Web: impossible, capability absent (unchanged). One more
Android landmine for the suite: with per-domain NSC configs present, the platform's
`RootTrustManager` *throws* on hostname-less trust checks — the adapter must never route
through a plain 2-arg `checkServerTrusted` path.

### 5.15 Error taxonomy (row 20)
Typed keys, growing three entries from the research: **`PermissionDenied`** — Chrome 142's
Local Network Access prompt (fetches to private IPs now fail on user denial) and Android 16→17
local-network permission (`EPERM` on LAN HTTP for targeting-17 apps) make "the platform asked
the user and the user said no" a first-class outcome, distinct from network failure;
**timeout vs cancel** — one key each, with the conformance rule that the web adapter
classifies via `signal.reason`, never the rejection's `.name` (probed: WebKit 26.5 rejects
with `AbortError` even on timeout); **`QuotaExceeded`** reserved for the background family
(Windows' 200-op queue, Android quotas). The RN-2026 lesson from prior-art stands: the
native-failure → key mapping is conformance-tested per adapter, never judgment.

### 5.16 Cancellation (row 21)
CORE: any in-flight effect is cancellable; completion arrives as the `Cancelled` typed input
(one effect, one completion, always — cancellation is not a silent drop). Pause/resume of
foreground calls exists on no platform (OkHttp/Cronet/fetch/reqwest: confirmed none) — OUT.
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
task-level for Basic/Digest, a documented trap; web dialogs unsuppressible for credentialed
same-origin fetches). Ambient OS auth (Kerberos/NTLM via logged-in identity) is CFG on
Windows/Apple, impossible on Android (no built-in NTLM; Digest is a FLAGGED third-party crate
situation on both Android and Rust) and web — hence row 29 OUT for the contract. Client certs:
CFG at the composition root (Apple `SecIdentity`, .NET `SslOptions`, OkHttp
KeyManager+KeyChain user grant, reqwest `identity()`); absent on web and on Cronet (FLAGGED
by-absence).

### 5.19 Proxy and trust (row 25)
Unchanged CFG, with the Linux asterisk now source-verified: reqwest 0.13's default
`system-proxy` reads the Windows registry and macOS SCDynamicStore but is **env-vars-only on
Linux** — no gsettings, no kioslaverc, no PAC, no portal (the XDG ProxyResolver portal exists
but only GLib consumes it). A GNOME/KDE user's GUI proxy settings are invisible to the Linux
adapter unless exported as env vars; `Proxy::custom` is the seam if this ever matters.
Document as adapter behavior; do not promise "system proxy" on Linux.

### 5.20 Cookies as values (row 26 — still §9)
Evidence gathered for the eventual design session: per-request participation is expressible
everywhere (web `credentials`, Apple `httpShouldHandleCookies`, OkHttp jar choice, .NET
`CookieContainer` per handler); cookie *values* are readable everywhere except web (browser-
owned, script-invisible; CHIPS partitioning is the surviving cross-site model now that
third-party-cookie phase-out is dead). Any capability shape must make the web adapter's
"participate but never read" mode a type, not a runtime surprise. Shape stays open.

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
   force-discretionary-when-backgrounded; Linux/web have nothing (the web's Background
   Fetch/fetchLater are Chromium-only enhancements). So the family's capability surface is
   `availability() -> {Available, NeedsForeground, NoIdentity, Unsupported, …}` — queried, not
   assumed, and the *scheduling precondition* is part of the type.
3. **Android's adapter is a three-way dispatch** (UIDT on 34+ / WorkManager / DownloadManager
   for plain downloads), all implementing the iOS-shaped contract; DownloadManager transfers
   survive even force-stop (separate system process; FLAGGED undocumented) but completion
   *broadcasts* to a stopped app do not — reattach-on-launch is the portable ceremony on every
   platform (Apple identifier re-attach, Windows `GetCurrentDownloads/UploadsAsync` +
   `AttachAsync`, Android query-on-start).
4. **Progress byte semantics must be self-defined** (§5.9's rule doubly so here: Windows'
   BackgroundTransfer excludes headers where WinRT foreground includes them; both regress on
   restart; `HasRestarted` is a field, not an anomaly).

## 7. Conformance rules the research forces

Rules that go into the suite's fixed rows regardless of final contract shape:

1. Same request ⇒ same typed response/error on every adapter (unchanged foundation).
2. Timeout-vs-cancel classified identically everywhere; web via `signal.reason`.
3. A stalled-body server yields `timeout` ≤ deadline+ε on every adapter (kills the .NET
   streamed-read hole and any idle-timer surprise).
4. https→http redirect: refused identically everywhere.
5. Manual `If-None-Match` yields a real 304 (not a cache-replayed 200) on every adapter.
6. Reserved headers: core-set is a compile error; adapter never silently drops a permitted one.
7. Decoded-body invariant: a gzip/brotli response yields identical bytes + `content_length:
   None`-or-honest on every adapter.
8. One effect, one completion: mid-flight failure surfaces the typed error — no hidden
   request-level retry (positive control required).
9. Cancellation always completes the effect with `Cancelled` (never silence).
10. Pin mismatch ⇒ the same typed pinning error on all four native adapters; the capability is
    absent-at-compile-time on web. Android additionally: pinning survives a custom
    `TrustManager` being absent — i.e., the suite tests the *adapter's* pins, never NSC's.

## 8. Still open after this round

- **FFI streaming mechanism** (§5.11) — the one gate on row 16; spike S-FFI decides.
- **Cookie capability shape** (§5.20) — design session, when a feature needs it.
- **WebSocket family** (§2) — protected possibility, undesigned.
- **`FileRef`** (§5.10) — the opaque file abstraction is shared with the background family and
  possibly with draft stash; its home (bolted-core? bolted-http?) is a structural question for
  a design session, not this doc.
- **Background family full contract** (§6) — unchanged §9 status, better-informed.
- Whether the priority hint (§5.8) survives Henrik's review as CORE or demotes to CAP.
