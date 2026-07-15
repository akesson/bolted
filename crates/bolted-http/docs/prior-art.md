# bolted-http — prior art: cross-platform HTTP abstractions and where they fail

**Status:** design study, 2026-07-09. Input to the `bolted-http` contract design (which happens
after spike steps 02–03). Companion doc: [platform-surfaces.md](platform-surfaces.md).

**Method:** web research across framework and systems ecosystems; every load-bearing claim
carries a source URL. Claims that could not be fully verified are flagged inline and collected
in [§7](#7-verification-notes). Quoted feature matrices should be re-checked against live docs
before the contract is frozen — several are fetch-extractions of pages that change.

**The question this doc answers:** `bolted-http` is "one typed contract, native execution per
platform" — the core emits typed request effects, adapters execute them on URLSession, OkHttp /
Cronet, WinHTTP / WinRT, libcurl / reqwest, `fetch`. At least six ecosystems have attempted a
version of this. Where did each fail, and what does that dictate for the contract?

---

## 1. The convergence: everyone ends up native-backed on mobile

Every mature ecosystem, regardless of starting point, arrived at the same asymmetry: **own
portable stack on server/desktop, native stack on iOS/Android/browser.**

| Who | Started with | Ended with |
|---|---|---|
| Google / Flutter | pure-Dart `dart:io HttpClient` | `cupertino_http` (NSURLSession) + `cronet_http` (Cronet) as blessed clients |
| Microsoft / .NET | platform handlers everywhere | own `SocketsHttpHandler` on desktop, **native handlers kept as mobile default** |
| JetBrains / Ktor | one API, per-platform engines from day one | Darwin=NSURLSession, OkHttp, Js=fetch — plus a published divergence matrix |
| React Native | native from day one (NSURLSession/OkHttp under a fetch polyfill) | still native; the polyfill is the problem (§3c) |
| Ionic / Capacitor | WebView fetch | official native HTTP plugin that monkey-patches `fetch` |
| Apple / Swift Foundation | — | URLSession API implemented **on libcurl** on non-Darwin only; native on Darwin ([forums.swift.org](https://forums.swift.org/t/nsurlsession-libcurl/1790)) |
| Google / Cronet | own stack everywhere | retreated to Android-only; iOS support dropped (~M108), no desktop ([issues.chromium.org](https://issues.chromium.org/issues/40550620), [net-dev](https://groups.google.com/a/chromium.org/g/net-dev/c/AP8whXcVKUs)) |

The recurring justifications are always the same bundle: OS TLS lifecycle + certificate store,
system proxy/VPN (WPAD/PAC), ATS / cleartext-policy compliance, HTTP/3, OS cookie store,
battery/radio scheduling, background transfer, binary size. The dart-lang statement is the
crispest official admission: `dart:io HttpClient` lacks "HTTP 3 support, platform constructs
(e.g. VPN, proxy and shared cookie support on iOS)"
([dart-lang/http#764](https://github.com/dart-lang/http/issues/764)).

Even Google, owning the best portable stack on earth, could not sustain it on Apple platforms.
Even Mozilla declined to extract Necko for embedding: "Necko is not well-suited for use outside
of the Gecko lifecycle" ([mozilla.github.io](https://mozilla.github.io/firefox-browser-architecture/text/0002-extracting-necko.html)).

**Conclusion:** bolted-http's premise (native-backed adapters) matches the end state of every
ecosystem studied. The rest of this doc is about how they failed *after* going native.

## 2. Why the platform stack is not optional on mobile

Things a portable stack **cannot reach even in principle** — they live in OS services above the
socket API:

- **iOS background transfers** run in an out-of-process daemon; the app can be suspended or
  terminated and is relaunched on completion. No third-party socket code can participate
  ([developer.apple.com](https://developer.apple.com/documentation/foundation/downloading-files-in-the-background)).
- **App Transport Security** is enforced only for the URL Loading System; libcurl/reqwest
  traffic silently bypasses the platform TLS policy — "you take responsibility for ensuring the
  security of the connection"
  ([developer.apple.com](https://developer.apple.com/documentation/security/preventing-insecure-network-connections)).
- **Android network security config** (cleartext policy, pinning, user CAs) is honored by
  URLConnection/Cronet/OkHttp; "native code libraries… mostly have no flag awareness"
  ([developer.android.com](https://developer.android.com/privacy-and-security/risks/cleartext-communications)).
- **IPv6-only App Review gate**: NSURLSession works with DNS64/NAT64 automatically; apps doing
  their own sockets/DNS get rejected ([developer.apple.com/support/ipv6](https://developer.apple.com/support/ipv6/)).
- **System proxy discovery** (WPAD/PAC, VPN routing) — the stated reason Electron exposes
  Chromium's stack as `net` ([electronjs.org](https://www.electronjs.org/docs/latest/api/net))
  and axios users ask for a `net` adapter ([axios#5698](https://github.com/axios/axios/issues/5698)).

## 3. Case studies

### 3a. libcurl — a pluggable *sub-layer* is already a divergence machine

libcurl is the inverse decomposition (one protocol engine, N TLS backends), and even that
narrow plug point produced a permanent per-backend feature matrix that curl must officially
document ([curl.se/libcurl/c/tls-options.html](https://curl.se/libcurl/c/tls-options.html),
[ssl-compared](https://curl.se/docs/ssl-compared.html)): OpenSSL supports the most options,
rustls the fewest; pinning (`CURLOPT_PINNEDPUBLICKEY`) works on some backends and not others;
Schannel gets TLS 1.3 only when the OS ships it, then diverges for years
([curl#4918](https://github.com/curl/curl/issues/4918),
[curl#15482](https://github.com/curl/curl/issues/15482)); HTTP/3 is unavailable on Schannel
entirely ([curl.se/docs/http3.html](https://curl.se/docs/http3.html)).

Backends also get **removed**: Secure Transport was dropped in 2025, orphaning exactly the
users who chose it for native cert-store access; a Network.framework backend is "far from
straight-forward" and unattempted
([daniel.haxx.se](https://daniel.haxx.se/blog/2025/01/14/secure-transport-support-in-curl-is-on-its-way-out/)).

**Lesson:** if swapping one *sub-layer* costs this much, swapping *entire stacks* diverges at
least as broadly. Write the contract against the intersection; type-gate the rest. Treat each
adapter as a product with a lifecycle, not a one-time port.

### 3b. Flutter — the lean contract that still leaked

`package:http`'s `Client` is deliberately minimal, with native-backed implementations and a
conformance suite (`http_client_conformance_tests`) created explicitly to keep implementations
honest. It still leaked:

- **Timeouts:** `cronet_http` cannot implement a per-request timeout at all — Cronet binds
  sockets to requests late ([dart-lang/http#1186](https://github.com/dart-lang/http/issues/1186)).
- **Threading:** `cronet_http` (platform channels) can't run off the main isolate;
  `cupertino_http` (FFI) can ([#876](https://github.com/dart-lang/http/issues/876)).
- **Backend availability:** Cronet comes from Play Services or a fat embedded build; absent
  Play Services you silently get a degraded Java fallback
  ([#1268](https://github.com/dart-lang/http/issues/1268)).

The background-transfer story is the sharpest cautionary tale. Core Flutter never shipped it
([flutter#32161](https://github.com/flutter/flutter/issues/32161)). `flutter_downloader`
accumulated iOS failure modes — segfault on the NSURLSession delegate queue when the app is
terminated mid-download ([#623](https://github.com/fluttercommunity/flutter_downloader/issues/623)),
files unfindable after relaunch ([#568](https://github.com/fluttercommunity/flutter_downloader/issues/568)).
The package that finally worked, `background_downloader`, **abandoned the http abstraction
entirely**: it wraps URLSession background sessions + WorkManager with a *task*-based API and
platform-declared envelopes stated up front (iOS ≤4h, Android ~9min unless pausable, force-kill
cancels everything) ([pub.dev](https://pub.dev/packages/background_downloader)).

### 3c. React Native — a foreign API promised, never specified

RN promises browser `fetch` over NSURLSession/OkHttp with no written contract beneath it. The
official docs *themselves* list the breakage: `redirect:manual` and `credentials:omit` "are
currently not working"; a 302 with `Set-Cookie` on iOS mis-sets the cookie and can loop
infinitely; duplicate headers on Android keep only the last
([reactnative.dev/docs/network](https://reactnative.dev/docs/network)). Streaming is
structurally impossible through the serialization bridge
([facebook/react-native#27741](https://github.com/facebook/react-native/issues/27741)); binary
bodies spawned a lineage of third-party stopgaps (rn-fetch-blob → react-native-blob-util). A
2026 regression made Android fire `onerror` instead of `ontimeout` because an internal switch
from OkHttp `connectTimeout` to `callTimeout` changed the exception type and nobody owned the
error mapping — the error taxonomy was never part of any contract.

Background: core has nothing; the canonical upload package is abandonment-grade
([Vydia/react-native-background-upload](https://github.com/Vydia/react-native-background-upload));
the maintained downloader again drops to raw URLSession background sessions and requires
native-side relaunch hooks
([kesha-antonov/react-native-background-downloader](https://github.com/kesha-antonov/react-native-background-downloader)).

### 3d. Cordova / Capacitor — emulating a foreign API over a bridge

Hybrid WebView HTTP inherits browser CORS against `capacitor://localhost` origins, so Ionic
officially recommends native HTTP plugins
([ionicframework.com](https://ionicframework.com/docs/troubleshooting/cors)). The official
`CapacitorHttp` plugin monkey-patches `window.fetch` — and the patch is not fetch:
`fetch(new Request(...))` silently falls back to browser fetch *with CORS*
([capacitor#6174](https://github.com/ionic-team/capacitor/issues/6174)); the patched XHR is
case-sensitive on headers, breaking under HTTP/2's lowercase headers
([capacitor#7160](https://github.com/ionic-team/capacitor/issues/7160)). The bridge carries
only strings/JSON, so binary is base64 and large transfers are punted to a separate plugin
([capacitorjs.com/docs/apis/http](https://capacitorjs.com/docs/apis/http)). The patching is
disabled by default — the flagship feature ships off.

### 3e. Xamarin / .NET MAUI — the rich contract that native stacks can't fill

The structurally *right* design (thin `HttpClient` façade over swappable `HttpMessageHandler`)
with the *wrong* contract size. `HttpClientHandler`'s rich surface — `Proxy`, cookie container,
`ServerCertificateCustomValidationCallback`, arbitrary methods, redirect config — is
unimplementable on NSURLSession/HttpURLConnection, so native handlers **throw
`PlatformNotSupportedException` at runtime or silently no-op for years**:

- NSUrlSessionHandler: a swath of members throw, tracked for .NET 11
  ([xamarin-macios#14632](https://github.com/xamarin/xamarin-macios/issues/14632)); `Proxy`
  still an open request ([#18635](https://github.com/xamarin/xamarin-macios/issues/18635)).
- Two cookie sources of truth (managed `CookieContainer` vs `NSHttpCookieStorage`) never
  reconciled — a standing bug family
  ([#9817](https://github.com/xamarin/xamarin-macios/issues/9817),
  [#9511](https://github.com/xamarin/xamarin-macios/issues/9511)).
- AndroidMessageHandler: cert-validation callback **existed but did nothing until 2022**
  ([dotnet/android#6665](https://github.com/dotnet/android/pull/6665)); no arbitrary HTTP
  methods (HttpURLConnection leak,
  [xamarin-android#7291](https://github.com/xamarin/xamarin-android/issues/7291)); HTTP/2
  incomplete — gRPC hard-errors and demands the managed handler
  ([grpc-dotnet#2031](https://github.com/grpc/grpc-dotnet/issues/2031)).

Net effect: **no single handler satisfies the contract on one platform** — the "right" handler
on Android depends on workload. Meanwhile Microsoft's desktop move to `SocketsHttpHandler` was
motivated by the mirror problem: "almost impossible to achieve consistency across platforms"
with platform handlers ([devblogs.microsoft.com](https://devblogs.microsoft.com/dotnet/net-5-new-networking-improvements/)).
The split fleet is deliberate: consistency wins where the OS is commoditized (desktop/server);
native wins where the OS is opinionated (mobile). Microsoft pays for both stacks plus a compat
matrix — the price of a contract too rich to be portable.

### 3f. Ktor — the closest prior art, and the honest one

One `HttpClient` API, per-platform engines (Darwin=NSURLSession, OkHttp, Js=fetch, CIO, Curl,
WinHttp). Ktor pulls policy up into common plugins (timeout, redirect, retry, negotiation) over
minimal engines — the right shape — and then **publishes its own divergence matrix** as
official docs ([ktor.io/docs/client-engines.html](https://ktor.io/docs/client-engines.html)):

- HTTP/2: unsupported on CIO and Android engines; opt-in with engine-specific switches elsewhere.
- Timeouts ([client-timeout](https://ktor.io/docs/client-timeout.html)): connect timeout
  unsupported on **Darwin** and **Js**; socket timeout unsupported on **Js** and **Curl**.
  Only the request timeout works everywhere.
- Proxy ([client-proxy](https://ktor.io/docs/client-proxy.html)): none on Jetty/Js; no SOCKS on
  Java/Apache/CIO; "HTTPS requests are currently not supported with the HTTP proxy for the
  Darwin engine."
- TLS/pinning: "SSL must be configured per engine" — no common API; pinning is OkHttp's
  `CertificatePinner` on Android, a Ktor-provided imitation on Darwin, nothing on CIO; unified
  TLS config is an open request ([KTOR-4085](https://youtrack.jetbrains.com/issue/KTOR-4085)).
- Escape hatches disable guarantees silently: a preconfigured `NSURLSession` ignores
  `configureSession`/`handleChallenge`; a custom session delegate **breaks the common timeout
  plugin** ([KTOR-8066](https://youtrack.jetbrains.com/issue/KTOR-8066)).
- Background transfer: **no story after ~10 years** — the Darwin background-session request
  sits in "Submitted" with 2 votes ([KTOR-7244](https://youtrack.jetbrains.com/issue/KTOR-7244)).

### 3g. Rust ecosystem — silent degradation, and two attempts at exactly this idea

- **reqwest** swaps its whole backend for browser `fetch` on wasm32 behind an *unchanged API*:
  `timeout()`, cookies, proxies, streaming request bodies, redirect policy all silently absent
  or different, with the same `Error` type so failures look uniform while semantics differ
  ([reqwest#1135](https://github.com/seanmonstar/reqwest/issues/1135),
  [docs.rs](https://docs.rs/reqwest/latest/wasm32-unknown-unknown/reqwest/)). This is the
  anti-pattern: a capability-degraded adapter hidden behind an identical surface.
- **Trust has already converged native**: reqwest v0.13 (Dec 2025) made rustls + the **platform
  certificate verifier** the default — even the flagship own-stack client concluded trust
  evaluation must be delegated to the OS
  ([seanmonstar.com](https://seanmonstar.com/blog/reqwest-v013-rustls-default/)).
- **nyquest** ([github.com/bdbai/nyquest](https://github.com/bdbai/nyquest)) is a live Rust
  attempt at exactly bolted-http's shape: interface crate + N backends (NSURLSession, WinRT
  `Windows.Web.Http`, WinHTTP, libcurl, reqwest). Young: cookies, WebSocket, progress, and
  middleware still missing — a map of exactly where per-platform semantics get hard. **frakt**
  (NSURLSession/WinHTTP/BITS bindings) is the abandonment mode: one author, 1 star, no releases.
- **isahc** (libcurl bridge) went dormant for ~4 years between releases — bridge crates outside
  a framework's maintenance envelope rot ([crates.io](https://crates.io/crates/isahc)).

### 3h. Electron and Tauri — the split-brain problem

Electron offers Chromium's stack (`net`: system proxy, WPAD/PAC, NTLM/Kerberos, cert store)
*alongside* Node's http — and the ecosystem still fragments because libraries default to the
portable stack. VS Code is the canonical casualty: Chromium UI proxies fine, the Node extension
host needed a years-long shim saga
([vscode#12588](https://github.com/microsoft/vscode/issues/12588)). Tauri's plugin-http runs
reqwest even on mobile, producing **two cookie worlds** (webview vs plugin jar) that break
login flows ([tauri#12988](https://github.com/tauri-apps/tauri/issues/12988)).

**Lesson:** default placement decides everything. If the native-backed path is not the *only*
route for app HTTP, the portable stack leaks back in through dependencies. Bolted's effect
funnel — all HTTP leaves the core as typed effects — is the structural fix; do not add an
escape-hatch client.

## 4. Failure taxonomy

Ranked by evidence volume across ecosystems:

1. **Timeout semantics.** Connect/read/write/call timeouts mean different things per stack;
   some stacks can't express some kinds at all (Cronet: no per-request timeout; Darwin: no
   per-request connect timeout; fetch: AbortController only). *Every* ecosystem leaked here.
2. **Cookies × redirects.** Two sources of truth (managed jar vs OS store), Set-Cookie on
   redirect hops, no redirect interception on fetch. Standing bug families in RN, Xamarin,
   Tauri, Capacitor.
3. **TLS validation / pinning.** Callbacks are unportable (URLSession = delegate challenge,
   OkHttp = CertificatePinner, fetch = impossible, curl = per-TLS-backend). Retrofits and
   silent no-ops everywhere.
4. **Proxy.** Per-engine support holes; enterprise WPAD/PAC only via OS stacks.
5. **Streaming / binary across the language boundary.** Bridges that carry only strings/JSON
   kill streaming and force base64 (RN, Capacitor). Body chunking must be first-class in the
   boundary protocol or it never appears.
6. **Error taxonomy.** Which native failure maps to which app-visible error was specified
   nowhere; internal engine changes silently changed app-visible behavior (RN 2026).
7. **Background transfer.** Failed everywhere it was bolted onto the request abstraction;
   succeeded only as a separate OS-task-shaped capability with process-death semantics.
8. **Escape hatches.** "Bring your own session/client" silently disables contract guarantees
   (Ktor KTOR-8066) unless the contract states what dies.
9. **Backend availability/lifecycle.** Backends appear (Play-Services Cronet vs fallback),
   degrade, and get removed (Secure Transport). Adapter capability is a *runtime* fact on some
   platforms.

## 5. Design lessons for bolted-http

1. **Contract = the intersection; capabilities = the rest, in types.** The surviving pattern is
   Ktor's (minimal engine contract + common policy layer + published divergence matrix). Bolted
   hardens the matrix from documentation into compile-time verification: an adapter that cannot
   express a per-request connect timeout must not typecheck against a contract that promises
   one. Rich contracts fail as `PlatformNotSupportedException` (Xamarin); identical surfaces
   over degraded backends fail as silent divergence (reqwest-wasm). Both are the same mistake
   with different error reporting.
2. **Conformance suite from day one.** Google needed `http_client_conformance_tests` to keep
   four implementations of a *lean* interface honest. Bolted already has the conformance-suite
   culture (invariant tests); per-adapter contract tests are the same artifact. Error taxonomy
   (which native failure → which typed error key) is part of the tested contract, not a mapping
   left to each adapter's judgment.
3. **Background transfer is a different capability, not a request option.** Every ecosystem's
   evidence agrees: OS task identity that survives process death, file-based payloads, native
   relaunch hooks, progress as events, documented OS envelopes (iOS ≤4h, force-kill cancels).
   Model it as a separate effect family (`BackgroundTransfer`), optional per platform, never as
   a flag on the foreground request effect. This matches ARCHITECTURE §9's durable-effect
   precondition.
4. **One owner for session state per platform.** Cookies, TLS trust, and proxy belong to the
   native stack; the core must not mirror them (Xamarin's dual cookie stores, Tauri's two
   jars). If the core needs cookie *values*, that is a capability request, not shared ownership.
5. **Pinning as declarative data, not callbacks.** SPKI hashes in config map to
   CertificatePinner / URLSession challenge handling / network-security-config XML; callbacks
   are unportable and were retrofitted or broken in every ecosystem studied.
6. **Never emulate a foreign API.** RN's fetch polyfill and CapacitorHttp's monkey-patch show
   that every unimplemented corner of someone else's API becomes a silent behavioral fork.
   bolted-http defines its own typed effect — smaller than fetch, honest about what it is.
7. **The boundary must carry binary chunks.** Strings/JSON-only bridges (RN, Capacitor)
   permanently foreclosed streaming. Whatever BoltFFI can or cannot stream (step-02 probe)
   directly determines whether response streaming is in the contract or gated as a capability.
8. **Adapters live inside the framework's maintenance envelope.** isahc and frakt are what
   third-party bridge dependencies become. Each adapter is a product with a lifecycle —
   including runtime capability variance (Play-Services Cronet present or not) that the
   contract must represent.
9. **No second stack, no escape hatch.** The effect funnel is the structural advantage over
   Electron/Tauri's split brain; preserve it. If an escape hatch is ever added, the contract
   must state which guarantees it voids (Ktor's lesson).

## 6. What this means against Bolted's verification ladder

- Rung 1 (types): capability traits per feature area (timeouts, streaming, pinning, background
  transfer) rather than one rich `Http` trait; `Option<impl BackgroundTransfer>`-style optional
  capabilities; typed error keys.
- Rung 2 (shipped components): the adapters themselves, one per platform, maintained in-repo.
- Rung 3 (build checks): per-adapter conformance tests (the http analog of the invariant
  suite); error-taxonomy mapping tests; divergence matrix generated from the capability types,
  not hand-written prose.

## 7. Verification notes

Flagged as not fully verified during research; re-check before the contract design cites them:

- Exact per-option rows of curl's TLS-options matrix and current rustls-HTTP/3 status
  ([tls-options](https://curl.se/libcurl/c/tls-options.html), [http3](https://curl.se/docs/http3.html)).
- Precise current cells of Ktor's engine/timeout tables (docs versions change; an SSE column
  may exist in newer versions).
- Ktor Js-engine redirect/cookie limits: inferred from browser fetch behavior, not stated in
  Ktor docs.
- reqwest wasm `ClientBuilder` method-level surface (check `src/wasm/client.rs`).
- "libcurl has no WPAD/PAC support": inferred from curl proxy docs + competitor feature lists,
  not from an explicit curl statement.
- RN timeout regression issue number (#55081) returned by search under a non-canonical repo
  path; content consistent, URL should be re-checked.
- cordova-plugin-advanced-http npm publish dates (npmjs.com blocked; assessment rests on GitHub
  commit history).
