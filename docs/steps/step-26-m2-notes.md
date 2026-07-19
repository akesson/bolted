# Step 26 M2 — the syntheses (notes for M3+)

**Milestone:** M2 (trust anchor, SPKI pinning + N3 controls, https→http refusal, file sink / `Io`, N4
gzip honesty, `PermissionDenied`, C3 Android column). **Branch:** `step/26-android-adapter`. Scope:
the FULL suite green on the real `BoltedHttp` OkHttp adapter — all C1 rows (11 `rows()` + 4
`extra_rows()`), every reachable C2 key, the pinned C3 Android column. Every newly-green M2 row was
watched red first. M3 (N5 HttpEngine detection) and M4 (mutation pass) are untouched. **No FFI/Rust
change** (the surface was already M2-ready from Apple; `bolted-http` contract untouched).

## Gate result

- `mise run check` — green (host, JDK-free; no Rust changed).
- `mise run test:android:http` — green on the headless `dev34` GMD (aosp_atd android-34 arm64):
  **`tests="12" failures="0" errors="0" skipped="0"`**, verified against the JUnit XML (not the
  wrapper exit code). 12 tests = 2 M0 bridge-gate (`ConformanceProbe`) + 3 N2 stream-probe
  (`StreamProbe`, retained) + 2 `M1Conformance` (watched-red, deadline) + **5 new `M2Conformance`**:
  - `theFullSuiteIsGreenOnTheRealAdapter` — the real adapter over C1 `rows()` + `extra_rows()` + C2
    `rows()` with the trust anchor installed: **all 25 driver rows GREEN, none skipped**, plus the
    pinned C3 Android column.
  - `theServerTrustManagerSplitIsCauseNotConflated` — the N3(a) unit control (the trust-vs-pin split +
    the SPKI computation).
  - `theHostnameLessTwoArgCheckServerTrustedIsTrustOnly` — the N3(b) 2-arg landmine.
  - `theNscPinSetDoesNotBindTheAdapter` — the N3 NSC `<pin-set>` verdict.
  - `thePermissionDeniedMapping` — the `PermissionDenied` cause-mapping control.

All row outcomes below are **observed** from the on-device per-test logcat
(`build/outputs/androidTest-results/managedDevice/debug/dev34/logcat-*.txt`) — the GMD JUnit XML has
no `<system-out>` (F-M1-6), so the messages live in logcat.

## Row status table (the six M2 rows — M1 RED → M2 GREEN, watched red first)

| Row | M1 | M2 | Watched-red evidence (observed, under BrokenHttp unless noted) |
|-----|----|----|----------------------------------------------------------------|
| C1/rule-04 https-to-http-refused | RED (Tls, no anchor) | **GREEN** | `WrongErrorKey { expected: InsecureRedirect, got: Transport }` |
| C1/rule-10 pin-mismatch-typed-error | RED (Tls, no anchor) | **GREEN** | `ExpectedSuccessGotError { got: Transport }` (its positive good-pin leg errors first) |
| C1/row-15 response-sink-correspondence | RED (WrongSink) | **GREEN** | `ExpectedSuccessGotError { got: Transport }` |
| C2/key-pin-mismatch | RED (Tls) | **GREEN** | `WrongErrorKey { expected: PinMismatch, got: Transport }` |
| C2/key-insecure-redirect | RED (Tls) | **GREEN** | `WrongErrorKey { expected: InsecureRedirect, got: Transport }` |
| C2/key-io | RED (ok, sink ignored) | **GREEN** | `WrongErrorKey { expected: Io, got: Transport }` |

All 19 M1-green rows stayed green (their watched-red evidence is in the M1 notes; the M2 watched-red
sweep re-reds every non-`Transport`-expecting row across C1+extra+C2). The two `Transport`-expecting
rows are watched red by `AlwaysOkHttp`: rule-08 `HiddenRetry { connections: 2 }`, key-transport
`ExpectedErrorGotSuccess { expected: Transport, status: 200 }`. The deadline red-watch still holds:
PerIdle/`readTimeout` `ExpectedErrorGotSuccess { expected: Timeout, status: 200 }` (RED) vs
Total/`callTimeout` `passed=true` (GREEN).

