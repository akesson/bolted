# Step 25 M2 — the syntheses (notes for M3+)

**Milestone:** M2 (A2 file sink, A4 pinning + hop trace, rule-4 https→http refusal, `PermissionDenied`
control, C3 Apple column). **Branch:** `step/25-apple-adapter`. Scope: the FULL suite green on the
real `BoltedHttp` adapter — all 13 C1 rows (11 `rows()` + 2 `extra_rows()`), every C2 key, the pinned
C3 Apple column. Every newly-green row watched red first. M3 (A1 streaming, A6 classic-loading-mode
sweep, A5 priority acceptance) and M4 (mutation pass) are untouched.

## Gate result

- `mise run check` — green (host, Xcode-free; workspace clippy `-D warnings`, the apple-ffi crate
  clean, all crate tests pass).
- `mise run test:apple:http` — green: **5 XCTest methods, 0 failures**.
  - `testC1Rule01IsGreenOnTheRealAdapter` / `…IsRedWithABrokenAdapter` — the M0 bridge fail-ability
    gate, retained.
  - `testFullSuiteIsGreenOnTheRealAdapter` — the real adapter over C1 + extra_rows + C2: **all 23
    driver rows green, none skipped**, plus the pinned C3 Apple column.
  - `testWatchedRedBaseline` — every green row shown RED first (BrokenHttp reds all but the two
    Transport-expecting rows; AlwaysOkHttp reds those two).
  - `testPermissionDeniedMapping` — the EPERM→key mapping proven (positive + two negative controls).

## Row status table (real adapter — the M2 rows, watched red first)

