# Step 25 — report: bolted-http II, the Apple adapter (S-AP)

**Status: done — no kill criteria hit; the suite grew two rows catching real blind spots.**
Third step under the Fable-orchestrates model; five Opus sub-agent milestones (M0–M4; M4
stalled once mid-pass waiting on its own background run and was resumed), report by the
planning session. All work on `step/25-apple-adapter` (M0 `a898750` → M4 `b5e79b9`), merged
to `design/bolted-http`. macOS host tier; the iOS device tier stays a non-gating follow-up.

## What was built

- **The harness bridge** (`crates/bolted-http-apple-ffi`, workspace member): the `Http`
  capability as a BoltFFI callback trait (`HttpAdapter`), `HttpHarness` (server lifecycle,
  token-keyed completion re-entry, cancel forwarding, progress re-entry), structured row
  drivers (`run_c1`/extra/`run_c2`/`run_c3` returning per-row pass/fail + typed message),
  and the A1 probe surface (`#[ffi_stream]` chunk stream). It is the conformance bridge,
  not the shipped consumer story.
- **The adapter** (`apple/bolted-http`, bundled SwiftPM pack output): `BoltedHttp.swift`,
  delegate-driven URLSession. Total deadline synthesized via `DispatchSourceTimer` racing
  the whole chain (never the per-idle `timeoutInterval` — the A3 hazard); cancel-vs-timeout
  classified by cause; full `URLError` → typed-key mapping; `didSendBodyData` progress with
  a load-bearing terminal top-up; SPKI pinning in a task-level trust delegate with the
  Linux split (real `SecTrustEvaluateWithError` chain+hostname, then pins; trust failure ⇒
  `Tls`, mismatch ⇒ `PinMismatch`); leaf SPKI via a structural DER walk (no OS API exists;
  a `SecKey` round-trip drops the AlgorithmIdentifier wrapper); https→http refusal + hop
  trace in `willPerformHTTPRedirection`; `downloadTask` file sink with synchronous persist
  inside the delegate callback and atomic temp+rename; real `HttpVersion` from
  `URLSessionTaskMetrics`; `Metrics` tier `Phase`; priority hint → `task.priority`.
- **The consumer test package** (`apple/bolted-http-conformance`): the bundled layout
  regenerates its own `Package.swift`, so tests live in a sibling package — resolving the
  packaging report's open "where do app-added targets go?" question.
- **Tooling**: `pack:apple:http` / `test:apple:http`, `test:apple` extended; `setup:boltffi`
  now rejects a git-installed CLI (`?rev=` in `cargo install --list`) and force-reinstalls
  from the registry — the version string alone cannot tell the killed step-23 build apart.

## Results