**25 driver rows green** (11 C1 `rules` + 4 `extra_rows` + 10 C2 `keys`).

**C3 Android column** (pinned in `theFullSuiteIsGreenOnTheRealAdapter`, generated from the capability
traits — `KotlinFactory` self-report: no `priority_hint` override, `metrics` at `Phase`):

```
capability     | presence
---------------+-----------------------
priority-hint  | absent
metrics        | present (Phase)
```

This is the decided Apple/Android divergence: Apple maps the hint to `URLSessionTask.priority`
(present); OkHttp has no per-`Call` priority knob (absent). Metrics tier matches Apple (`Phase`).

## What M2 built

**`android/bolted-http/.../BoltedHttp.kt`** — the four adapter-side syntheses on top of the M1 base:

- **Server-trust anchoring** (`trustAnchorDer`, a TEST-tier field mirroring Apple's `trustAnchorDER`;
  the shipped adapter hard-codes no anchor). When set from `ServerInfo.goodCertDer`, the per-call
  client verifies the good self-signed endpoint against a custom `X509TrustManager` trusting exactly
  that anchor (built via a one-entry `KeyStore` → the platform PKIX `TrustManagerFactory`). The
  untrusted endpoint stays rejected (`key-tls` green).
- **The SPKI pinning split** (rule 10), mirroring the Linux `PinningVerifier` / Apple trust delegate:
  the custom TM (1) delegates the real chain check to the anchor-based PKIX manager, then (2) on a
  PASSING chain with pins present, compares SHA-256 over the leaf's SubjectPublicKeyInfo. A
  chain/hostname failure throws → OkHttp wraps it `SSLHandshakeException` → `Tls`; a chain that
  VALIDATES but has no pin matching the leaf SPKI records the cause (`ctx.pinMismatch`) and throws →
  the classifier maps the recorded cause to `PinMismatch`, never `Tls`. Any one matching pin
  satisfies.
- **https→http refusal** (rule 4): `followSslRedirects(false)` makes OkHttp DECLINE a cross-scheme
  redirect (leaving the un-followed 3xx as the response); `insecureDowngradeTarget` detects the
  https→http downgrade in `onResponse` and refuses with `InsecureRedirect(to)`. Same-scheme redirects
  are still auto-followed, so the `priorResponse` hop trace, `TooManyRedirects` cap, and rule-03/08
  are untouched (all re-verified green).
- **The file sink** (row 15 / `Io`): a `File` sink streams the body to the path with Okio
  (`response.body.source()` → `File.sink().buffer().writeAll(source)` — never buffering the whole
  body), atomic finalize (a same-dir temp file, then `renameTo`); a write failure is `Io`.

**`android/bolted-http-conformance/.../M2Conformance.kt`** — the full-suite gate + the N3/permission/C3
controls. **`.../M1Conformance.kt`** — retained the watched-red baseline (now sweeping the whole suite,
so the six M2 rows are red-watched) + the total-deadline red-watch; dropped the M1-only "green except
syntheses" split test (superseded by the M2 full-suite gate).

**`.../src/main/res/xml/network_security_config.xml` + manifest** — the N3 NSC `<pin-set>` control
(see below). TEST-tier only; the shipped `android/bolted-http` carries no NSC.

## N3 fragility controls (the freeze §9 evidence)

**(a) The NSC `<pin-set>` verdict — the headline §9 answer.** The conformance manifest installs a
Network Security Config with a `<pin-set>` carrying a **deliberately wrong** pin (32 zero bytes) for
`127.0.0.1`. `theNscPinSetDoesNotBindTheAdapter` proves the split (observed, verbatim):

