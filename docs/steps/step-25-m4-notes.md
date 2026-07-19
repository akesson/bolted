# Step 25 M4 — the mutation pass (notes for M5 / the freeze)

**Milestone:** M4 (the mutation pass over the Apple URLSession adapter). **Branch:**
`step/25-apple-adapter`. Scope: extend `crates/bolted-http/docs/conformance-mutation-table.md` with the
Apple adapter as the third implementor, two-hypotheses discipline on every survivor, and update the
feature-matrix Apple statuses. M5 (report + ROADMAP) is a planning session, not this sub-agent.

## Gate result

- `mise run check` — **green** (host, Xcode-free): workspace clippy `-D warnings` clean, the mock C1/C2/C3
  suites pass (including the two new rows on the correct mock and their red-twins), and the Linux reqwest
  conformance passes the two new `extra_rows`.
- `mise run test:apple:http` — **green** (unmutated tree): 9 XCTest methods, 0 failures. The two new
  blind-spot rows (`C1/row-negotiated-version-observable`, `C1/row-deadline-total-not-per-idle`) are
  GREEN on the real `BoltedHttp` adapter.
- Working tree at the end: the adapter (`BoltedHttp.swift`) and FFI crate (`lib.rs`) have **zero diff**
  (all 20 mutations reverted); only the committed suite-strengthening remains (four files, below).

## Mutations run: 20 · caught: 18 · survived → fixed: 2

The full table (site, expected catcher, observed typed `FailureReason`, result) is in
`conformance-mutation-table.md` under **"The Apple adapter (`BoltedHttp.swift`) — step 25 M4"**. Every
result is from a real `mise run test:apple:http` run — a "caught" means the named row went red with the
recorded reason, never a reasoned prediction.

Coverage across the syntheses the suite claims to pin:

- **Pinning (row 19):** MA1 bypass, MA2 wrong-leaf-SPKI, MA3 `PinMismatch`→`Tls` conflation, MA4 skip
  chain evaluation — all caught (rule-10 / key-pin-mismatch / key-tls).
- **Deadline (rows 4):** MA5 remove the total timer (→ `NoCompletion`); MA6 per-idle substitution
  (**survived** → blind spot, fixed with `/drip`).
- **Cancel (rules 2/9):** MA7 silence (→ `NoCompletion`), MA8 cancel-as-timeout (→ `KeysNotDistinct` /
  `WrongErrorKey`).
- **Redirect (rows 6/7):** MA9 follow the downgrade, MA10 drop a hop, MA11 misreport `final_url` — caught.
- **Progress (row 14):** MA12 non-monotone (`ProgressNotMonotone`), MA13 non-terminal (`ProgressNotTerminal`).
- **Sink (row 15):** MA14 skip atomic rename, MA15 Memory-for-File — caught (`WrongSink`).
- **Error mapping (row 20):** MA16 swap connect↔name-resolution keys — caught (`WrongErrorKey` both ways).
- **content_length / version observables:** MA17 dishonest memory-sink length (caught by rule-07); MA18
  wrong negotiated version (**survived** → blind spot, fixed with a version row).
- **FFI bridge (load-bearing token routing):** MA17 (`to_http_response` content_length) and MA19 (the
  token-parked-sink lookup in `report_progress` → wrong token → no progress routed → `ProgressNotTerminal`).
- **Priority (row 12, A5):** MA20 swap High→low — caught by the A5 acceptance assertion.

## Survivors — the two-hypotheses discharge (both hypothesis 1)

Both survivors were checked against [[a-surviving-mutation-is-two-hypotheses]] *before* being called
blind spots: is the mutant observably different from the correct adapter? For each, **yes**, so
hypothesis 1 (a real behaviour the suite was blind to), and each is fixed with a new row watched red
first — never a test that asserts the mutant's own behaviour.

### MA6 — per-idle `timeoutInterval` instead of the synthesized total deadline

Substituting URLSession's per-idle `timeoutInterval` for the total-deadline `DispatchSource` timer
**passed the entire suite** (all 23 rows green, 0 A6 divergences, in normal time). The only deadline
fixture was `/stall`, which sends one burst (`start`) then holds the socket silent — so a per-idle
timer fires at ~the deadline anyway, indistinguishable there from a total deadline. But the mutant IS
observably different on a **trickling** body (bytes arriving faster than the idle interval keep
resetting a per-idle timer forever; a total deadline must still fire). Hypothesis 1.

**Fix (committed):** a `/drip?count=N&interval_ms=M` server endpoint (one byte every `M` ms) + a new row
`C1/row-deadline-total-not-per-idle` (drip 40×50 ms, 300 ms deadline, requires `Timeout` in budget).
Watched red: mock red-twin `arm_deadline=false` (`deadline_total_red_under_per_idle`), and — re-applying
MA6 on the real adapter — the drip row went red `ExpectedErrorGotSuccess{Timeout,200}` **while every
`/stall`-based row stayed green** (the exact blind-spot signature). Correct mock, reqwest, and the real
Apple adapter all pass it green.

