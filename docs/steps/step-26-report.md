# Step 26 — report: bolted-http III, the Android adapter (S-AN)

**Status: done — no kill criteria hit; the suite grew one row catching a real blind spot; the
freeze agenda got its sharpest streaming evidence yet.**
Fourth step under the Fable-orchestrates model; five Opus sub-agent milestones (M0–M4; M0 and
M1 each stalled once mid-pass waiting on their own background emulator run and were resumed —
M2+ ran under a synchronous-run discipline that eliminated the failure mode), report by the
planning session. All work on `step/26-android-adapter` (M0 `17198fe` → M4 `44216d7`), merged
to `design/bolted-http`. Gating tier: the headless GMD instrumented ART tier (aosp_atd
android-34 arm64); a physical-device (Pixel 8a) pass stays a non-gating follow-up.

## What was built

- **The harness bridge** (`crates/bolted-http-android-ffi`, workspace member): a near-verbatim
  mirror of `bolted-http-apple-ffi` — same callback trait (`HttpAdapter`), token-keyed
  completion re-entry, structured row drivers, server lifecycle, chunk-probe surface. The two
  crates diverge only in target config, names, docs, and one behavioural line: `PriorityHint`
  is absent on Android (row 12 — OkHttp legally ignores the hint). **Duplicated, not shared,
  by necessity**: BoltFFI's bindgen reads the packed crate's source text, so exported items
  must live in the crate being packed (F-M0-1 — drift-check/tooling candidate for the freeze).
- **The adapter** (`android/bolted-http`): `BoltedHttp.kt`, OkHttp. **Total deadline =
  `callTimeout`, honestly — no timer synthesis needed** (the opposite disposition to Apple;
  proven both legs on `/drip`: per-idle `readTimeout` resets on every dribbled byte and never
  fires — watched red — while `callTimeout` fires mid-trickle). Cause-based C2 classification
  (recorded-cause disambiguation for cancel-vs-timeout); SPKI pinning with the exact
  chain-fail ⇒ `Tls` / pin-fail ⇒ `PinMismatch` split; `followSslRedirects(false)` + downgrade
  detection for rule 4 with the hop trace kept intact (`priorResponse` chain, reversed to
  traversal order); Okio-streamed file sink with atomic temp+rename; real version from
  `Response.protocol`; monotone upload progress via a request-body wrapper.
- **The consumer test package** (`android/bolted-http-conformance`): sibling Gradle project —
  the Android analog of the step-25 sibling-package convention.
- **Tooling**: `pack:android:http` + `test:android:http`. The test task **gates on the JUnit
  XML, never the wrapper exit code** (the `test:android` masking landmine is not inherited),
  and was proven fail-able with real assertion failures before any green counted.

## Results

