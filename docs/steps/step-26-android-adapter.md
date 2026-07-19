# Step 26 — bolted-http III: the Android adapter (S-AN)

**Phase 4 · status: ready · branch: `step/26-android-adapter` off `design/bolted-http`.**
Spike-plan §4 (S-AN). The gating tier is the **instrumented ART tier** (`test:android`-style
Gradle-managed emulator, aosp_atd android-34 arm64 — no GUI session needed); a physical-device
(Pixel 8a) pass is a non-gating follow-up. This is the **last implementor before the contract
freeze** (re-scheduled at ARCHITECTURE v1.15: freeze **after** this step) — friction found here
is freeze-agenda input, and the **N2 JNI stream probe's verdict feeds the freeze's headline
question directly** (the streaming seam + F-M3-1 subscription lifecycle). The friction log
matters more than usual, again.

Read first: `crates/bolted-http/docs/feature-matrix.md` (§7 rules, the row classes),
`crates/bolted-http/docs/spike-plan.md` §4, `crates/bolted-http/docs/architecture.md`
(adapter placement, callback-trait topology), **`crates/bolted-http-apple-ffi/src/lib.rs` and
`docs/steps/step-25-report.md`** (the proven harness-bridge shape and the packaging
convention — mirror them, do not rediscover), `docs/steps/step-24-report.md` (what the
harness proves, decisions taken), `android/profile-probe/` + the `pack:android`/`test:android`
mise tasks (the proven Kotlin/JNI packaging + GMD recipe).

## Decisions already taken (do not re-litigate)

- Adapters are shell-side; the capability crosses the FFI as a BoltFFI **callback trait**
  (architecture.md §3). The Apple bridge (`bolted-http-apple-ffi`) is the reference topology:
  token-keyed completion re-entry, structured `RowReport` drivers, server lifecycle export.
  Mirror its shape; deviations are report material.
- **Priority hint is CAP and OkHttp legally ignores it** (row 12): the Android adapter does
  **not** implement `PriorityHint` on the OkHttp path. The hint data still rides every request.
  (If N5 finds HttpEngine real and honoring, record it — don't implement engine priority in
  this step.)
- Row 16 mechanism is `ffi_stream` async push (F1, step-24 verdict; held on Apple, step 25).
  Its core-seam contract surface is **deliberately unfrozen** (freeze agenda Q1); the N2 probe
  here is probe-grade — findings feed the freeze, no contract surface is added.
- Errors are typed enums projecting to key+params data (ARCHITECTURE v1.14/v1.15). No
  `Into<ErrorData>` bridge in this step — freeze agenda.
- boltffi pin: **registry 0.27.5 only**; `setup:boltffi` now auto-rejects a git-installed CLI.
- Packaging convention (step-25 decision, review at the freeze): bundled package + **sibling
  conformance/test package**. Kotlin edition: the pack output is the consumable artifact;
  instrumented tests live in a sibling Gradle project, not inside the pack output.

## Scope

One new Rust FFI crate + one new Kotlin package pair + tier wiring:

- **`crates/bolted-http-android-ffi`** — the FFI surface, mirroring `bolted-http-apple-ffi`:
  (a) the `Http` capability as a callback trait the Kotlin adapter implements
  (`execute(FfiRequest)` / `cancel(token)`); (b) the conformance driver exports returning
  structured per-row results (`run_c1`/`run_extra_rows`/`run_c2` → `Vec<RowReport>`, `run_c3`);
  (c) test-server lifecycle (`start_server` → `ServerInfo` with the three base URLs + TLS
  material, `stop_server`); (d) completion/progress re-entry (`complete_ok`/`complete_err`/
  `report_progress`); (e) the N2 chunk-stream probe surface (`ffi_stream`, mirroring Apple's).
  Reuse by extraction is allowed if cheap (a shared `bolted-http-ffi-core` helper crate), but
  **only** if it stays mechanical — if sharing forces a design decision, duplicate and record.
- **`android/bolted-http`** — the pack output: `BoltedHttp.kt` (OkHttp adapter, hand-written)
  bundled with the generated bindings per the proven `pack:android` layout.
- **`android/bolted-http-conformance`** — the sibling instrumented-test project: drives the
  conformance rows through the driver on ART (GMD, same recipe as `test:android`).
- **mise wiring** — `pack:android:http` + `test:android:http` following the existing shapes;
  `check` stays host-only and JDK-free. **The task must fail on test failure — verify the exit
  path against the JUnit XML, not the wrapper's exit code** (the `test:android` masking gotcha
  is a known landmine; do not inherit it).