### MA18 — wrong negotiated version (`Http2` for the HTTP/1.1 server)

Reporting the wrong `HttpResponse::version()` **passed the entire suite**: a grep confirmed **no**
C1/C2/C3 row referenced `version()` — the exact shape of step 24's redirect-trace blind spot, for the
version field. The mutant reports `Http2` where every correct implementor reports `Http1_1` against the
same 1.1 server — observably different. Hypothesis 1.

**Fix (committed):** row `C1/row-negotiated-version-observable` (drive `/ok`, assert `version() == Http1_1`)
+ `FailureReason::WrongHttpVersion { got, expected }` + mock knob `honest_version`. Watched red: mock
red-twin `negotiated_version_red_when_wrong`, and — re-applying MA18 on the real adapter — the row went
red `WrongHttpVersion{got:Http2, expected:Http1_1}`. Correct mock, reqwest, and Apple all pass green.

## The committed suite strengthening (found by mutation)

Four files (the adapter + FFI crate are NOT touched — mutations reverted):

- `crates/bolted-http/src/conformance/c1.rs` — two new `extra_rows`
  (`row-negotiated-version-observable`, `row-deadline-total-not-per-idle`) + two red-twin tests
  (`negotiated_version_red_when_wrong`, `deadline_total_red_under_per_idle`); `SINK_ROWS` grows 2 → 4.
- `crates/bolted-http/src/conformance/mod.rs` — `FailureReason::WrongHttpVersion { got, expected }`
  (+ `HttpVersion` import).
- `crates/bolted-http/src/conformance/netmock.rs` — `MockBehavior::honest_version` knob (gates the
  reported version).
- `crates/bolted-http/src/conformance/server.rs` — the `/drip?count&interval_ms` endpoint.

Both rows run against all three implementors (mock, reqwest, Apple) and are green on each. No new
`content_length` blind spot exists: rule-07 already reads it (MA17 caught); the File-sink length is
correctly `None` (unobservable — body on disk), so no row asserts it.

## Friction log (freeze-agenda input)

- **F-M4-1 — `/stall` cannot pin total-vs-per-idle; the suite's rule-3 claim was partly hollow until M4.**
  The A3 hazard the M1/M2 notes call load-bearing ("URLSession's per-idle `timeoutIntervalForRequest`
  must not mask the total deadline") was **not** actually pinned by any row before this pass — a per-idle
  substitution passed the whole suite. The `/drip` row closes it for every implementor. Freeze note: the
  deadline conformance rule should explicitly require a *trickling*-body fixture, not only a
  stalled-from-the-start one, on any future surface.
- **F-M4-2 — the negotiated-version observable had no positive control on any implementor.** Row 11 is
  CORE ("every surface always reports it") yet no C1/C2/C3 row read `version()` — an adapter could report
  any protocol and pass. Now pinned to the server's actual `HTTP/1.1`. If a future adapter negotiates h2
  against the (1.1-only) test server the row would need a per-fixture expected version; today all four
  surfaces speak 1.1 to the hand-rolled server, so a literal `Http1_1` is correct.
- **F-M4-3 — URLSession's `didSendBodyData` is sparse for a small POST; the adapter's terminal top-up is
  load-bearing for rule-11.** The first attempt at a non-monotone mutation (invert the reported bytes)
  was **flaky** — the adapter's own terminal top-up (which fires when the OS-fed progress stops short)
  masked it in one test method and not another. The committed mutations were redesigned to be
  deterministic (drive the drop / the short value *through* the top-up), so each reds rule-11 in every
  run. Worth noting the adapter genuinely relies on the terminal top-up to reach the body length —
  meaning "OS-fed monotone progress" is really "OS-fed progress + an adapter-synthesised terminal
  sample". The suite pins the composite, which is the honest contract (rule 11 is monotone-per-attempt +
  terminal consistency, not wire-truth).
- **F-M4-4 — NoCompletion mutations leak the `/stall` handler for its bounded hold, slowing the suite.**
  Removing the deadline (MA5) or silencing cancel (MA7) makes stall/cancel rows return `NoCompletion` at
  budget while the server-side `/stall` thread holds ~30 s; across the A6 double-sweep this stacks. These
  runs were driven under a pseudo-tty and read from the A6-sweep row failures (same `mise` task, observed
  live). Not a contract issue — a property of the test server's bounded hold; recorded so M5/CI sizing
  accounts for it. (Also observed: intermittent slow `.invalid` DNS on the host inflates some runs — a
  host resolver quirk, not the adapter.)

## M5 hand-off

- Report `step-25-report.md` + ROADMAP row. The mutation table + feature-matrix Apple statuses are done.
- Freeze-agenda inputs from this pass: F-M4-1 (deadline fixtures must trickle) and F-M4-3 (upload
  progress is a composite of OS-fed + adapter-synthesised terminal sample) are the two that touch the
  contract's conformance-rule wording; F-M4-2 is closed by the new row.