- **Full conformance green on the real adapter**: 15 C1 rows (13 + M4's two), 10 C2 keys,
  C3 Apple column pinned (`PriorityHint` present, `Metrics(Phase)`). **Every row watched
  red first** (M0's gate proved the bridge itself can fail legibly before any green counted).
- **A1 — the step-24 F1 streaming verdict holds on Apple**: 200/200 chunks, ordered,
  lossless, complete, both pacings, consumer always off-main (p50 ≈ 25µs, p99 ≈ 200µs),
  stable under full 14-core saturation; the corruption control detects a single dropped
  chunk. Kill criterion 3 not hit.
- **A6 — no divergence** under `usesClassicLoadingMode = false` (all rows, C3, and the A1
  probe identical). The flag is macOS 15.4+/iOS 18.4+ and silently no-ops below.
- **A5 — non-vacuous acceptance**: a `.high` request's task carries 0.75, not the 0.5
  default; the contract's 5 levels fold onto URLSession's 3 named buckets.
- **The mutation pass**: 20 mutations (syntheses, classifications, two FFI-bridge
  token-routing sites); 18 caught; **2 survivors, both genuine blind spots (hypothesis 1),
  both fixed** with rows watched red first on the mock and confirmed against the real
  mutation on all three implementors:
  - **MA6**: per-idle timeout masquerading as the total deadline passed the whole suite —
    `/stall` bursts-then-stalls, so a per-idle timer fires at ≈ the deadline. New `/drip`
    trickle endpoint + `row-deadline-total-not-per-idle`; the trickle is the only fixture
    shape that pins "total".
  - **MA18**: a wrong negotiated version passed because **no row on any implementor read
    `version()`** — the observable had shipped without a positive control since M1.5. New
    `row-negotiated-version-observable` + mock `honest_version` control.
- **`PermissionDenied` (step-24 Q4): platform-gated on the host tier, recorded with
  evidence, not faked.** No hermetic denial is reachable from a non-sandboxed SwiftPM
  XCTest; `c2::reachability` already classifies it `AdapterOnly`; the POSIX-cause mapping
  is unit-proven with negative controls. Open whether the key is inherently
  device/app-bundle-tier.

## Deviations from the step doc

- Rule 5 (manual 304) came green in M1 — the ephemeral session produces a real 304, as the
  spike plan predicted; M2's list shrank by one.
- The `setup:boltffi` hardening was pulled into M1 from M0's friction (F-M0-4) — tooling,
  not scope creep.
- M4 added two suite rows (blind-spot fixes) — suite strengthening in step 24's
  redirect-trace tradition, not contract change; `SINK_ROWS` grew 2→4.

## Decisions recorded by the milestones (review at the freeze)

- **Shipped-adapter packaging convention**: bundled package + sibling conformance/test
  package (the bundled `Package.swift` is pack-owned). Bundled `output` with `..` works.
- **Never name a `#[data]` type `FfiError`** — reserved by BoltFFI's throwing error style;
  fails only at `swift test` ("ambiguous for type lookup"). Candidate upstream/`bolted new`
  lint.
- Task-level (not session-level) trust delegate — only it can read per-request pins.
- `content_length`: `Some(len)` for the Memory sink, `None` for the File sink — honesty
  currently splits by sink kind.
- `TooManyRedirects.limit` = sentinel `0` — URLSession's ceiling is internal.
- FFI surface stayed strictly additive across M1–M4 (request mirrors, error variants,
  drivers); the contract crate is untouched.

## Friction log (aggregated; freeze-agenda input — the point of running Apple before the freeze)

- **F-M3-1 (headline, → freeze Q2):** the `ffi_stream` consumer needs explicit teardown —
  a stalled consumer leaks a dead subscription into the shared streaming runtime and
  starves the *next* run. Never lost-in-transit; purely re-delivery lifecycle. The
  streaming core seam must specify a scope-bound (`Drop`-bound) subscription lifecycle.
- **Redirect ceiling** (F-M1-1/F-M2-4): URLSession has no honest limit source — should the
  ceiling be composition-root CFG (as Linux's effectively is)?
- **`content_length` honesty splits by sink** (F-M1-3/F-M2-3): the streaming sink (row 16's
  contract surface) faces the same question with no in-memory body.
- **Poll-based `CancelToken` costs a thread per request** (F-M1-4): a push-cancellation
  seam on the capability would help every native adapter.
- **Cause-over-shape classification is load-bearing in four places** (F-M2-5): refusal,
  pinning, deadline, cancel all depend on classifying by cause, not error shape.
- URLSession retains its delegate (F-M1-7 — hidden by the FFI-owned lifetime); leaf-SPKI
  DER extraction is hand-rolled (F-M2-2 — shared-helper candidate); `didSendBodyData` is
  sparse, so the terminal progress top-up is load-bearing (F-M4-3); `/stall` cannot pin
  "total deadline" — only a trickle fixture can (F-M4-1); the version observable had no
  control anywhere until M4 (F-M4-2 — the "a rule is only as checkable as its observables"
  lesson, second edition).

## Open questions (→ the contract-freeze design session, with step-24's list)

1. The streaming seam contract: chunk re-entry, back-pressure, end-of-body, **and now the
   subscription lifecycle (F-M3-1)** — the step's sharpest new input.
2. Redirect ceiling as CFG at the composition root?
3. `content_length` semantics per sink kind (contract wording, before the streaming sink).
4. A push-cancellation seam on the capability trait (vs the poll thread)?
5. `PermissionDenied`: inherently device/app-bundle-tier? (Its control would land in the
   iOS device tier or S-AN.)
6. `HttpError → Into<ErrorData>` bridge (the v1.14 residue).
7. Adapter packaging convention (sibling test package) as a `bolted new` scaffolding rule?

## Next

The **contract-freeze design session** (agenda: the seven above + step-24's remaining
items), per the v1.14 scheduling decision — Apple was the last implementor before the
freeze. Then step 26 (S-AN, Android — opens with the JNI stream probe). The iOS device
tier is a non-gating follow-up (A7 background family stays deferred with it). S-WIN still
waits on upstream finding 07 (owner files; description ready).
