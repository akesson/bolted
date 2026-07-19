# Step 26 M4 — the mutation pass (Android OkHttp adapter)

**Milestone:** M4 (the mutation pass against the Android adapter — the fourth conformance implementor).
**Branch:** `step/26-android-adapter`. Scope: mutate the M2 syntheses + classifications + the FFI
bridge token routing, one mutation at a time (or in independent, distinctly-attributable batches),
prove each is caught or discharge it under the two-hypotheses rule, fix any genuine blind spot with a
committed row watched red first. **The mutations are NOT committed** — the shipped adapter
(`android/bolted-http/.../BoltedHttp.kt`) and the FFI crate (`crates/bolted-http-android-ffi/src/lib.rs`)
end with **zero** diff (verified with `git diff`); only the one suite-strengthening (a `WrongHopOrder`
row assertion + a mock knob + its red-twin) is committed.

The full round table (site / mutation / caught-by-or-survived / typed reason) lives in
`crates/bolted-http/docs/conformance-mutation-table.md` (the "Android adapter — step 26 M4" section),
mirroring the Linux + Apple rounds. This file records the run discipline, the survivor evidence, the
blind-spot fix, and the friction log.

## Gate result

- **Baseline** (before any mutation): `mise run test:android:http` — 14/14 green on the headless `dev34`
  GMD (aosp_atd android-34 arm64), `tests="14" failures="0"` (JUnit XML). ~2m40s per Kotlin-only run.
- **Final clean run** (all mutations reverted; the `WrongHopOrder` suite-strengthening present):
  `mise run test:android:http` — **14/14 green**, and the shipped sources (`BoltedHttp.kt`, `lib.rs`)
  have **zero** diff vs HEAD (`git diff --stat` empty for both).
- `mise run check` — green (mock red-twins incl. the new `redirect_trace_red_when_hops_reordered`, the
  correct mock still green, the Linux reqwest c1 suite green with the new assertion).
- `mise run test` — green (full Rust gate incl. the reqwest adapter over the strengthened row).
- `mise run test:apple:http` — green (Apple passes the strengthened row).

## Run discipline (how every result was observed)