The spike-plan clusters, mapped:

- **N1 — packaging.** The Kotlin/AAR-equivalent of the bundled layout: hand-written
  `BoltedHttp.kt` next to generated bindings, one consumable artifact, sibling test project.
  (Step-05 proved the mechanics for gen-profile; this scopes it to http and a *second*
  package — the same "does the story survive package #2" question step 25 answered for Swift.)
- **N2 — the JNI stream probe (runs FIRST, in M0, right after the bridge gate).** The S-FFI
  chunk check, JNI edition: chunks through `ffi_stream` across JNI into a Kotlin consumer —
  ordered, lossless, complete, both pacings, consumer off-main, numbers recorded. **Explicitly
  probe the F-M3-1 lifecycle question on ART**: what happens to a subscription whose consumer
  stalls or is abandoned — does the dead-subscription starvation reproduce, change shape under
  ART's GC, or disappear? (Caution: a GC-dependent probe needs a control — see inherited
  cautions.) The verdict paragraph is freeze input; step-02's stall ghost is the risk.
- **N3 — pinning, both controls.** (a) Map the request's `PinSet` into the adapter
  (`CertificatePinner` or interceptor-level SPKI check — whichever can express the contract's
  trust-failure ⇒ `Tls` vs pin-mismatch ⇒ `PinMismatch` split; mirror Linux/Apple exactly).
  (b) The fragility controls: prove a custom `TrustManager` makes NSC `<pin-set>` stop
  enforcing (the suite must never silently depend on NSC — this evidence answers the §9
  `<pin-set>` freeze question), and pin the hostname-less 2-arg `checkServerTrusted` landmine
  in a unit test.
- **N4 — transparent-gzip normalization.** `BridgeInterceptor` strips `Content-Length` under
  transparent gzip: prove `content_length()` stays None-or-honest (rule 7). Upload progress via
  request-body sink wrapping (rule 11): monotone, terminally consistent — watch for the
  buffer-jump-to-100% failure mode.
- **N5 — `HttpEngine` feature detection (probe-grade, time-boxed).** API 34 emulator: present?
  h3 negotiable against the test server? If cheap, one conformance row through the engine.
  This decides whether the adapter's engine matrix (OkHttp / HttpEngine) is spike-real or
  paper — record the verdict, do not build the second engine path in this step.
- **N6 — cancellation.** `Call.cancel()` from a non-call thread ⇒ `Cancelled` completion
  (rule 9); an `IOException("Canceled")` must never leak as a network-error key. Deadline:
  OkHttp's `callTimeout` covers the whole call including redirects — if it honestly implements
  the total deadline, no timer synthesis is needed (the `/drip` trickle row from step-25 M4
  will verify "total, not per-idle"; watch it red first like everything else).
- **C2** — the taxonomy on OkHttp; `PermissionDenied` follows the step-25 treatment: positive
  control if reachable on the ART tier, otherwise recorded platform-gated with evidence
  (this feeds freeze Q5 — is the key inherently device/app-bundle-tier?).
- **C3** — the Android column generated from the capability traits: `PriorityHint` absent,
  `Metrics` tier `Phase` (OkHttp `EventListener`), pinned expectations.

## Milestones (one Opus sub-agent each; Fable reviews between)

- **M0 — packaging + the harness bridge + the N2 probe.** The FFI crate, the Kotlin package
  pair (walking-skeleton adapter: exactly one C1 row), structured-result driver, server
  control, mise wiring. **Gate 1:** one row green from the instrumented tier AND a
  deliberately-broken adapter showing the same row red with a legible message. **Gate 2 (N2):**
  the JNI stream probe verdict — ordered/lossless/complete or not, numbers, and the F-M3-1
  lifecycle observation. Kill criteria 2 and 3 are evaluated here, before further investment.
