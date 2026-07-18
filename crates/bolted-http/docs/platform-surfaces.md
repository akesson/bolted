# bolted-http — the native API surfaces the adapters must map

**Status:** design study, 2026-07-09. Input to the `bolted-http` contract design (which happens
after spike steps 02–03). Companion doc: [prior-art.md](prior-art.md).

**Method:** inventory of official documentation (developer.apple.com, developer.android.com,
learn.microsoft.com, MDN, curl.se, libsoup/GLib docs). Claims that could not be verified
against a fetched official page are flagged inline and collected in
[§8](#8-verification-notes) as spike candidates.

**2026-07-18:** web removed from the platform set — it was never part of the asked
win/lin/mac/android/ios surface, and its "weakest adapter shapes the floor" premise no longer
holds. §6 keeps a short overview of how web would fit if it ever joins
(details: [feature-matrix.md §9](feature-matrix.md)); §9 (new) lists what can be made
homogeneous across the five platforms / four adapter surfaces, including what adapter code
can synthesize.

---

## 1. The surfaces at a glance

| | Apple (iOS/macOS) | Android | Windows | Linux |
|---|---|---|---|---|
| Native stack | URLSession (the only one) | OkHttp / Cronet / HttpsURLConnection (choice) | WinHTTP / WinRT / WinINet (+ own stack) | libcurl / libsoup3 / own stack |
| HTTP/3 | yes (system stack) | only via Cronet | Win 11 / Server 2022+ | via libcurl builds |
| OS-managed background transfer | yes — up+down, app relaunch | downloads only (DownloadManager) | yes — up+down (packaged apps); BITS otherwise | **none** |
| App code runs during bg transfer | **never** | yes (WorkManager) / no (DownloadManager) | no | always (it's your process) |
| Pinning | delegate (SecTrust) + declarative plist | CertificatePinner + declarative XML | cert-context inspection / WinRT event | you own the verifier |
| Cookies default | on (3rd-party blocked) | OkHttp: **off** | WinHTTP: session-only; WinRT: managed jar | per-library |
| Cache default | on (protocol policy) | OkHttp: **off** | WinHTTP: none; WinRT: yes | per-library |
| Timeout vocabulary | idle (60 s) + total (7 d) | connect/read/write (10 s) + call (off) | per-phase knobs; bg fixed 5 min/2 min | per-library |
| Proxy | OS-managed (+ per-session config) | ProxySelector / system | WPAD/PAC APIs, per-interface failover | env vars vs gsettings vs kioslaverc — no single truth |
| Cleartext HTTP | blocked (ATS) | blocked since API 28 | allowed | allowed |

Two structural facts fall out immediately:

1. **The background-transfer models are mutually incompatible in kind**, not just in detail —
   who executes the transfer, whether app code can run, what payloads are legal, and what
   survives termination differ per platform (§7.1).
2. **No single surface is the floor** (since web's removal): the contract's ceiling is set
   per dimension by the least-featured stack *after* adapter synthesis — see §9 for which
   gaps configuration or custom adapter code can close, and feature-matrix §4 for the rows
   that remain capabilities because no seam exists to synthesize from.

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

## 6. Web — out of the platform set; how it would fit

Web was removed from the platform set on 2026-07-18 (never part of the asked surface). The
Rust-web shell exists as a Bolted target, so the question may return; the short version of
what the 2026-07-18 web sweep established (raw evidence:
[research/2026-07-18-web.md](research/2026-07-18-web.md), full row-by-row deltas:
[feature-matrix.md §9](feature-matrix.md)):

- The adapter would be `fetch` via web-sys, zero FFI — a fifth conformance implementor with
  no BoltFFI machinery.
- The deadline maps to `AbortSignal.timeout`, with timeout-vs-cancel classified via
  `signal.reason` (WebKit rejects with `AbortError` even on timeout).
- Several portable rows would demote to capabilities again: redirect hop trace (fetch hides
  hops), upload progress (fetch has none; XHR-only), pinning (impossible — HPKP removed),
  negotiated-version observability (TAO-gated Resource Timing only).
- Download-to-file survives via OPFS `createWritable` (Baseline since Sept 2025) — but into
  origin-private storage, not a path, which is why `FileRef` stays an opaque newtype.
- CORS, forbidden headers, browser-owned cookies/TLS/proxy, and the absence of any portable
  background mechanism are absolute — browser-owned, no adapter seam.
- The one decision worth taking early *if* web joining is plausible: contract traits must use
  conditional `Send` bounds (wasm futures are `!Send`) — cheap when the traits are written,
  expensive to retrofit.

## 7. What the intersection permits — contract implications

### 7.1 Background transfer is four different machines

| | iOS | Android | Windows (packaged) | Linux |
|---|---|---|---|---|
| Who executes | OS daemon | your code (WorkManager) or OS (downloads only) | OS service | you |
| Uploads | file-based only | app-code only | yes | you |
| Survives force-quit | **no** (cancelled) | WorkManager: yes | yes | n/a |
| Redirect/auth hooks during bg | none | full (own code) / none (DownloadManager) | none | full |
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
callTimeout / fixed OS budgets) — and on Apple/.NET the per-request form is adapter-
synthesized (timer + cancel), not native. Idle, connect, read, and write timeouts exist on
*some* stacks with *different* meanings — Apple has idle+total but no connect; OkHttp has
connect/read/write but total off by default; background transfer has none configurable
anywhere. Contract: `deadline` portable; finer-grained timeouts are composition-root
configuration. (This single issue produced the most prior-art bugs — see prior-art §4.)

### 7.3 Pinning: declarative data, mapped per adapter

All four surfaces can implement SPKI-pin *data* (SecTrust evaluation / CertificatePinner /
cert-context inspection / own verifier) — native only on Android; adapter code elsewhere
(see §9 bucket C). The declarative-XML route on Android needed a spike check and got its
answer (feature-matrix §5.14: `<pin-set>` binds OkHttp via Conscrypt but is
custom-TrustManager-fragile — the adapter carries its own pins). Callbacks are unportable
everywhere (prior-art lesson 5).

### 7.4 Cookies and caching: pick an explicit default; the defaults conflict

URLSession: both on. OkHttp/WinHTTP: both off. A contract that is silent inherits a
per-platform coin flip. The bolted-shaped answer: **the portable request effect is
cookie-less and cache-less by default** (matching a core that treats HTTP as an effect, not
ambient state); cookie/cache participation is opt-in per adapter.

### 7.5 Redirects: observe-maybe, intercept-never

Foreground *observation* exists on URLSession (delegate), OkHttp (interceptors), Cronet
(explicit consent), WinHTTP (callback); on .NET only by disabling auto-redirect and
following manually — an honest adapter synthesis under a cookie-less contract
(feature-matrix §5.5). **No background transfer exposes hops on any platform.** Portable
contract: redirects are followed; the final URL is reported; the hop *trace* is
foreground-only and recorded, never an async veto.

### 7.6 Streaming and progress

Response streaming is portable (every surface). Request streaming is now platform-portable
too, but excluded by design — an effect carries complete data (feature-matrix §5.3).
Upload progress is portable via adapter synthesis (OS-fed only on Apple; sink/stream
wrapping elsewhere). Download progress is portable. Independent of platforms, whether
streamed bodies can cross BoltFFI at all is exactly the step-02 probe (stream burst/ordering
semantics) — the platform ceiling is known; the FFI ceiling is the open question.

### 7.7 Proxy and trust: never portable concepts

Proxy is OS-coherent on Windows/Apple, a four-way policy mess on Linux. Trust evaluation is
platform-delegated everywhere (and the Rust ecosystem converged on the same — prior-art
§3g). Neither appears in the portable contract; both are adapter configuration.

### 7.8 Protocol versions and cleartext

The contract must not promise HTTP/3 (OkHttp caps at h2; Windows 10 caps at h2) — version
is an observable, not a request parameter. HTTPS-only is safe as the portable rule: ATS and
Android API 28+ already enforce it, and the sans-io core can refuse non-https URLs before
dispatch on Windows/Linux; cleartext is a dev-only, platform-config-gated exception.

### 7.9 Observability

DNS/connect/TLS/first-byte/bytes timing is rich on Apple/.NET/OkHttp and **coarse on Linux**
— the "implementable on every native surface" claim here was corrected by the second round
(feature-matrix §1.5: reqwest exposes no per-phase timing, and no seam exists to synthesize
it from). Metrics: an optional, *tiered* capability.

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

Most of these flags were resolved by the 2026-07-18 sweeps — see feature-matrix §1 and the
raw reports in `research/`.

The step-02/03 spikes intersect this list where it matters most: BoltFFI streaming semantics
(§7.6), NSC pinning coverage (§7.3), and — if background transfer is ever promised — a
minimal URLSession-background + relaunch rehydration probe on real hardware.

## 9. What can be made homogeneous — and how

Added 2026-07-18. The five platforms / four adapter surfaces, sorted by *what it takes* to
make each feature behave identically everywhere. Buckets A–C are all homogenizable — the
difference is only where the work lives; bucket D is not, because no seam exists to write
code against. Classifications match [feature-matrix.md](feature-matrix.md) §4 (bucket C =
its CORE(adapter) rows).

### A. Homogeneous natively — the primitive exists on all four surfaces

Adapter work is mapping, not building: method/URL/typed headers; `Bytes`/`File` request
bodies streaming from disk; redirect auto-follow with final URL; response streaming;
download progress; cancellation of in-flight calls; negotiated-version observability
(all four stacks report it); 401/407 as ordinary responses; typed error mapping (the mapping
table is per-adapter work, but every input is a native error the stack already surfaces).

### B. Homogeneous by configuration — the defaults conflict, one decision per adapter

- **Cookie-less + cache-less**: ephemeral/no-store session (Apple), `NO_COOKIES` + no cache
  (OkHttp — its default), no `CookieContainer`/no cache (.NET — its default), no jar
  (reqwest — its default).
- **Decompression normalized**: `DecompressionMethods.All` on .NET (default is None!),
  brotli/zstd modules on OkHttp, gzip/brotli/zstd features on reqwest; Apple decodes always.
- **Request-level retry off**: reqwest `retry()` unused; connection-level recovery stays at
  platform defaults everywhere (the split in feature-matrix §5.17).
- **HTTPS-only**: enforced natively by ATS / Android API 28; on Windows/Linux the sans-io
  core refuses non-https URLs before dispatch — configuration-free homogeneity.
- **Fine timeouts**: aligned client-wide at the composition root; never per-request anywhere.

### C. Homogeneous by adapter code — a native gap, closed by custom code in the shipped adapter

The feature-matrix CORE(adapter) rows; per-surface synthesis (— means native, no synthesis):

| Feature | Apple | Android/OkHttp | Windows/.NET | Linux/reqwest |
|---|---|---|---|---|
| Per-request total deadline | timer + cancel (no native per-request wall clock) | — (`Call.timeout`) | per-request CTS; re-armed per-read cancel once streaming | — (`RequestBuilder.timeout`) |
| Multipart body | hand-built (core-supplied boundary) | — (`MultipartBody`) | hand-built | — |
| https→http refusal | redirect delegate | `followSslRedirects(false)` (config) | — (enforced) | custom redirect policy |
| Redirect hop trace | — (delegate sees hops) | — (network interceptor) | manual-follow loop (no observation hook) | — (policy closure) |
| Upload progress | — (OS-fed `didSendBodyData`) | sink wrapping | flush-aware content-stream wrapping | body-stream wrapping |
| Download-to-file | — (`downloadTask`) | stream copy to file | stream copy | `bytes_stream` to disk |
| SPKI pinning | trust-evaluation delegate | — (`CertificatePinner`) | cert-validation callback | custom rustls verifier (spike-gated) |

The cost of this bucket is real and lives in the conformance suite: every synthesis is
adapter code the platform will never test for us (feature-matrix §7, rules 3, 4, 10, 11).

### D. Cannot be homogenized — no seam to synthesize from

- **Per-phase metrics on Linux**: reqwest exposes no DNS/connect/TLS timing and no hook that
  observes those phases → metrics stay a *tiered* capability.
- **Request priority on OkHttp/.NET**: no API (FIFO dispatcher / closed-wontfix) — nothing
  to write code against, and faking wire priority is not compensation.
- **Trailers on Apple/reqwest**: no public API surfaces them → OUT.
- **Enterprise auth on Android**: no built-in NTLM/Negotiate; reimplementing auth protocols
  in adapter code is a liability, not a synthesis → OUT.
- **OS-run background transfer on Linux**: no OS service exists; a helper process is an app
  architecture, not an adapter → the background family's `availability()` reports it.
- **A single proxy truth on Linux**: structural (env vars vs gsettings vs kioslaverc) →
  document, don't promise.
