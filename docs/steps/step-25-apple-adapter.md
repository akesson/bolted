# Step 25 — bolted-http II: the Apple adapter (S-AP)

**Phase 4 · status: ready · branch: `step/25-apple-adapter` off `design/bolted-http`.**
Spike-plan §3 (S-AP), macOS host tier; the iOS device tier is explicitly non-gating (spike
plan: "when convenient"). This is the last implementor before the contract freeze
(re-scheduled at ARCHITECTURE v1.14: freeze **after** this step) — friction found here is
freeze-agenda input, so the friction log matters more than usual.

Read first: `crates/bolted-http/docs/feature-matrix.md` (§7 rules, the row classes),
`crates/bolted-http/docs/spike-plan.md` §3, `crates/bolted-http/docs/architecture.md`
(adapter placement, callback-trait topology), `crates/bolted-http/docs/spike-packaging-report.md`
(the proven SwiftPM `bundled` layout — follow this recipe, do not rediscover it),
`docs/steps/step-24-report.md` (what the harness already proves on Linux, decisions taken).

## Decisions already taken (do not re-litigate)

- Adapters are shell-side; the capability crosses the FFI as a BoltFFI **callback trait**
  (architecture.md §3; round-trip + packaging proven in `crates/spike-http-ffi`).
- Priority hint is **CAP** (row 12, Henrik) — spike-plan A5's "if the row survives review"
  is resolved: it survives; implement acceptance-only.
- Row 16 mechanism is `ffi_stream` async push (F1) — step-24 verdict. Its **core-seam
  contract surface is deliberately unfrozen** (freeze-agenda Q2); A1 here is probe-grade.
- `HttpError` is a typed enum with `key()`; errors are enums projecting to key+params data
  (ARCHITECTURE v1.14). Do not add an `Into<ErrorData>` bridge in this step — freeze agenda.
- boltffi pin: **registry 0.27.5 only.** The step-23 git pin is killed and parked; nothing
  in this step touches the pin.

## Scope

One new Rust FFI crate + one new Swift package + tier wiring:

- **`crates/bolted-http-apple-ffi`** — the FFI surface: (a) the `Http` capability as a
  BoltFFI callback trait the Swift adapter implements; (b) a conformance **driver** export:
  run the suite's C1/C2/C3 rows against the registered adapter and return structured
  results (pass/fail + message per row — never a bare bool, the Swift test must be able to
  print *why*); (c) test-server lifecycle control (start/stop, ports for the three
  listeners). The suite and server already exist behind `bolted-http`'s `conformance`
  feature — this crate adapts them across the boundary, it does not reimplement rows.
- **`apple/bolted-http`** — the SwiftPM package: `BoltedHttp.swift` (URLSession adapter,
  hand-written) bundled with the generated bindings per the packaging report's layout; an
  XCTest target that drives the conformance rows through the driver.
- **mise wiring** — `pack:apple:http` + `test:apple:http` following the existing
  `pack:apple`/`test:apple` shape; extend `test:apple` to include the new package. `check`
  stays host-only and Xcode-free.

The spike-plan clusters, mapped:

- **A1** — streamed response through the S-FFI mechanism: URLSession `bytes(for:)` →
  chunks across the boundary → a harness-side consumer proves ordered, lossless, complete
  delivery inside a real request. Probe-grade: findings feed the freeze; no contract surface
  is added.
- **A2** — download-to-file: `downloadTask` + synchronous move inside the delegate callback
  → `FileRef` completion; verify the temp-file-lifetime rule under the adapter's threading.