Every GMD invocation ran **synchronously in the foreground**, output to a log file, gated on the JUnit
XML (never the wrapper exit code — memory `test:android exit code masks failures`). Row outcomes were
read from `theFullSuiteIsGreenOnTheRealAdapter`'s on-device logcat, which prints `M2 [RED] <row> —
<reason>` for **every** row before the assertion trips (so a single failing run yields the complete
per-row picture), plus the failing `@Test` names from the XML. Independent mutations reding **distinct**
rows were batched into one run and attributed by row (each row's typed reason pinpoints its mutation);
pin-conflation and cancel/deadline mutations (which touch shared rows) were run one at a time.

Runs: baseline; K1 (7 distinct-row mutations); K2 (MK1); K3 (MK2); K4 (MK8+MK6, distinct rows); K5
(MK9); K6 (MK7); S1 (the 4 expected-survivors together → 14/14 green confirms all four survive); R1
(MK19+MK21, distinct rows, Rust rebuild); R2 (MK22, Rust rebuild); MK23 (`cargo check`, compile-only);
the blind-spot fix (`check`/`test`/`test:apple:http`); the AD7/MK13 red-watch (Rust rebuild + Kotlin);
the final clean run.

## Mutation round — result summary

**22 behavioural mutations + 1 structural (MK23).** 19 behavioural caught, MK23 compile-enforced, 3
survivors dispositioned. Full table + reasons: the mutation-table doc. Highlights:

- **Pin split (the "sharpest M2 target"):** corrupt leaf-SPKI (MK1 → rule-10 `ExpectedSuccessGotError`),
  `PinMismatch⇒Tls` (MK2 → rule-10 + key-pin-mismatch `WrongErrorKey`), `Tls⇒PinMismatch` vice-versa
  (MK3 → key-tls `WrongErrorKey`), require-all pins (MK4 → the N3 split unit test) — all caught. The
  drop-chain-first-ordering mutation (MK5) **survived**, hypothesis 2 (see below).
- **Deadline:** `callTimeout⇒readTimeout` per-idle regression (MK6) caught by the `/drip` row
  (`row-deadline-total-not-per-idle` + the M1 total-deadline test) — the per-idle regression the /drip
  row exists to catch; drop-the-deadline (MK7) caught by the M1 total-deadline test.
- **Cancel:** leak-as-Transport (MK8) and cancel-as-Timeout (MK9) both caught (rule-09/key-cancelled;
  MK9 also rule-02 `KeysNotDistinct`).
- **Redirects:** truncate hops (MK12 → `WrongHopTrace`), break too-many classification (MK11 →
  key-too-many-redirects `WrongErrorKey`) caught; **reorder hops (MK13) survived → fixed** (see below).
- **File sink:** skip rename (MK14 → row-15 `WrongSink`), Memory-for-File (MK16, pinned via `WrongSink`),
  swallow write failure (MK17 → key-io `ExpectedErrorGotSuccess`) caught; buffer-whole-body (MK15)
  **survived**, hypothesis 2.
- **Version:** fixed-wrong-version (MK18) caught by `row-negotiated-version-observable` — the MA18 lesson
  holds on Android (the version-observable row created in step-25 M4 catches it).
- **Content-length:** dishonest length (MK19, FFI `to_http_response`) caught by rule-07
  `DishonestContentLength`. (Android structurally cannot report the *compressed* figure — OkHttp strips
  it and the adapter forwards decoded bytes; the bridge derives the length, so the honest analog is a
  bridge `+1`. F-M1-3 / M2 note.)
- **Bridge token routing (2 sites):** wrong-token progress (MK21 → rule-11 `ProgressNotTerminal{0}`),
  wrong-token completion (MK22 → every success row `NoCompletion`) caught. **Double-complete (MK23) is
  structurally impossible** — `CompletionSink::complete(self: Box<Self>)` consumes the sink and
  `take_pending` removes-and-returns the single-flight entry, so a second completion cannot be written:
  `cargo check` fails `E0382: use of moved value: pending.completion`. The single-flight guarantee is
  type-enforced (the finding the step doc asked for).

## Survivors — two-hypotheses discharge

- **MK5 (drop chain-first ordering) → hypothesis 2 (vacuous).** The order matters only for a non-matching
  pin against a chain-*failing* cert; no fixture constructs that (every pinned row targets the good
  cert; key-tls carries no pins). On the good cert both orders are identical. No row added — such a row
  is not shared-suite-expressible: the socket mock models pinning as trust-**replacement**
  (`netmock.rs`: pins present ⇒ the pin set *is* the anchor), so a "pinned request to an untrusted cert
  ⇒ Tls" row would break the correct mock. Adapter-local invariant, recorded.
- **MK15 (buffer whole body) → hypothesis 2 (unobservable).** File contents identical; the in-process
  suite reads the file back and sees the same bytes. Streaming-vs-buffering is a memory-footprint
  guarantee, not a suite-observable correctness one (mirrors reqwest A4b/A6). No test added.
- **MK20 (fake the upload total) → recorded non-assertion, not a shared blind spot.** `judge_progress`
  (rule 11) judges monotonicity of `sent` + terminal `sent` == body length, and **ignores `total`** —
  deliberately and uniformly (§5.9 / row 14: "indicative, monotone, not wire-truth"; `total` is an
  `Option` hint every implementor forwards and no row asserts). Observably different but pins a property
  the progress contract does not cover on any implementor. Adding a total-accuracy assertion expands the
  row-11 contract — an ARCHITECTURE §7-invariant decision for a design session, not a mutation pass.
  Recorded, no row added (mirrors Apple leaving File-sink `content_length` unasserted with rationale).

## The blind spot fixed — redirect hop **order** (committed)

MK13 (drop `hops.reverse()` in `redirectHops`) reports the hops in **reverse** order — right count (2),
right tail (`n=0`), wrong order — and **survived the whole suite** (S1 ran 14/14 green with it applied).
The redirect-trace row asserted count + `final_url` but **nothing referenced hop order**. Hypothesis 1:
the correct adapter (and mock, reqwest, Apple) reports `[.../n=2, .../n=1]` (traversal order) for
`/redirect-chain?n=2`; the mutant reports `[.../n=1, .../n=2]`. Hop traversal order is an already-
documented observable (§5.5, netmock "first hop first"), so asserting it strengthens the suite for a
shipped property (not a contract expansion — unlike MK20).

**Committed suite-strengthening (3 files, all in `crates/bolted-http/src/conformance/`):**

- `mod.rs`: new typed `FailureReason::WrongHopOrder`.
- `c1.rs`: `redirect_trace_correspondence` now asserts `hops[0]` contains `n=2` and `hops[1]` contains
  `n=1` (traversal order) → `WrongHopOrder`; + the mock red-twin `redirect_trace_red_when_hops_reordered`.
- `netmock.rs`: new `MockBehavior::honest_redirect_hop_order` knob (default `true`); off ⇒ `hops.reverse()`
  on the honest-trace path.

**Watched red two ways, green on all four implementors:**

- mock red-twin (`honest_redirect_hop_order = false`) ⇒ `WrongHopOrder` (in `mise run check`).
- real Android mutation (MK13 re-applied, new assertion present) ⇒ `C1/row-redirect-trace-final-url-and-hops
  — WrongHopOrder` (exactly one row red on the GMD).
- correct mock + correct reqwest (`mise run check`, `mise run test`) + Apple (`mise run test:apple:http`)
  + the real Android adapter (final clean `test:android:http`, 14/14) all pass the strengthened row green.

## Friction log (freeze-agenda input)

- **F-M4-1 — dropping the deadline entirely crashes the ART instrumentation (uncatchable).** MK7 (no
  per-`Call` timeout) leaves the `/stall` OkHttp calls hanging past the driver's 5 s budget; the leaked
  calls / late completions took the instrumentation down with `Process crashed` **after 5 tests**, so
  `theFullSuiteIsGreenOnTheRealAdapter` never ran — the `/stall` C1/C2 timeout rows were not observable
  for MK7. The catch is still observed (the M1 total-deadline test's `/drip` Total arm reds), and the
  deadline synthesis is independently pinned by MK6 (per-idle) on the real adapter. Same shape as
  **F-M3-1** (HttpEngine init crash) and the Apple deadline-NoCompletion handling (step-25 M4, run under
  a pseudo-tty): **a native adapter that drops the deadline is a *destabilising* fault on the ART tier,
  not a clean red** — freeze input for how the harness should bound a completely-unbounded adapter (a
  hard per-row wall-clock kill, not just the 5 s `recv_timeout`, so a leaked call cannot outlive its row).
- **F-M4-2 — the pin-ordering (chain-first) invariant is not shared-suite-expressible (MK5).** The
  socket mock models pinning as trust-replacement, so the "chain failure wins over pin mismatch"
  ordering the *adapters* all implement cannot be pinned by a shared C1 row without breaking the mock.
  It is an adapter-local invariant across Linux/Apple/Android, currently unpinned by conformance.
  **Freeze input:** if the pin-vs-trust ordering is to be a *contract* guarantee, the mock needs a
  separate chain-validity axis (a cert that fails the chain *and* carries a pin), which today it does
  not model.
- **F-M4-3 — the upload-progress `total` is unasserted on every implementor (MK20).** row-11 judges
  `sent` only; `total` (an `Option` hint) is forwarded but never checked. Faking it survives on all four
  adapters. **Freeze input:** whether the progress contract should assert `total == Some(body_len)` when
  the length is known is an open row-11-contract (§7 invariant) question — deliberately left to a design
  session, not resolved here.
- **F-M4-4 — batching relies on the full-suite logcat printing all rows before the assertion.** The
  attribution mechanism is `theFullSuiteIsGreenOnTheRealAdapter`'s pre-assertion `record()` loop; it is
  robust because the driver has bounded budgets on every row (no hang → every row returns a typed
  `RowResult`). The one exception is F-M4-1 (a crash truncates the log) — which is exactly why the
  deadline-drop mutation was run alone.

## M5 hand-off (the report)

- The mutation pass is complete: **19 caught + MK23 structural (compile-enforced), 3 survivors
  dispositioned (2 hypothesis-2, 1 recorded non-assertion), 1 genuine blind spot fixed** (redirect hop
  order → `WrongHopOrder`). The Android adapter is the fourth implementor with a real mutation pass.
- The **only** committed code change is the suite-strengthening (`WrongHopOrder` + the mock knob + the
  red-twin) in the shared `bolted-http` conformance module — it runs against **all four** implementors
  and is green on each. `mise run check` / `mise run test` / `mise run test:apple:http` /
  `mise run test:android:http` all green. Shipped adapter + FFI crate: zero diff.
- **Feature-matrix Android row statuses updated** (§4, an "Android / OkHttp leg proven (step 26 S-AN)"
  block mirroring the Apple one): rows 4, 6, 7, 14, 15, 19 syntheses pinned; rows 11, 12, 18 recorded.
- Three freeze-agenda items raised (F-M4-1 harness-bounds-an-unbounded-adapter; F-M4-2 pin-ordering not
  shared-suite-expressible; F-M4-3 progress `total` unasserted) — all recorded for the freeze, none
  resolved unilaterally.