- **M1 — the adapter.** Full `BoltedHttp.kt`: dispatch, one-shot completion, full C2 error
  mapping, deadline via `callTimeout` (or synthesis if the trickle row says otherwise), cancel
  (N6), memory sink, upload progress (N4), version observable (`Response.protocol`), redirect
  hop trace (`priorResponse` chain). Target: C1/C2 green except the syntheses. Every row
  watched red first.
- **M2 — the syntheses.** File sink, N3 pinning + both fragility controls, rule-4 https→http
  refusal, rule-5 304, `PermissionDenied` treatment, N4 gzip honesty, C3 Android column.
  Full suite green.
- **M3 — probes + sweeps.** N5 HttpEngine detection verdict; any remaining N4 edge rows;
  confirm the A1-analog numbers under load (saturation, both pacings) if M0 ran the probe
  lightly.
- **M4 — the mutation pass.** Extend `conformance-mutation-table.md` with the Android adapter:
  mutations across the syntheses (pinning, deadline, trace, cancel, progress, gzip) and the
  bridge's token routing; two-hypotheses discipline on every survivor; suite rows added for
  genuine blind spots, watched red on all implementors.
- **M5 — report** (planning session, not a sub-agent): `step-26-report.md` + ROADMAP; the
  freeze agenda assembled from steps 24 + 25 + 26.

## Kill criteria (real; if hit, stop and report)

1. A C1 rule that cannot pass on OkHttp **without contract change** → stop, report the rule;
   the row gets redesigned in the freeze session, not the adapter bent (spike-plan criterion,
   verbatim).
2. N1 packaging inexpressible in BoltFFI's model for the second Kotlin package → stop; that's
   a design session, not a workaround (spike-plan criterion).
3. The N2 stream stalls or reorders on JNI (step-02's ghost) → stop the probe cluster, record
   shape + numbers per-platform (the S-FFI fallback rule), continue the rest; the verdict
   paragraph then says so honestly — and the freeze's streaming-seam decision inherits it.

## Non-goals (→ elsewhere)

The contract freeze itself (next session — this step only *feeds* it). HttpEngine as a second
engine path (N5 is detection only). Background transfer family. Physical-device tier
(non-gating follow-up). Cookie capability, `Into<ErrorData>` bridge, streaming-seam contract
surface (→ freeze). Anything C# (parked on upstream finding 07).

## Inherited cautions

- Build/test only via `mise run …` tasks; never pipe tier wrappers through `tail`/`head`.
  **The instrumented tier's exit code has masked failures before — gate on the JUnit XML.**
- Verify the CLI before packing: `cargo install --list` must show `boltffi_cli v0.27.5` with
  no `?rev=` (the guard self-heals now, but verify once in M0).
- Watch generated-name collisions (the Swift `FfiError` lesson): if a `#[data]` name collides
  with BoltFFI's generated Kotlin, rename ours and record the lint candidate.
- A GC-dependent probe needs a control: if the N2 lifecycle probe polls a `WeakReference`, the
  poll keeps the object alive — use a `ReferenceQueue` (the ART-GC-probes lesson, verbatim).
- Kotlin: no constraint literals (deadlines/limits come from the effect — no magic numbers in
  the adapter); the adapter maps errors, it never invents keys.
- Rust: edition 2024, clippy `-D warnings`, no `unwrap`/`expect`/`panic!` in library code;
  `bolted-http` stays dependency-free on its default build.
- Stale rust-analyzer/IDE diagnostics are not the committed state — verify by building.
- If a decision is missing here: smallest reversible choice, record it in the report. If
  structural (traits, invariants, ARCHITECTURE): stop, record, leave for the freeze.

## Exit checklist

- [ ] `mise run check` green (host, JDK-free) and the new instrumented task green **verified
      against its JUnit XML**.
- [ ] All C1/C2 rows green on `BoltedHttp.kt`, each watched red first; C3 Android column
      pinned; `PermissionDenied` treatment recorded with evidence.
- [ ] N2 verdict paragraph written (ordered/lossless/complete, numbers, threading, **the
      F-M3-1 lifecycle observation on ART**) — flagged as freeze input.
- [ ] N3 fragility controls recorded (NSC `<pin-set>` evidence → freeze §9 question).
- [ ] N5 HttpEngine verdict recorded (real or paper).
- [ ] Mutation table extended; survivors discharged with the two-hypotheses rule.
- [ ] Feature-matrix Android statuses updated; report + ROADMAP row done.
