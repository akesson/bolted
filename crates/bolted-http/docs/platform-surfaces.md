# bolted-http — the native API surfaces the adapters must map

**Status:** design study, 2026-07-09. Input to the `bolted-http` contract design (which happens
after spike steps 02–03). Companion doc: [prior-art.md](prior-art.md).

**Method:** inventory of official documentation (developer.apple.com, developer.android.com,
learn.microsoft.com, MDN, curl.se, libsoup/GLib docs). Claims that could not be verified
against a fetched official page are flagged inline and collected in
[§8](#8-verification-notes) as spike candidates. The browser section is included beyond the
asked win/lin/mac/android/ios set because the Rust-web target must implement the same contract
with zero FFI — it is the weakest adapter and therefore shapes the contract's floor.

---

## 1. The surfaces at a glance

| | Apple (iOS/macOS) | Android | Windows | Linux | Web (WASM) |
|---|---|---|---|---|---|
| Native stack | URLSession (the only one) | OkHttp / Cronet / HttpsURLConnection (choice) | WinHTTP / WinRT / WinINet (+ own stack) | libcurl / libsoup3 / own stack | `fetch` (the only one) |
| HTTP/3 | yes (system stack) | only via Cronet | Win 11 / Server 2022+ | via libcurl builds | browser-negotiated, invisible |
| OS-managed background transfer | yes — up+down, app relaunch | downloads only (DownloadManager) | yes — up+down (packaged apps); BITS otherwise | **none** | none portable (Background Fetch = Chromium, experimental) |
| App code runs during bg transfer | **never** | yes (WorkManager) / no (DownloadManager) | no | always (it's your process) | no |
| Pinning | delegate (SecTrust) + declarative plist | CertificatePinner + declarative XML | cert-context inspection / WinRT event | you own the verifier | **impossible** |
| Cookies default | on (3rd-party blocked) | OkHttp: **off** | WinHTTP: session-only; WinRT: managed jar | per-library | browser-owned, script-invisible |
| Cache default | on (protocol policy) | OkHttp: **off** | WinHTTP: none; WinRT: yes | per-library | browser-owned + Cache API |
| Timeout vocabulary | idle (60 s) + total (7 d) | connect/read/write (10 s) + call (off) | per-phase knobs; bg fixed 5 min/2 min | per-library | AbortSignal only |
| Proxy | OS-managed (+ per-session config) | ProxySelector / system | WPAD/PAC APIs, per-interface failover | env vars vs gsettings vs kioslaverc — no single truth | invisible |
| Cleartext HTTP | blocked (ATS) | blocked since API 28 | allowed | allowed | mixed-content blocked |

Two structural facts fall out immediately:

1. **The background-transfer models are mutually incompatible in kind**, not just in detail —
   who executes the transfer, whether app code can run, what payloads are legal, and what
   survives termination differ per platform (§7.1).
2. **The web target is the contract's floor**: no pinning, no proxy, no cookie access, no
   upload progress in fetch, no portable background anything. Whatever the portable core of
   `Http` promises must be implementable there.

## 2. Apple (iOS + macOS) — URLSession

### Model

One stack, three session kinds: `.default` (persists cookies/cache/credentials), `.ephemeral`
(nothing touches disk), `.background(withIdentifier:)` (transfers run in a separate system
process, `nsurlsessiond`). Task types: **data** (in-memory bodies), **upload** (data/file/
stream), **download** (straight to disk, resumable via resume data), **stream** (raw TCP/TLS),
**websocket** (first-class, shares session config/delegates/metrics). Invocation via
completion handlers, delegates (incremental data, auth, redirects, metrics), or Swift async
(`data(for:)`, `bytes(from:)` as an AsyncSequence). The session holds a strong reference to
its delegate until invalidation — a documented leak hazard the adapter must own.

### Contract-relevant configuration

- **Timeouts — only two, and neither is a connect timeout**:
  `timeoutIntervalForRequest` is an **idle timer** (default 60 s, reset whenever data
  arrives); `timeoutIntervalForResource` is the **total wall-clock cap** (default 604 800 s =
  7 days). There is no per-request connect timeout — the exact gap Ktor documents for its
  Darwin engine.
- `waitsForConnectivity`: queue until a network exists instead of failing (connection
  establishment only; mid-transfer loss still errors). Ignored by background sessions — they
  always wait.
- Network gates: `allowsCellularAccess`, `allowsExpensiveNetworkAccess`,
  `allowsConstrainedNetworkAccess` (Low Data Mode) — request-level policy with matching
  per-transaction metrics.
- Multipath TCP (`multipathServiceType`) — entitlement-gated, iOS-family only, not macOS.
- Cookies: pluggable `HTTPCookieStorage`; accept policy defaults to
  `.onlyFromMainDocumentDomain` (third-party rejected). Cache: pluggable `URLCache`,
  protocol-driven policy by default. `httpMaximumConnectionsPerHost` default 6.
- HTTP/3: on by default via Alt-Svc discovery; `URLRequest.assumesHTTP3Capable` races QUIC
  without a prior request.
- `protocolClasses` (`URLProtocol`): full in-process interception — the heavyweight escape
  hatch; there is no OkHttp-style interceptor chain.
- Extras with no cross-platform analog: `enablesEarlyData` (TLS 1.3 0-RTT),
  `requiresDNSSECValidation`, `proxyConfigurations` (iOS 17+).

### Auth / TLS / pinning

Challenges arrive on the delegate at two tiers: session-level (server trust — the **pinning
hook**: evaluate `SecTrust`, answer `.useCredential` or `.cancelAuthenticationChallenge`) and
task-level (Basic/Digest/client certs). Declarative alternative: `NSPinnedDomains` in
Info.plist — pinning with no code at all. **App Transport Security** blocks cleartext and
sub-par TLS for the URL Loading System; exceptions are static Info.plist keys — build-time
facts, which suits Bolted's rung-3 checks (a `bolted-check` could diff declared exceptions).

### Background sessions — the crown jewel and the trap

- The OS transfers on the app's behalf; the app may be suspended or **terminated** and is
  relaunched when transfers finish. No app code runs during the transfer.
- **Restrictions**: HTTP(S) download tasks and upload-**from-file** tasks only — no data
  tasks, no in-memory or stream bodies. Delegate-only (no completion handlers). Redirects are
  **always auto-followed** — `willPerformHTTPRedirection` is never called.
- **Identifier ceremony**: fixed string identifier; recreating a session with the same
  identifier re-attaches to in-flight transfers. Relaunch delivery via
  `application(_:handleEventsForBackgroundURLSession:completionHandler:)` — which exists on
  UIKit platforms but **not on macOS AppKit** (verified availability).
- **Scheduling**: `isDiscretionary` lets the system defer for power/Wi-Fi — and transfers
  started while the app is backgrounded are **forced discretionary regardless**. The OS
  applies a documented **escalating rate limiter** to background relaunches (resets when the
  user foregrounds the app); the documented pattern is batching many tasks per wake.
- **Termination semantics**: system-initiated termination → transfers continue, app relaunched.
  **User force-quit → all background transfers cancelled**, no relaunch.
- File delivery: the downloaded temp file is valid **only until the delegate method returns** —
  must be moved synchronously.

### Observability

`URLSessionTaskMetrics` is the richest telemetry of any surface: per-transaction DNS/connect/
TLS/request/response timing, byte counts before/after encoding, negotiated TLS version and
cipher suite, `networkProtocolName` (h1/h2/h3), DNS resolution protocol (DoH visibility),
`isProxyConnection` (detects iCloud Private Relay), `isCellular`/`isExpensive`/`isConstrained`,
cache-vs-network fetch type.

### Only Apple / Apple forbids

**Only Apple:** OS-executed background *uploads and* downloads with relaunch; HTTP/3 in the
system stack; multipath TCP; constrained/expensive network semantics; `waitsForConnectivity`;
metrics of that depth; declarative plist pinning; websocket as a sibling task type.

**Apple forbids:** in-memory/background data tasks; redirect interception in background;
surviving user force-quit; runtime ATS exceptions (build-time only); a connect-timeout knob;
an interceptor chain.

## 3. Android

### OkHttp (the de-facto application stack)

- One shared `OkHttpClient` per app (documented guidance — it owns the connection pool and
  thread pools). Immutable request/response values, sync and async calls, streaming bodies.
- **Protocols: HTTP/1.1 and HTTP/2 only. No HTTP/3/QUIC** (verified against the 5.x protocol
  list). HTTP/3 on Android means adopting Cronet.
- **Interceptors** — the platform's signature feature, with two distinct hook points:
  *application* interceptors (once per logical call, cache hits included, can short-circuit
  and adjust per-call timeouts) and *network* interceptors (once per wire request, see
  redirects/retries individually). No URLSession equivalent exists.
- **Four timeout kinds**: `connectTimeout`/`readTimeout`/`writeTimeout` (defaults 10 s each)
  plus `callTimeout` (whole-call deadline, **off by default**).
- Silent recovery is default-on: `retryOnConnectionFailure` retries alternate IPs, stale
  pooled connections, and falls through proxy lists.
- **Cookies and cache are OFF by default** (`CookieJar.NO_COOKIES`; cache opt-in with
  directory+size) — the exact opposite of URLSession's defaults.
- TLS: `ConnectionSpec` tiers (RESTRICTED/MODERN/COMPATIBLE/CLEARTEXT; contents shift per
  OkHttp release — a contract must not hardcode cipher expectations), `CertificatePinner`
  (SHA-256 SPKI pins per host pattern), swappable trust manager for private CAs.
- Observability: `EventListener` (DNS/connect/TLS/headers/body phase events, repeated under
  retries) — timing yes, but no TLS-suite/DoH/private-relay metadata à la Apple.

### Cronet (Chromium stack as a library)

HTTP/3 + Brotli + request prioritization + disk cache; ships primarily via **Google Play
Services** (absent on non-GMS devices; the documented fallback is a "less performant"
pure-Java engine), embeddable at binary-size cost (size undocumented officially). Callback
model with **explicit redirect consent** (`onRedirectReceived` → `followRedirect()` or
cancel). Cronet dropped iOS (M108) — HTTP/3 on Apple is URLSession's job.

### Background transfer — the model inversion

**Key architectural fact: Android has no general equivalent of iOS's "OS executes your
upload, no app code runs."** The choices are:

- **WorkManager** — the OS runs **your code** (`doWork()`) under declarative constraints
  (network type, charging, idle, storage), with persistence across reboot, chains, and
  exponential backoff. Periodic work floors at **15 minutes**; expedited work is
  quota-limited; work >10 min must become a foreground service and inherit dataSync limits.
- **DownloadManager** — the OS runs the transfer in a system process (survives app death,
  retries across reboots, built-in notification UI) but: **downloads only — there is no
  upload API whatsoever**, HTTP(S) only, and no TLS/pinning/auth/interception hooks of any
  kind (headers in, file out).
- **Foreground service (`dataSync`)** — Android 14 requires a declared type + Play Console
  declaration; **Android 15 (target 35) imposes a hard ~6 h per rolling 24 h budget** shared
  across the app's dataSync services, with `onTimeout` → seconds to stop → fatal exception,
  and bans BOOT_COMPLETED starts. Google's documented migration targets: WorkManager,
  user-initiated data-transfer jobs, DownloadManager.
- **Doze / App Standby**: network access is suspended entirely outside maintenance windows;
  idle apps degrade to roughly once-daily network. Only FCM high-priority, the
  battery-optimization allowlist (Play-policy-restricted), or a foreground service punch
  through.

Consequence for a sans-io core: on Android the core **can** be alive during a background
upload (WorkManager runs app code); on iOS it **cannot** (the OS runs the transfer). The
background-transfer capability must be designed for the iOS case — durable effect handed
over, completion arrives as an input to a possibly-new core instance — and Android merely
implements that contract with more freedom, never the reverse.

### Network Security Config (declarative XML)

Manifest-referenced: per-domain trust anchors, **pin-sets with expiration dates**
(a documented fail-open safety valve Apple has no analog for), cleartext opt-in/out
(cleartext **blocked by default since API 28**; user CAs untrusted since API 24),
`debug-overrides` for dev CAs. **Scope caveat flagged for the spike:** the official page does
not promise that `<pin-set>` binds OkHttp — and especially not Cronet; library compliance
"depends on library implementation". Declarative pinning on Android cannot be assumed
portable across stacks without verification.

### Only Android / Android forbids

**Only Android:** interceptor chains; constraint-scheduled execution of arbitrary app code
with reboot persistence; four independent timeout knobs; DownloadManager's zero-code
download UX; per-request priorities + QUIC via Cronet; runtime-swappable trust; pin
expiration dates; a *choice* of network stack.

**Android forbids:** OS-managed background **uploads**; cleartext by default; network during
Doze; dataSync beyond the 6 h budget (Android 15); unmetered-timing guarantees (nothing is
exact); HTTP/3 without Cronet.

## 4. Windows

### Foreground stacks

- **WinHTTP** (C API, `winhttp.dll`): "server-based scenarios… system services and HTTP-based
  client applications"; async strongly recommended; **HTTP/1.1 by default — HTTP/2/3 are
  per-request opt-in flags** (and HTTP/1.1 can't be disabled); HTTP/3 needs Windows 11/Server
  2022 (flagged: Q&A source). WPAD/PAC is a first-class API (`WinHttpGetProxyForUrl`, per-
  interface proxy failover with `AUTOMATIC_PROXY`). SChannel integration both directions:
  client certs picked from Windows cert stores, server cert exposed as a context — the
  **pinning hook**. No persistent cookies, **no cache at all**, decompression off by default,
  no SOCKS. Telemetry is unusually deep (request times/stats, connection stats, QUIC stats).
  Enterprise auth (Basic/Digest/NTLM/Negotiate/Kerberos) built in for server and proxy.
- **WinINet**: interactive/IE-era client; may show credential UI; explicitly unsupported in
  services. Its distinguishing features are all shared-with-IE state (cookie jar, cache,
  zones). Legacy; new code is routed to WinHTTP or WinRT.
- **WinRT `Windows.Web.Http.HttpClient`**: async-only, **filter chain = real interceptor
  architecture** (each filter wraps an inner `IHttpFilter` — OkHttp-shaped), HTTP cache +
  inspectable `HttpCookieManager`, `ServerCustomValidationRequested` event as the pinning
  hook, and — verified — **usable from unpackaged Win32 desktop apps** (it requires neither
  package identity nor UWP UI context).
- **What .NET does**: `SocketsHttpHandler` — fully managed own stack, chosen for performance,
  the elimination of libcurl, and "consistent behavior across all .NET platforms". The
  desktop world's own-stack precedent (prior-art §3e).

### Background transfer — two OS services

- **`Windows.Networking.BackgroundTransfer`** (the iOS analog): OS-managed uploads *and*
  downloads that "persist beyond app termination", Data-Sense/Battery-Sense aware, cost
  policies (`UnrestrictedOnly`/`Default`/`Always` vs metered/roaming state), pause/resume,
  toast/tile completion notifications, and `BackgroundTransferCompletionGroup` — the OS runs
  **your background task when transfers finish even if the app isn't running** (closer to
  Android's WorkManager-on-completion than iOS's relaunch, and richer than both).
  **Constraints:** requires **package identity — unpackaged apps are locked out** (MSIX
  only); ≤200 concurrent operations per app ("exceeding that limit may leave the app's
  transfer queue in an unrecoverable state"); mandatory reattach ceremony on every launch
  (`GetCurrentDownloadsAsync`/`GetCurrentUploadsAsync` + `AttachAsync` — "not doing this will
  cause the leak of already-completed transfers"); **fixed** timeouts (5 min connect, 2 min
  response, 3 automatic retries — not configurable).
- **BITS**: COM job model for services and unpackaged software; survives logoff and
  **reboot** (resumes at next logon); idle-bandwidth throttling that yields to interactive
  traffic; files invisible until `Complete()`; jobs auto-cancelled after 90 days idle;
  single-file upload jobs; PowerShell module for admin scripting; Windows Update runs on it.
  Microsoft's split guidance: packaged apps → BackgroundTransfer, everything else → BITS.

### Only Windows / Windows forbids

**Only Windows:** *two* OS transfer services, one surviving reboot; OS-evaluated WPAD/PAC as
a reusable API; cert-store integration for client-cert *selection* (issuer-list filtering);
Kerberos/NTLM everywhere; completion-group background tasks; toast/tile notifications wired
into transfers; QUIC statistics at the API level.

**Windows forbids:** BackgroundTransfer without package identity; >200 queued transfers;
configurable background timeouts; WinHTTP cookies/cache; WinINet in services; HTTP/3 below
Windows 11.

## 5. Linux

- **No OS transfer service — confirmed by absence.** Nothing in freedesktop portals or
  systemd takes a transfer job and runs it on the app's behalf. "Survives app termination"
  on Linux means *you* spawn a process (systemd user units being the composable substrate;
  they survive the launching app, and logout only with lingering enabled).
- **libcurl**: the universal C ABI — easy (blocking) and multi (non-blocking, epoll-
  integrable, "beyond thousands of simultaneous transfers") interfaces; env-var proxies with
  the classic quirk (`http_proxy` honored **only lowercase**); TLS backend chosen per distro
  build, so cert behavior varies with the package, not your code.
- **libsoup3**: the GNOME-integrated stack — system proxy via `GProxyResolver`
  (libproxy/GNOME-settings/Flatpak-portal implementations in glib-networking), TLS via
  glib-networking, cookie jars with persistence, file-based HTTP cache, `SoupSessionFeature`
  as an interceptor mechanism, WebSockets.
- **The proxy mess is structural**: env vars, GNOME gsettings (`org.gnome.system.proxy`,
  including PAC via `autoconfig-url`), KDE's `kioslaverc` (flagged: community sources), and
  libproxy trying to unify them. curl-based apps see env vars; GLib apps see GNOME/portal
  settings; each toolkit picks its own. "System proxy" on Linux is a **policy decision the
  adapter must make and document**, not a lookup.
- **Cert stores**: Debian-family `/etc/ssl/certs` + `update-ca-certificates`; Fedora/RHEL
  p11-kit trust (flagged); `SSL_CERT_FILE`/`SSL_CERT_DIR` overrides; `rustls-native-certs` +
  `openssl-probe` are the pragmatic Rust answer.
- **Own stack is idiomatic**: no stable system HTTP ABI besides libcurl, distro variance in
  curl builds, static-linking culture — reqwest/hyper + rustls + native cert discovery is
  the norm (the doc-verifiable anchor being Microsoft's identical SocketsHttpHandler
  rationale). For bolted, this makes Linux the one platform where the *portable* adapter is
  the native choice.
- **Sandboxes**: Flatpak network is all-or-nothing (`--share=network`, with the documented
  abstract-socket caveat); snap's `network` interface typically auto-connects.

**Only Linux:** total freedom — you own the verifier, the proxy policy, every byte.
**Linux forbids nothing — and provides nothing**: no OS transfers, no cost policies, no
single proxy truth, no canonical cert path. Freedom as an unfunded mandate.

## 6. Web / WASM (`fetch`) — the contract's floor

Included beyond the asked platform set because the Rust-web shell consumes the same core
with zero FFI; every portable contract promise must survive this adapter.

- **Model**: promise-based; `Response` resolves at headers; HTTP error statuses do **not**
  reject. Works identically in workers and service workers.
- **Timeout**: none. `AbortSignal` is the only cancellation mechanism (`AbortSignal.timeout()`
  → `TimeoutError`, manual abort → `AbortError`; combine with `AbortSignal.any()`). A single
  total-deadline semantic is the only timeout the web adapter can honor.
- **Streaming**: response-body streaming is broadly supported (`ReadableStream`).
  **Request-body streaming is effectively Chromium-only** (105+), requires HTTP/2+, is
  half-duplex, always triggers CORS preflight, and Safari accepts the API but won't send it.
- **Redirects**: `follow` / `error` / `manual` (opaque) — intermediate responses are **never
  visible** and cannot be rewritten. No redirect interception, period.
- **Upload progress: fetch has none.** XHR's `upload.progress` events are the only
  mechanism — a web adapter that promises upload progress is an XHR adapter for those
  requests (XHR is unavailable in service workers).
- **CORS is an absolute boundary** with no application escape hatch; forbidden headers
  (`Cookie`, `Host`, `Origin`, `Referer`…) are browser-owned.
- **TLS/auth**: entirely browser-owned. **No pinning is possible** (HPKP removed from the
  platform; by-absence claim), no client-cert API, no proxy visibility.
- **Cookies**: browser-managed and script-invisible on the wire; the `credentials` option
  (`omit`/`same-origin`/`include`) is the only control.
- **Background**: Background Fetch API (SW-based, survives page close, browser progress UI)
  is **Chromium-only and experimental** — a progressive enhancement, never a portable
  capability. `keepalive: true` covers the page-unload case with a 64 KiB cap.
- Unique to the web: subresource-integrity enforcement in the request API (`integrity`),
  zero-config TLS/HSTS/CT, service-worker request interception + Cache API offline layer.

## 7. What the intersection permits — contract implications

### 7.1 Background transfer is four different machines

| | iOS | Android | Windows (packaged) | Linux / Web |
|---|---|---|---|---|
| Who executes | OS daemon | your code (WorkManager) or OS (downloads only) | OS service | you / nobody |
| Uploads | file-based only | app-code only | yes | you / no |
| Survives force-quit | **no** (cancelled) | WorkManager: yes | yes | n/a |
| Redirect/auth hooks during bg | none | full (own code) / none (DownloadManager) | none | full / none |
| Completion with app dead | relaunch + delegate rehydration | WorkManager reschedule / broadcast | background task invoked | no |
| Fixed OS limits | discretionary scheduling, escalating relaunch throttle | 6 h dataSync budget, Doze | 200 ops, 5 min/2 min timeouts | none |

The honest portable shape (matching prior-art lesson 3): a **separate, optional
`BackgroundTransfer` capability** whose contract is the *intersection of the OS-executed
cases* — durable, serializable, file-based transfer descriptors with stable identities;
handed over entirely (no per-chunk hooks, no redirect interception, no in-memory bodies);
completion delivered as an input to a possibly-new core instance; force-quit loss is legal
behavior. Android's extra freedom (core code may run) must not leak into the contract, or
iOS can't implement it. This is the same durable-effect precondition already noted in
ARCHITECTURE §9.

### 7.2 Timeouts: one portable knob, the rest capabilities

The only timeout every surface can honor is a **total deadline** (resource timeout /
callTimeout / AbortSignal / fixed OS budgets). Idle, connect, read, and write timeouts exist
on *some* stacks with *different* meanings — Apple has idle+total but no connect; OkHttp has
connect/read/write but total off by default; fetch has abort only; background transfer has
none configurable anywhere. Contract: `deadline` portable; finer-grained timeouts typed as
per-adapter capabilities. (This single issue produced the most prior-art bugs — see
prior-art §4.)

### 7.3 Pinning: declarative data, gated off the web

All four native platforms can implement SPKI-pin *data* (SecTrust evaluation /
CertificatePinner / cert-context inspection / own verifier); the web cannot, full stop.
Pinning is therefore a capability the web adapter simply does not provide — and the
declarative-XML route on Android needs a spike check (does `<pin-set>` bind OkHttp/Cronet?
undocumented). Callbacks are unportable everywhere (prior-art lesson 5).

### 7.4 Cookies and caching: pick an explicit default; the defaults conflict

URLSession: both on. OkHttp/WinHTTP: both off. Web: browser-owned, invisible. A contract
that is silent inherits a per-platform coin flip. The bolted-shaped answer: **the portable
request effect is cookie-less and cache-less by default** (matching a core that treats HTTP
as an effect, not ambient state); cookie/cache participation is opt-in per adapter, and on
the web the browser's ownership is accepted as-is (credentials mode, cache mode).

### 7.5 Redirects: observe-maybe, intercept-never

Foreground interception exists on URLSession (delegate), OkHttp (interceptors), Cronet
(explicit consent), WinHTTP (callback) — but **not on fetch**, and **not in any background
transfer**. Portable contract: redirects are followed by the stack; the final URL is
reported; hop-by-hop interception is a capability absent on web and in background.

### 7.6 Streaming and progress

Response streaming is portable (every surface). Request streaming is not (Chromium-only on
web). Upload progress is portable **only if** the web adapter drops to XHR. Download progress
is portable. Independent of platforms, whether streamed bodies can cross BoltFFI at all is
exactly the step-02 probe (stream burst/ordering semantics) — the platform ceiling is known;
the FFI ceiling is the open question.

### 7.7 Proxy and trust: never portable concepts

Proxy is OS-coherent on Windows/Apple, a four-way policy mess on Linux, invisible on web.
Trust evaluation is platform-delegated everywhere (and the Rust ecosystem converged on the
same — prior-art §3g). Neither appears in the portable contract; both are adapter
configuration.

### 7.8 Protocol versions and cleartext

The contract must not promise HTTP/3 (OkHttp caps at h2; Windows 10 caps at h2; web is
invisible) — version is an observable, not a request parameter. HTTPS-only is safe as the
portable rule: ATS, Android API 28+, and browsers already enforce it; cleartext is a
dev-only, platform-config-gated exception.

### 7.9 Observability

DNS/connect/TLS/first-byte/bytes timing is implementable on every native surface
(TaskMetrics / EventListener / WinHTTP request-times / curl probes) and **not on the web**
(fetch exposes nothing; the Resource Timing API gives coarse figures — unverified here).
Metrics: an optional capability, rich on native, absent-or-coarse on web.

## 8. Verification notes

Aggregated flags from research — candidates for spike verification rather than trust:

**Apple:** `waitsForConnectivity` default; `httpShouldSetCookies` default; default URLCache
sizes; websocket-task behavior on background sessions (assumed unsupported); ATS's numeric
TLS requirements; macOS daemon/lifecycle contrast (platform knowledge, not fetched).
**Android:** whether NSC `<pin-set>` binds OkHttp and Cronet (the docs explicitly defer to
"library implementation"); Cronet embedded binary size; Java-fallback feature gaps; App
Standby bucket quotas; DownloadManager force-stop survival; OkHttp `followSslRedirects`
default and dispatcher limits.
**Windows:** WinHTTP HTTP/2 arriving in Win10 1607 (corroborated, not doc-verified); HTTP/3
requiring Win 11/Server 2022 (Q&A source); WinINet deprecation status (interpretation).
**Linux:** KDE proxy specifics (community sources); snap network-interface details (reference
page 404); Fedora p11-kit paths; NetworkManager metered-flag non-integration; libsoup3
HTTP/2.
**Web:** Firefox status on streaming request bodies; Resource Timing granularity for fetch;
Background Sync/Periodic Sync support status.

The step-02/03 spikes intersect this list where it matters most: BoltFFI streaming semantics
(§7.6), NSC pinning coverage (§7.3), and — if background transfer is ever promised — a
minimal URLSession-background + relaunch rehydration probe on real hardware.
