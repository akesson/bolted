# Step 24 — bolted-http I: the harness, the streaming verdict, the reference adapter

**Phase 4 — Harness (the bolted-http sequence begins). Status: ready.**

The v1.13 go decision (2026-07-19) schedules the bolted-http implementation along
`crates/bolted-http/docs/spike-plan.md`. This step is the plan's first working-session shape:
**S-CONF + S-FFI + S-LX** — after it, the conformance harness exists, one real adapter passes
it, and the two verdicts gating the contract freeze (FFI streaming mechanism, row 16; SPKI
pinning on Linux, row 19) are in. Steps 25/26 (Apple, Android adapters) build on this; the
contract-freeze design session runs after this step's verdicts.

**Read before anything**: `crates/bolted-http/docs/spike-plan.md` (§0–§2, §6 are this step's
spec), `feature-matrix.md` (§4 the classification, §5 the per-row evidence, **§7 the eleven
fixed rules — the suite's backbone**), `architecture.md` (the three-layer shape and the §2
contract sketch). Where they conflict, the matrix wins.

## Decisions already taken (do not re-litigate; the matrix records them)

- Priority hint is **CAP** (Henrik, 2026-07-19 — row 12).
- **Conditional `Send` bounds from the first trait written** (Henrik, 2026-07-19): the
  contract traits must compile with and without `Send` requirements (target-conditional —
  hard-require on native, relaxed where wasm would need it). A later web adapter must never
  be locked out at the type level.
- **`FileRef` lives in `bolted-http`** (design session, 2026-07-19): newtype over a path,
  opaque-ready.
- Web is **out** of the platform set; no web code, no wasm target in this step.
- **boltffi stays at the registry 0.27.5 pins.** The step-23 git pin is killed/parked — this
  step must not touch it. S-FFI (§2 below) runs at 0.27.5, which both step-02 probes already
  proved is where the stream machinery converges.

## Scope

Three clusters; S-CONF is the deliverable that outlives the spike, S-FFI and S-LX produce the
two verdicts. The mock-first ordering is load-bearing (the one-implementor lesson): **the
suite must fail correctly before it can pass correctly** — build it against a mock adapter,
then the reqwest reference adapter, then mutate both.

### S-CONF — the contract types + the conformance harness (host, Rust)

1. **Contract types in `bolted-http`** (the stub crate exists; keep `#![forbid(unsafe_code)]`
   and the sans-io promise — the lib target gains **no** tokio/reqwest/TLS deps):
   `HttpRequest` (method, URL, headers, body `Bytes | File(FileRef)`, **one total deadline**),
   `HttpResponse` (status, headers, body sink outcome, final URL, hop trace, negotiated
   version as an observable), `HttpError` (typed keys + params — Bolted's error rule, never
   strings), the `Http` capability trait (effect out, completion as typed input), the optional
   capability traits per matrix §4 (Metrics tiered; PriorityHint; FineTimeouts is
   composition-root config, not a trait), and the reserved-header compile-time guard (rule 6's
   core half). Follow the matrix rows, not the older architecture-§2 sketch, where they
   differ.
2. **The conformance suite as a feature-quarantined module** of `bolted-http` (the
   `wasm-budget`-behind-a-`budget`-feature precedent): a `conformance` feature gates the
   suite + the test server; the default feature set stays dependency-clean. The suite is
   generic over "an adapter under test".
3. **The local test server** the harness owns (spike-plan §0): echo, delay, stall-mid-body,
   redirect chains incl. https→http, 304/ETag, gzip/brotli, 401, TLS-failure endpoints,
   pin-mismatch cert (self-signed cert generation is in scope; crate choice is the
   implementer's, smallest reversible, recorded).
4. **C1**: the eleven §7 rules as parameterized rows. **C2**: the error-taxonomy matrix —
   every typed error key reachable via the server, **a positive control per key** (a needle
   that can never match is green forever). **C3**: the divergence matrix **generated from the
   capability types** — the harness emits, per adapter, the capabilities-present/absent table;
   hand-written prose matrices are the prior-art failure mode.
5. **The mock adapter** in the conformance module: passes every row by construction, and is
   the vehicle for watching every row fail correctly first.

### S-FFI — the streaming mechanism verdict (host, boltffi 0.27.5)

The one probe gating a contract row (matrix §5.11 / row 16). Re-run the step-02 stream shapes
**inside an http round-trip** at 0.27.5, in the existing non-workspace spike crate family
(`crates/spike-http-ffi` has the packaging spike's http round-trip;
`crates/spike-profile-ffi-stall-probe` has the stream-shape machinery — read both crates'
docs before writing code):

- **F1**: 100-chunk response body via `ffi_stream` push, live consumer (the shape that
  stalled at 15/100 on 0.27.3). Measure completeness + latency.
- **F2**: the same body via callback-trait push (the machinery measured at ~8 ns).
- **F3**: the same via wake-and-read batch pull (`snapshot()` getter).
- Output: a decision artifact (numbers, not vibes) — which mechanism carries response
  streaming across the FFI, or the fallback.

**The fallback is legal, not a kill**: if all push shapes still stall in the http context,
row 16 drops to `Memory | File` sinks; record it (it parks SSE with WebSocket, §5.11).

### S-LX — the reference adapter (Linux/host, reqwest)

1. **`bolted-http-linux`** (new workspace crate): the reqwest adapter behind the `Http`
   capability trait, config per the matrix's CFG rows (no ambient cookies/cache; retry off —
   connection-level recovery only). It passes the full suite. This is the reference adapter
   the suite is debugged against, *after* the mock.