- **A3** — C1/C2 on the real adapter. Named hazards: rule 3 (stalled body vs the
  **synthesized** total deadline — URLSession's per-idle `timeoutIntervalForRequest` must
  not mask it; the request's deadline is the contract, row 2 is CORE(adapter)), rule 5
  (ephemeral `URLSession` ⇒ real 304 for manual `If-None-Match`), rule 4 (https→http
  refusal in `willPerformHTTPRedirection` — row 6's synthesis).
- **A4** — the two remaining syntheses: SPKI pinning in the trust-evaluation delegate
  (rule 10: real chain+hostname evaluation AND pins, mismatch ⇒ `PinMismatch`, trust
  failure ⇒ `Tls` — mirror the Linux verifier's split exactly) and the redirect hop trace
  via `willPerformHTTPRedirection` (row 7).
- **A5** — priority: map the effect's hint to `task.priority`, assert acceptance only. The
  RFC 9218 wire observation is FLAGGED lore — do **not** conformance-test the wire.
- **A6** — regression guard: run the whole cluster with `usesClassicLoadingMode = false`
  and record divergence (Apple says the default flips; find out now whether we care).
- **C2 addition** — `PermissionDenied` gets its positive control here (step-24 report Q4):
  App-Sandbox/ATS-style denial mapped to the key, watched red first.
- **C3** — the Apple column generated from the capability traits: `PriorityHint` present,
  `Metrics` tier `Phase` (`URLSessionTaskMetrics`), pinned expectations.

## Milestones (one Opus sub-agent each; Fable reviews between)

- **M0 — packaging + the harness bridge.** The FFI crate, the Swift package (walking-
  skeleton adapter: enough to pass exactly one C1 row), the driver returning structured
  results, server control, mise wiring. **Gate:** one row green from `swift test` AND one
  deliberately-broken adapter run showing the same row red with a legible message — the
  bridge must be proven able to fail before anything trusts its greens.
- **M1 — the adapter.** Full `BoltedHttp.swift`: dispatch, completion one-shot, error
  mapping to the C2 keys, deadline synthesis, cancel (rule 9 — `Cancelled`, never a
  URLError leaking as a network key), memory sink, upload progress (rule 11, monotone).
  Target: C1/C2 green except the A2/A4 syntheses. Every row watched red first.
- **M2 — the syntheses.** A2 file sink, A4 pinning + hop trace, rule-4 refusal, rule-5 304,
  `PermissionDenied` control, C3 Apple column. Full suite green.
- **M3 — streaming + sweeps.** A1 probe (+ its verdict paragraph for the freeze), A6
  classic-loading-mode sweep, A5 priority acceptance.
- **M4 — the mutation pass.** Extend `conformance-mutation-table.md` with the Apple
  adapter: mutations across the syntheses (pinning, deadline, trace, cancel, progress),
  two-hypotheses discipline on every survivor. Matrix row statuses updated.
- **M5 — report** (planning session, not a sub-agent): `step-25-report.md` + ROADMAP.

## Kill criteria (real; if hit, stop and report)

1. A C1 rule that cannot pass on URLSession **without contract change** → stop, report the
   rule; the row gets redesigned in the freeze session, not the adapter bent (spike-plan
   kill criterion, verbatim).
2. The bundled packaging story fails for a *second* package (something the packaging report
   didn't cover) → stop; that's a design session, not a workaround.
3. The A1 stream stalls or reorders on the Apple path (the step-02 stall's ghost) → stop
   the A1 cluster, record shape + numbers, continue the rest; the verdict paragraph then
   says so honestly.

## Non-goals (→ elsewhere)

Background transfer family (A7 — D38 deferral stands). iOS device tier (record as
follow-up if the host tier lands; non-gating). The streaming core-seam contract, cookie
capability, `Into<ErrorData>` bridge (→ freeze session). Android (step 26). Anything C#
(parked on the step-23 pin + upstream finding 07).

## Inherited cautions

- Build/test only via `mise run …` tasks; never pipe tier wrappers through `tail`/`head`
  (masks exit codes). `test:apple:ui` needs a GUI session — not part of this step's gates.
- **Verify the CLI before packing**: `cargo install --list` must show `boltffi_cli v0.27.5`
  with **no `?rev=`** — a step-23 git build reports the same version string. If present,
  `mise run setup:boltffi` restores the registry build.
- Swift: no constraint literals (deadlines, limits come from the effect/request — never a
  magic number in the adapter); adapter code maps errors, it never invents keys.
- Rust: edition 2024, clippy `-D warnings`, no `unwrap`/`expect`/`panic!` in library code;
  `bolted-http` stays dependency-free on its default build — the FFI crate is downstream.
- Stale rust-analyzer diagnostics are not the committed state — verify by building.
- If a decision is missing here: smallest reversible choice, record it in the report. If
  structural (traits, invariants, ARCHITECTURE): stop, record, leave for the freeze.

## Exit checklist

- [ ] `mise run check` green (host, Xcode-free) and `mise run test:apple` green (packs +
      both existing packages + the new one).
- [ ] All C1/C2 rows green on `BoltedHttp.swift`, each watched red first; C3 Apple column
      pinned; `PermissionDenied` positive control in.
- [ ] A1 verdict paragraph written (ordered/lossless/complete, numbers, threading notes).
- [ ] A6 sweep recorded (divergence or clean).
- [ ] Mutation table extended; survivors discharged with the two-hypotheses rule.
- [ ] Feature-matrix Apple statuses updated; report + ROADMAP row done.