> `N3 NSC verdict: NSC <pin-set> present (wrong pin for 127.0.0.1) but NOT enforced on the adapter's
> custom-SSLSocketFactory connection (200); the identical pin BLOCKS via CertificatePinner ⇒ pinning
> is adapter-enforced, never NSC. (§9: <pin-set> does NOT bind OkHttp.)`

- Arm 1 (bypass): a client using the adapter's custom `SSLSocketFactory` + **no** `CertificatePinner`
  GETs the good HTTPS endpoint → **200**, even though the wrong NSC pin is present. The *entire*
  full-suite HTTPS run is itself corroborating evidence: it passes with the wrong NSC pin installed.
- Arm 2 (control): the **same** wrong pin enforced at the OkHttp level (a `CertificatePinner`) →
  **`SSLPeerUnverifiedException`** — the pin value is genuinely wrong and OkHttp-level pinning does
  bite.

**Verdict for the freeze §9 question ("does `<pin-set>` bind OkHttp?"): NO** — Android's NSC pinning
is enforced by the platform `NetworkSecurityTrustManager`, which is only in the chain when the default
SSLContext/TrustManager is used. Every real pinning adapter (and this one) installs a custom
`SSLSocketFactory`/`X509TrustManager`, which replaces it entirely, so `<pin-set>` is never consulted.
The suite's pinning is therefore *adapter*-enforced and must never silently depend on NSC. (Cleartext
policy is enforced at a different layer and DOES still apply — the NSC keeps `cleartextTrafficPermitted`
true so the loopback http rows work.)

**(b) The hostname-less 2-arg `checkServerTrusted` landmine.**
`theHostnameLessTwoArgCheckServerTrustedIsTrustOnly` pins it (observed):

> `N3(b) landmine: 2-arg checkServerTrusted is hostname-blind — host binding is the HostnameVerifier's
> job`

`X509TrustManager.checkServerTrusted(chain, authType)` is the two-argument interface method — it
receives **no hostname**, so it can express the trust decision (chain to the anchor) and the pin
decision (leaf SPKI) but CANNOT bind the cert to the connection's host: the same 2-arg call accepts
the good cert with no host context at all (the test asserts the 2-arg signature reflectively). Host
binding is OkHttp's `HostnameVerifier`'s job (the adapter leaves the default in place). An adapter that
did its trust logic here and forgot the verifier would accept a valid-but-wrong-host cert. The runtime
proof that host binding IS present end-to-end is `key-tls` (the untrusted endpoint is rejected).

**The split, at the unit level** (`theServerTrustManagerSplitIsCauseNotConflated`, observed):

> `N3(a) split: SPKI matches good_spki; matching pin accepts, wrong pin ⇒ PinMismatch cause`