2. **L2 — the pinning verdict (row 19)**: `tls_backend_preconfigured` + a custom rustls
   verifier carrying the contract's SPKI pin data; pin-mismatch ⇒ the typed pinning error
   (rule 10). If the rustls verifier API cannot express it cleanly: row 19 demotes to CAP
   with Linux absent — **report, don't hack**; that is a verdict, not a failure.
3. **L3**: retry-off proven by rule 8's positive control. **L4**: proxy env-vars-only
   behavior recorded into the C3 divergence output, not worked around.

### The mutation pass (both implementors)

The suite bites or it doesn't: mutate the mock and the reqwest adapter (wrong error key,
dropped header, silent retry, non-monotone progress, pin bypass …) and watch the suite go
red each time. A surviving mutation is two hypotheses — rule out "the mutant was identical"
before "the suite is blind". Record the mutation table in the report.

## Milestones

- **M0 (S-CONF 1/2)**: contract types + `FileRef` + reserved-header guard + capability
  traits (conditional `Send`); mock adapter; suite skeleton failing correctly. Commit.
- **M1 (S-CONF 2/2)**: test server + C1 rows + C2 taxonomy w/ positive controls + C3
  generated divergence matrix; mock green; every row watched red first. Commit.
- **M2 (S-FFI — independent of M0/M1, may run in parallel)**: F1/F2/F3 + the decision
  artifact. Commit.
- **M3 (S-LX)**: `bolted-http-linux` green on the suite; L2/L3/L4 verdicts. Commit.
- **M4**: the mutation pass on both implementors. Commit.
- **M5**: report (`step-24-report.md`) + ROADMAP + matrix rows 16/19 updated with the
  verdicts (planning session writes this).

## Kill criteria (real; if hit, stop and report)

1. **A C1 rule cannot be expressed without breaking the sans-io boundary or a frozen
   invariant** (ARCHITECTURE §1–§7) → stop; the *row* gets redesigned in a design session,
   not the crate.
2. **The contract types force a change in `bolted-core`** or any ARCHITECTURE §9 question
   would need resolving ad hoc → stop, record, design session.
3. S-FFI all-stall and L2-infeasible are **verdicts with prescribed fallbacks** (above), not
   kills — record and continue.

## Non-goals (→ elsewhere)

- Apple/Android/Windows adapters (steps 25/26; S-WIN rides the parked C# pin).
- BackgroundTransfer, cookies, WebSocket (§9-open / parked — nothing here may foreclose
  them; the effect-family seam stays open in the types).
- The FFI glue / facet-binding generation for bolted-http (after the contract freeze).
- The contract *freeze* itself (design session after this step's verdicts).
- Touching the boltffi pins in any direction.

## Inherited cautions

- The suite must fail correctly before it passes (watch every row red; positive control per
  error key). A drift/conformance check nobody watched red proves nothing.
- Don't pipe a tier wrapper through `tail` when its exit code is load-bearing.
- Build/test only via `mise run check` / `mise run test` (wire new crates into the existing
  verbs; a new mise task is fine if `check` covers it).
- Rust: edition 2024; clippy `-D warnings`; no `unwrap`/`expect`/`panic!` in library code
  (test/dev code in the conformance module follows the workspace's existing test idiom).
- No constraint literals in anything adapter-facing; deadlines/limits come from the contract.
- Commit per milestone; never `git -C`.

## Exit checklist

- [ ] `bolted-http` lib target still dependency-clean (sans-io); conformance behind the
      feature; conditional `Send` bounds in place and compile-checked both ways.
- [ ] C1 eleven rows + C2 with positive controls + C3 generated; mock and reqwest adapters
      both green; every row watched red first.
- [ ] The S-FFI decision artifact exists with measurements; row 16's verdict recorded.
- [ ] L2 pinning verdict recorded (works ⇒ row 19 stands; infeasible ⇒ demote documented).
- [ ] Mutation table in the report; no surviving mutation left unexplained.
- [ ] `step-24-report.md` + ROADMAP row updated; feature-matrix rows 16/19 carry the
      verdicts; ARCHITECTURE untouched (the freeze is a later design session).