| Row | M1 status | M2 status | Recorded RED (watched-red evidence) |
|-----|-----------|-----------|-------------------------------------|
| C1/rule-04 https-to-http-refused | RED | **GREEN** | broken: `WrongErrorKey { expected: InsecureRedirect, got: Transport }` |
| C1/rule-10 pin-mismatch-typed-error | RED | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` (its positive good-pin leg errors first) |
| C1/row-15 response-sink-correspondence | not wired | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C1/row-redirect-trace-final-url-and-hops | not wired | **GREEN** | broken: `ExpectedSuccessGotError { got: Transport }` |
| C2/key-pin-mismatch | RED | **GREEN** | broken: `WrongErrorKey { expected: PinMismatch, got: Transport }` |
| C2/key-insecure-redirect | RED | **GREEN** | broken: `WrongErrorKey { expected: InsecureRedirect, got: Transport }` |
| C2/key-io | RED | **GREEN** | broken: `WrongErrorKey { expected: Io, got: Transport }` |

All 16 M1-green rows stayed green (see M1 notes for their watched-red evidence). No row skips.

**C3 Apple column** (pinned in `testFullSuiteIsGreenOnTheRealAdapter`, generated from the capability
traits — `SwiftAdapter: PriorityHint` marker + `Metrics { tier = Phase }`):

```
capability     | presence
---------------+-----------------------
priority-hint  | present
metrics        | present (Phase)
```

## What M2 built

**FFI crate (`crates/bolted-http-apple-ffi/src/lib.rs`)** — additive mirror growth only:
- `FfiPin { hash: Vec<u8> }` + `FfiRequest.pins: Vec<FfiPin>` — the request's `PinSet` crosses now.
- `FfiResponseSink { Memory | File { path } }` + `FfiRequest.sink` — the row-15 response-sink selector.
- `FfiResponse.hops: Vec<String>` (the redirect trace) + `FfiResponse.sink_path: String` (empty ⇒
  a `Memory` outcome carrying `body`; non-empty ⇒ a `File` outcome at that path, `content_length`
  reported as `None`).
- `FfiHttpError` grew `PinMismatch`, `InsecureRedirect { to }`, `Io`, `PermissionDenied` — each maps
  to its `HttpError` variant (the `InsecureRedirect` target re-types via `cleartext_dev`, falling
  back to `Transport` on the impossible parse failure rather than an `unwrap`).
- `HttpHarness::run_extra_rows()` (drives `c1::extra_rows()`) and `run_c3()` (renders
  `c3::divergence(SwiftFactory)`).
- `SwiftAdapter` implements `PriorityHint` (marker) + `Metrics` (tier `Phase`); `SwiftFactory`'s
  `priority_hint()` / `metrics()` self-report `Some(..)` — the type-checked C3 seam.

**Swift adapter (`apple/bolted-http/Sources/BoltedHttp/BoltedHttp.swift`)**:
- **The pinning SPLIT** (rule 10, mirrors the Linux `PinningVerifier`): the trust delegate moved to
  the TASK level (so it can read the request's pins). (1) a real chain + hostname evaluation against
  the installed anchor via `SecTrustEvaluateWithError`; (2) only on a PASSING chain, when pins are
  present, compare SHA-256 over the leaf's SubjectPublicKeyInfo. Trust/hostname failure ⇒
  `performDefaultHandling` ⇒ `Tls`; pin mismatch ⇒ `.cancelAuthenticationChallenge` + the
  `.pinMismatch` cause ⇒ `PinMismatch`. Never conflated.
- **Leaf SPKI extraction** is a minimal structural DER walk of the certificate
  (`Certificate → tbsCertificate → …[6th field] = subjectPublicKeyInfo`), hashing the full SPKI TLV —
  matching `x509_parser`'s `public_key().raw`. Deliberately NOT reconstructed from a `SecKey`
  (`SecKeyCopyExternalRepresentation` drops the SPKI's `AlgorithmIdentifier` wrapper and is key-type
  specific — the classic pinning gotcha).
- **`willPerformHTTPRedirection`** (rules 4/7): an `https → http` downgrade sets the
  `.insecureRedirect(to:)` cause and does not follow (`completionHandler(nil)`), surfacing
  `InsecureRedirect`; a permitted redirect records the issuing URL as a hop, then follows. The
  synthesized total-deadline timer already spans the whole chain (verified: rule-04 + redirect-trace
  both complete well inside budget).
- **The file sink** (row 15 / `Io`): a `File` sink uses a `downloadTask`; `didFinishDownloadingTo`
  persists the temp file **synchronously** (the temp-file-lifetime rule) — a cross-FS move into the
  destination directory then an atomic same-dir rename. A failure (e.g. the `Io` control's
  nonexistent parent dir) sets `.ioFailure` ⇒ `Io`.
- **Synthesized causes win in `didCompleteWithError`**: pin-mismatch / insecure-redirect / io-failure
  are classified by CAUSE before the raw `URLError` shape (and even over an OS-reported success, for
  the file-sink write failure).
- **`PermissionDenied`**: `mapError` inspects the `URLError`'s underlying-error chain for a POSIX
  `EPERM` (a sandbox / local-network denial) and maps THAT to `PermissionDenied` — a genuine mapping,
  never invented.

## Decisions taken (smallest reversible; recorded per the working agreement)

1. **File sink = `downloadTask`, not buffered `dataTask`.** Honors A2's named hazard directly (the
   temp-file-lifetime rule under delegate threading) rather than sidestepping it by buffering. Atomic
   finalize is temp-move-into-dest-dir + same-dir rename, mirroring the Linux sink's temp+rename.
2. **File-sink `content_length` is `None`.** The body is on disk, not in memory, so no honest decoded
   length is available (closes part of friction F-M1-3 for the file case). No row inspects it.
3. **Leaf SPKI via structural DER walk, not `SecKey` reconstruction.** Robust to the cert's key
   algorithm (rcgen currently emits EC P-256); a `SecKey`-prefix approach would be a fragile magic
   ASN.1 header per key type. The DER tag constants (0x30/0xA0) are wire-format, not contract
   constraint literals.
4. **`PriorityHint` implemented as a marker only; A5 acceptance stays M3.** M2's C3 deliverable is the
   column (the capability *declaration* read off the trait impl). The actual priority→`task.priority`
   wiring + its acceptance assertion are A5/M3 — no `FfiRequest.priority` field was added this
   milestone (kept the FFI surface minimal). The marker + `Metrics(Phase)` are what C3 reads.
5. **`PermissionDenied` live host control is platform-gated; its positive control is the MAPPING.**
   See the friction log — no hermetic EPERM-producing `URLError` is reachable on the macOS SwiftPM
   test host. So the load-bearing, non-vacuous proof is `testPermissionDeniedMapping`
   (`permissionKeyForPOSIX(EPERM) == .permissionDenied`, plus `ECONNREFUSED`/`ETIMEDOUT` ⇒ `nil`
   negative controls). `c2::reachability` already classifies the key `AdapterOnly`, so it is (still)
   correctly absent from `c2::rows()` — the C2 driver suite is complete without it.
6. **Task-level trust delegate replaces the session-level one.** Server-trust challenges route to the
   task-level method when implemented, and only the task level can read the per-request pins.

## Friction log (freeze-agenda input — friction matters more than usual this step)

- **F-M2-1 — `PermissionDenied` has no hermetic host control on the macOS host tier.** The genuine
  causes (Apple Local Network privacy prompt; an App-Sandbox network denial → EPERM) need a GUI or an
  entitlement-signed sandboxed app bundle — both non-gating for this step (the iOS device / app-bundle
  tiers). A plain SwiftPM XCTest executable is not sandboxed and cannot make the OS deny a request, so
  no `URLError` it produces carries EPERM. **Evidence:** cleartext to `127.0.0.1` loads freely under
  the test host's ATS (F-M1-9); closed-port / bad-DNS produce `ECONNREFUSED` / DNS codes, not EPERM.
  **Recorded, not faked** — the adapter's EPERM mapping is proven at the unit level instead. Freeze
  question: is `PermissionDenied` inherently a device/app-bundle-tier key with no host control on ANY
  platform, and should the matrix mark it so uniformly (it is already `AdapterOnly`)?
- **F-M2-2 — leaf-SPKI extraction is hand-rolled DER on Apple.** Apple exposes no API for the SPKI
  DER of a `SecCertificate` (only the full cert DER, or a `SecKey` that omits the algorithm wrapper),
  so honest SPKI pinning requires a small ASN.1 walk. It is ~40 lines and structural, but it is
  adapter-carried crypto plumbing that every Apple pinning consumer would re-need. Worth a shared
  helper (or a note that BoltFFI/`bolted` should ship one) so each native adapter does not re-derive
  it. Contrast: Linux gets it free from `x509_parser`.
- **F-M2-3 — `content_length` honesty now splits by sink.** Memory ⇒ `Some(decoded len)`; File ⇒
  `None`. This resolves the file-sink half of F-M1-3, but the streaming sink (row 16, M3/freeze) will
  face the same question with no in-memory body AND no completed file — the honest answer there is
  likely `None` or an `expectedContentLength` filtered by `Content-Encoding`. Flagged for the
  streaming decision.
- **F-M2-4 — URLSession's redirect cap is still internal/unexposed** (carries forward F-M1-1). The
  hop trace and https→http refusal are now adapter-driven in `willPerformHTTPRedirection`, but the
  chain *ceiling* is still URLSession's own (yielding `httpTooManyRedirects` ⇒ the sentinel
  `TooManyRedirects { limit: 0 }`). The delegate could count hops and enforce a request/CFG limit, but
  the contract carries no redirect limit — same freeze question as F-M1-1 (redirect ceiling as a
  composition-root CFG; should `TooManyRedirects` carry `limit` at all if a platform can't report it).
- **F-M2-5 — `willPerformHTTPRedirection` refusal relies on cause-over-shape.** Refusing with
  `completionHandler(nil)` (not `task.cancel()`) lets the ignored 3xx complete as a "success"; the
  adapter is correct only because `didCompleteWithError` reads the `.insecureRedirect` cause before
  the response. Robust here, but it is the same by-cause-not-by-shape discipline rule 2 needs — worth
  noting the pattern is load-bearing in three places now (deadline, cancel, redirect refusal, pin).

## M3 / M4 hand-off

- **A1 streaming** (M3): probe-grade, `bytes(for:)` chunking through the S-FFI mechanism; verdict
  paragraph for the freeze. Kill criterion 3 (stall/reorder) applies.
- **A6 classic-loading-mode sweep** (M3): re-run the cluster with `usesClassicLoadingMode = false`;
  record divergence. The current session uses the default loading mode.
- **A5 priority acceptance** (M3): wire `FfiRequest.priority` → `URLSessionTask.priority` and assert
  acceptance-only (the marker trait + C3 column are already in; only the behavior + its assertion
  remain). RFC 9218 wire observation stays FLAGGED lore — do not conformance-test the wire.
- **M4 mutation pass**: mutate the M2 syntheses — pin check (swap the leaf-SPKI compare / drop the
  chain-first ordering), the hop trace (drop a hop), the redirect refusal (follow the downgrade), the
  file-sink atomic finalize (skip the rename) — two-hypotheses discipline on every survivor.