Non-vacuous core: `BoltedHttp.spkiSha256(goodCert)` equals the server's `good_spki`
(`PublicKey.getEncoded()` IS the SubjectPublicKeyInfo DER on Android — the same bytes `x509_parser`'s
`public_key().raw` gives, so **no hand-rolled ASN.1 walk is needed**, unlike Apple's `SecCertificate`).
Then: no pins → accept; matching pin → accept; any-one-of-{wrong,right} → accept; wrong pin on a
passing chain → fire the `onPinMismatch` cause + throw.

## PermissionDenied treatment (mirrors step-25 F-M2-1)

`PermissionDenied` has **no hermetic host control** on the ART conformance tier: with INTERNET granted
and the in-process server on loopback, no host request can make the OS deny permission — the genuine
causes (a missing INTERNET permission → `SecurityException`; an app-sandbox / local-network denial →
`ErrnoException` `EPERM`/`EACCES`) are device/app-bundle-tier, not gating here. `c2::reachability`
already classifies the key `AdapterOnly`, so it is correctly absent from `c2::rows()`.

So the positive control is the load-bearing **mapping** (`thePermissionDeniedMapping`, observed):

> `PermissionDenied: mapping proven (SecurityException/EPERM/EACCES ⇒ key; network failures ⇒ null);
> live host control platform-gated`

`BoltedHttp.permissionKeyFor` walks the throwable cause chain: a `SecurityException` or an
`ErrnoException` with `EPERM`/`EACCES` maps to `PermissionDenied` (including nested in an `IOException`);
a `ConnectException` / plain `IOException` maps to `null` (the negative control — not vacuous). It is
wired into `classify` ahead of the type dispatch, so a genuine denial is mapped, never invented. This
feeds freeze Q5 (is `PermissionDenied` inherently a device/app-bundle-tier key on ALL platforms — it
is now `AdapterOnly` and platform-gated on both Apple and Android).

## N4 gzip honesty

Confirmed (no adapter change needed): rule-07 (memory sink) is green — OkHttp's transparent gzip
decodes the body and strips `Content-Encoding`/`Content-Length`; the adapter forwards the decoded
bytes and the Rust bridge computes `content_length = Some(decoded.len())`. The adapter itself never
reports a content-length (the FFI carries no such field for the adapter to lie with), so "None-or-honest"
holds **by construction**. Under the file sink the bridge reports `content_length = None` (a `File`
outcome), so gzip honesty survives the file sink (closing F-M1-3's file-sink half on Android too).

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **https→http refusal via `followSslRedirects(false)`, not a manual follow loop.** OkHttp declines a
   cross-scheme redirect and returns the un-followed 3xx; the adapter inspects it for the downgrade.
   This keeps M1's `priorResponse` hop trace, the `TooManyRedirects` cap, and same-scheme following
   entirely intact (all re-verified green) — a smaller, lower-risk change than rewriting redirect
   handling. The hop trace and the refusal coexist; the manual loop the M1 notes floated is not needed.
2. **SPKI via `PublicKey.getEncoded()`** (the SubjectPublicKeyInfo DER) — no ASN.1 walk. Android's
   `X509Certificate.getPublicKey().getEncoded()` is exactly the SPKI, unlike Apple's `SecCertificate`
   (F-M2-2). The unit control asserts it equals the server's `good_spki`.
3. **File sink = Okio streaming + `renameTo`.** `File.sink().buffer().writeAll(source)` streams
   segment-by-segment (never materialising the whole body); atomic finalize is a same-dir temp file +
   `renameTo`. A write failure (the `key-io` control's nonexistent parent dir) throws `IOException` →
   `Io`. File-sink `content_length` is `None` (mirrors Apple decision 2).
4. **`TMPDIR` is redirected to the app cache dir before the file-sink rows run.** The Rust suite builds
   the file-sink destination from `std::env::temp_dir()`, which on Android resolves to an unwritable
   `/tmp`; `android.system.Os.setenv("TMPDIR", cacheDir, true)` (in the full-suite test, before the
   rows) makes the in-process suite's `getenv("TMPDIR")` point at a writable dir so row-15 can write —
   while `key-io`'s nonexistent subdirectory stays nonexistent (→ the honest `Io`). See F-M2-1.
5. **The NSC `<pin-set>` control lives in the conformance manifest globally.** A wrong pin for
   `127.0.0.1` is installed process-wide; the full suite passing with it present is itself part of the
   proof that a custom `SSLSocketFactory` bypasses NSC. `cleartextTrafficPermitted` stays true so the
   loopback http rows keep working. TEST-tier only.
6. **`PermissionDenied` maps `SecurityException` + `ErrnoException(EPERM|EACCES)`.** Both genuine OS
   denials; a network errno (refused/timeout) is not permission-shaped. Live host control platform-gated
   (decision mirrors Apple's; positive control is the mapping).
7. **The M1 "green except syntheses" test was dropped, not kept red.** With the M2 file sink landing,
   that test's assertion (row-15/key-io RED without an anchor) is now false (the file sink needs no
   TLS). Superseded by the M2 full-suite gate; the watched-red baseline (extended to the whole suite)
   and the deadline red-watch are retained.

## Friction log (freeze-agenda input — friction matters more than usual this step)

- **F-M2-1 — `std::env::temp_dir()` is unwritable on Android; the file-sink path is Rust-chosen.** The
  suite builds the row-15 / key-io destination Rust-side from `temp_dir()` (`/tmp` on Android, not
  writable in the app sandbox). The adapter cannot influence the path — it is handed one across the
  FFI. The M2 workaround sets `TMPDIR` from the test before the rows run. **Freeze input:** a
  host-tier suite that hands a native adapter a `temp_dir()` path assumes a writable `/tmp`, which
  Android does not provide; the file-sink row's path source should be a tier-provided writable dir (or
  the suite should take it from an env the harness controls), not `std::env::temp_dir()`.
- **F-M2-2 — the NSC `<pin-set>` verdict cannot use a system-trusted server on the loopback tier.** The
  cleanest §9 proof (a wrong NSC pin *blocking* a default-stack connection, then the custom stack
  bypassing it) needs a system-trusted server; the test server is self-signed loopback, so the default
  stack fails at *trust* before pinning. The verdict is instead carried by (Arm 1) the custom-stack
  connection succeeding despite the wrong NSC pin + (Arm 2) the identical pin blocking via
  `CertificatePinner`. Honest and non-vacuous, but it proves "NSC not consulted on the custom stack"
  rather than "NSC would otherwise block" directly. **Recorded for the freeze** — the §9 answer (NSC
  does not bind OkHttp under a custom SSLSocketFactory) is sound; the demonstration is loopback-shaped.
- **F-M2-3 — `content_length` honesty splits by sink** (inherited from Apple F-M2-3, holds on Android).
  Memory ⇒ `Some(decoded len)`; File ⇒ `None`. The streaming sink (row 16, freeze) faces the same
  question with no in-memory body AND no completed file.
- **F-M2-4 — the pinning trust manager is rebuilt per request.** A fresh `SSLContext` + `KeyStore` +
  `TrustManagerFactory` is constructed inside every `execute` (to close over the request's pins), then
  handed to `client.newBuilder().sslSocketFactory(...)`. Fine for a conformance harness; a smell for a
  shipped adapter (a real one would cache the anchor-based factory and vary only the pin comparison).
  **Freeze input:** the pin set is per-request but the trust anchor is per-adapter — the per-request
  SSL rebuild conflates the two.
- **F-M2-5 — the too-many-redirects text match is still the one unavoidable string inspection**
  (carries F-M1-2 forward). `followSslRedirects(false)` did not remove it — OkHttp's own follow-up cap
  still surfaces only as a `ProtocolException` message. The redirect-ceiling-as-CFG freeze question
  (F-M1-1) would let the adapter enforce the cap itself and emit a typed cause.

## M3 / M4 hand-off

- **N5 HttpEngine detection** (M3, probe-grade, time-boxed): is `android.net.http.HttpEngine` present
  on the API-34 ATD? h3 negotiable against the test server? Record the verdict (real or paper); do NOT
  build the second engine path. `PriorityHint` stays absent regardless (OkHttp path).
- **M4 mutation pass**: mutate the M2 syntheses — the pin check (swap the leaf-SPKI compare / drop the
  chain-first ordering so a pin mismatch reads as `Tls` / drop the `any-one-matches`), the downgrade
  refusal (follow the https→http), the file-sink atomic finalize (skip the rename / buffer the whole
  body), the trace, cancel, progress, and the bridge's token routing. Two-hypotheses discipline on
  every survivor. Watch the `pinMismatch`-vs-`Tls` conflation specifically — the split is the sharpest
  M2 mutation target.
- **C3 Android column** pinned: `priority-hint absent`, `metrics present (Phase)`.