- **Full conformance green on the real adapter**: 25 driver rows (11 C1 rules + 4 extra rows +
  10 C2 keys), C3 Android column pinned (`PriorityHint` absent, `Metrics(Phase)`). **Every row
  watched red first** (M0's gate: one row green AND legibly red under a deliberately-broken
  adapter, before anything trusted the bridge's greens).
- **N2 — the JNI stream verdict (freeze input, the point of the v1.15 re-sequencing).** The
  cross-FFI `ffi_stream` push is **lossless and ordered on ART**: 200/200 ingested, ascending
  seq, consumer always off-main, both pacings, and it **holds under 2-core CPU saturation** —
  step-02's stall ghost is dead on JNI too. Latency is real: p50 ≈ 506µs (paced) / 2.3ms
  (burst) vs Apple's ≈ 25µs (coroutine hop + batch-16 poll). Two sharp findings:
  - **F-M0-4:** BoltFFI's *generated Kotlin* `callbackFlow` does `trySend` into a bounded
    64-slot channel — **silent drop-on-overflow**. Fast collector: 200/200 both pacings; slow
    or contended collectors: 171, 132, 125/200 (and 130/200 under real CPU pressure in M3).
    The loss is entirely the binding's overflow policy, never the native push. → the streaming
    seam must specify a back-pressure/overflow policy.
  - **F-M0-5:** Apple's F-M3-1 **reproduces on ART, shape-changed**: an abandoned consumer no
    longer starves the next run's ingest, but the leaked subscription — which lives
    **native-side and survives ART GC-collecting the Kotlin scope** (proven with a
    ReferenceQueue control) — starves the next consumer's re-delivery (0–90/200). Two
    platforms now demonstrate the same requirement: **the subscription lifecycle must be
    scope-/Drop-bound at the native seam**; `awaitClose`-style unsubscribe never fires for an
    abandoned consumer.
- **N5 — HttpEngine is spike-real, its h3 leg is paper**: present and constructible on API 34
  (Cronet 114), drove a live conformance-relevant request end-to-end in the test target. But
  the conformance `TestServer` is a raw HTTP/1.1 listener (no ALPN/QUIC), so h2/h3 rows are
  not cheaply testable (F-M3-2). Bonus finding: a missing `ACCESS_NETWORK_STATE` makes
  HttpEngine crash uncatchably on Cronet's internal thread, taking the whole instrumentation
  down — an engine matrix must gate on permissions *before* construction (F-M3-1).
- **NSC `<pin-set>` does NOT bind OkHttp** (the ARCHITECTURE §9 question, answered with
  evidence): a deliberately-wrong process-wide NSC pin is silently bypassed once a custom
  `SSLSocketFactory`/`TrustManager` is installed, while the same pin blocks via
  `CertificatePinner` (control). Pinning on Android must be adapter-enforced; the declarative
  NSC route cannot carry the contract. The hostname-blind 2-arg `checkServerTrusted` landmine
  is unit-pinned.
- **`PermissionDenied`: platform-gated on the ART tier, recorded with evidence, not faked**
  (second platform, same treatment as step 25): no hermetic denial reachable; the
  `SecurityException`/`EPERM`/`EACCES` cause mapping is unit-proven with negative controls;
  `c2::reachability` stays `AdapterOnly`. Freeze Q5 (inherently device/app-bundle-tier?)
  strengthens.
- **The mutation pass**: 22 behavioural mutations + 1 structural check; **19 caught; 3
  survivors dispositioned** (two hypothesis-2 — behaviourally identical, evidence recorded;
  one recorded non-assertion: upload `total` is unasserted on *every* implementor, F-M4-3 —
  a §7-invariant decision, not a mutation-pass call); **1 genuine blind spot found and
  fixed**: the redirect-trace row never asserted hop *order* — Android reverses OkHttp's
  last-first `priorResponse` chain, and dropping the `.reverse()` survived the whole suite.
  New `WrongHopOrder` assertion + mock control + red-twin, watched red on mock and real
  mutant, green on all four implementors. **MK23 (double-complete) is structurally
  impossible** — it fails to compile (E0382): the one-effect-one-completion invariant is
  type-enforced, exactly as designed. Running totals across the three rounds: **68 mutations
  audited, 61 caught, 4 blind spots found and fixed, all survivors dispositioned.**

## Deviations from the step doc

- The FFI surface needed **zero additions** across M1–M4 (Apple's round was "strictly
  additive"; Android's was empty) — the homogenization seam was already complete.
- M3's HttpEngine probe crashed the instrumentation once (the permission finding) before the
  test-tier manifest was fixed; scope unchanged.
- M4 added one suite row (hop order) — suite strengthening in the established tradition.
- The two agent stalls (M0, M1) cost wall-clock but no correctness: both were resumed with
  context intact and concluded normally under the never-record-unrun rules.

## Decisions recorded by the milestones (review at the freeze)

- FFI bridge crates are per-target **copies** (bindgen reads source text); a drift check or a
  mechanical shared macro-input is the tooling answer, not a shared crate.
- Android packaging convention: consumable Gradle library + sibling conformance project;
  `dist/android` sourced in place (no `wrapper_sources` analog exists, none needed).
- The Swift `FfiError` name reservation is **Swift-specific** (Kotlin generates
  `FfiException`); the lint candidate narrows to the Swift backend.
- `test:android:http` gates on JUnit XML; observed row messages live in retained
  `logcat-*.txt` (the GMD XML carries no `<system-out>`, F-M1-6).
- Test-tier allowances, explicit and shipped-code-free: cleartext-to-loopback manifest flag,
  `TMPDIR` redirect (F-M2-1), `ACCESS_NETWORK_STATE` for the engine probe.

## Friction log (aggregated; freeze-agenda input)

- **F-M0-4 / F-M0-5 (headline, → freeze Q1):** generated-binding drop-on-overflow + the
  native-side, GC-surviving subscription lifecycle — see Results.
- **Redirect ceiling** (F-M1-1/F-M1-2): OkHttp has no honest limit source either, and signals
  exhaustion only via exception *text* — the classifier's one unavoidable string match.
  Ceiling-as-CFG (freeze Q2) would remove it on both platforms.
- **`content_length` honesty splits by sink** (F-M1-3/F-M2-3): holds on a third platform.
- **Poll-based `CancelToken` costs a watcher thread** (F-M1-4): third platform; the
  push-cancellation seam (freeze Q4) keeps earning its slot.
- **The file-sink path is Rust-chosen and `/tmp` is unwritable on Android** (F-M2-1): the
  row's path source should be tier-provided — new freeze/harness item.
- **The pin trust manager is rebuilt per request** (F-M2-4): per-request pins vs per-adapter
  anchor are conflated in OkHttp's client model — adapter-internal, recorded.
- **Pin chain-first ordering is not shared-suite-expressible** (F-M4-2): the mock models
  pinning as trust-replacement; the ordering invariant lives in per-adapter unit tests — a
  conformance-scope boundary worth stating at the freeze.
- **A dropped deadline crashes the ART harness** (F-M4-1): leaked unbounded `/stall` calls
  starve the instrumentation; the harness should hard-kill a row rather than lean on the 5s
  `recv_timeout` — harness-hardening item.
- **The TestServer speaks only HTTP/1.1** (F-M3-2, plus Cronet's `unknown` protocol string
  F-M3-3): any future engine-matrix or h2/h3 conformance needs an ALPN-capable listener.

## Open questions (→ the contract-freeze design session, with steps 24 + 25's lists)

1. **The streaming seam contract** (Q1, now three-platform-informed): chunk re-entry,
   back-pressure/overflow policy for generated bindings (F-M0-4), end-of-body — and the
   **scope-/Drop-bound subscription lifecycle**, demanded independently by Apple (F-M3-1) and
   ART (F-M0-5) with different failure shapes.
2. Redirect ceiling as CFG at the composition root (now also removes the last text-match).
3. `content_length` semantics per sink kind (third platform, same split).
4. A push-cancellation seam on the capability trait (third platform paying the poll thread).
5. `PermissionDenied`: inherently device/app-bundle-tier? (Two platforms gated the same way.)
6. `HttpError → Into<ErrorData>` bridge (v1.14 residue, unchanged).
7. Adapter packaging conventions as `bolted new` scaffolding rules — now with both the SwiftPM
   and Gradle shapes proven; plus the FFI-bridge-crate drift check (F-M0-1).
8. Conformance-scope boundary: which invariants are shared-suite obligations vs per-adapter
   unit obligations (F-M4-2), and should row 11 assert upload `total` (F-M4-3)?

## Next

The **contract-freeze design session** (v1.15 scheduling — Android was the last reachable
implementor; agenda = the eight above merged with steps 24/25's remaining items). Then S-WIN
still waits on upstream finding 07 (owner files; description ready). The Pixel 8a
physical-device pass and any engine-matrix work are non-gating follow-ups.
