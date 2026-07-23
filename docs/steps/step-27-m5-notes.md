# Step 27 M5 — the mutation pass: evidence

**Branch:** `step-27/m5` (off main at `8b24f16`, the merged M0–M4 head). One deliberate,
semantics-breaking defect at a time into **shipped** code (never tests); the relevant suite is run;
the red test recorded; the mutant **reverted** before the next. Final tree contains only the notes
file and one test-side addition (host tests in `bolted-http-ffi`) — every shipped-code mutant is
gone (`git diff 8b24f16` = the FFI test module + this file).

## How verdicts were gathered

- Fast inner loop per mutant: targeted `cargo test -p <crate> [--features …]` — the identical test
  binary the gate builds.
- **Recorded** verdict: the full **`mise run test`** (`cargo test --workspace`). Feature unification
  in the workspace pulls `bolted-http`'s `conformance` feature on (via `bolted_http_ffi`'s
  non-dev dependency `bolted-http = { features = ["conformance"] }`), so `mise run test` **does**
  run the conformance rows, the `stream`/`signal`/`redirect` unit tests, the `compile_fail`
  doctests, the Linux reqwest tier, and the new `bolted_http_ffi` host tests — a single superset
  watcher. FFI-internal verdicts (4b/4c/6c) were read from `cargo test -p bolted_http_ffi` (same
  binary `mise run test` runs).
- Final clean-tree gate: `mise run check` (exit 0) **and** `mise run test` (exit 0), both green with
  the added tests in place.

## Mutation table

| # | Site (file:line) | Edit (before → after) | Verdict | Watcher(s) — verbatim test + failure |
|---|---|---|---|---|
| 1 | `bolted-http/src/stream.rs:150` (ring bound) | `ring.len() >= RING_CAPACITY` → `>` | **caught** | `stream::tests::ring_overflow_is_a_typed_failure` — `left: Ok(())` / `right: Err(StreamOverflow { capacity: 256, seq: 256 })` (the 257th chunk wrongly accepted) |
| 2a | `stream.rs:146` (seq check — accept a gap) | `chunk.seq != next_seq` → `chunk.seq < next_seq` | **caught** | `stream::tests::first_chunk_must_be_seq_zero` + `stream::tests::out_of_order_seq_is_rejected` FAILED (gap/first≠0 accepted); `repeated_seq…` correctly stayed green |
| 2b | `stream.rs:146` (seq check — accept a repeat) | `chunk.seq != next_seq` → `chunk.seq > next_seq` | **caught** | `stream::tests::repeated_seq_is_rejected` FAILED (repeat accepted); `out_of_order…` correctly stayed green |
| 3 | `stream.rs:202` (completeness gate) | `total == ingested_bytes` → `>=` | **caught** (2 watchers) | `stream::tests::completeness_gate_rejects_a_wrong_total` — `left: Ok(5)` / `right: Err(Transport)`; **and** `conformance::stream::tests::row_12_red_on_truncation` FAILED (truncation now passes the gate) |
| 4a | `stream.rs:199` (terminal-once, type-level) | `finish(self, …)` → `finish(&self, …)` | **caught** | Both `compile_fail` doctests `stream::TerminalIsExactlyOnceByConstruction` (lines 221, 231) FAILED — they now **compile** (second `finish` / chunk-after-`finish` no longer a use-after-move), which a `compile_fail` doctest reports as a failure |
| 4b | `bolted-http-ffi/src/lib.rs:640` (`finish_body` remove-then-consume) | body → no-op (`let _ = (token, end);`) | **survivor → killed by added test** | `bolted_http_ffi` tests::`finish_body_removes_and_consumes_the_parked_sink` FAILED (terminal never fired / `live_streams` stayed 1) |
| 4c | `bolted-http-ffi/src/lib.rs:621` (`deliver_chunk` close-on-error) | on `Some(Err)`: remove+`finish(Failed)` → do nothing | **survivor → killed by added test** | `bolted_http_ffi` tests::`deliver_chunk_closes_the_stream_on_a_typed_failure` FAILED (stream left parked, no `Failed` terminal) |
| 5a | `bolted-http/src/redirect.rs:59` (`enforce_count` boundary) | `hop_count > max` → `>=` (off-by-one, fires one early) | **caught** | `redirect::tests::within_the_ceiling_is_ok` + `redirect::tests::enforce_count_matches_enforce` — `left: Err(TooManyRedirects{limit})` / `right: Ok(())` (exactly-at-ceiling wrongly rejected) |
| 5b | `bolted-http-linux/src/lib.rs:350` (adapter ceiling enforcement) | `redirect_ceiling.enforce(&hops)?` → no-op | **caught** | `c2_every_reachable_key_produced` (Linux tier) — `Fail(WrongErrorKey { expected: TooManyRedirects, got: Timeout })` (unbounded `/redirect-loop` runs to the 5 s deadline) |
| 6a | `bolted-http/src/signal.rs:82` (signal wiring — `pause()`) | `emit(Pause)` → `emit(Resume)` | **caught** (4 watchers) | `signal::tests::all_three_uses_reach_the_observer_in_order` — `left: [Resume, Resume, Cancel]` / `right: [Pause, Resume, Cancel]`; **and** `conformance::stream::tests::back_pressure_is_load_bearing_no_overflow_under_slow_consumer` (`left: 256` / `right: 384` — ring overflowed, StreamOverflow) + `correct_stream_mock_passes_both_rows` + `row_13_red_on_missing_terminal` |
| 6b | `bolted-http-linux/src/lib.rs:541` (Linux observer — `Pause` arm) | `Pause => paused.store(true)` → `Pause => {}` | **SURVIVOR** — see analysis below | — (`mise run test` stayed green, exit 0) |
| 6c | `bolted-http-ffi/src/lib.rs:416` (`NativeFlowObserver` — `Cancel`) | `Cancel => FfiFlowSignal::Cancel` → `Cancel => return` (drop) | **survivor → killed by added test** | `bolted_http_ffi` tests::`flow_signals_are_forwarded_to_the_native_task` FAILED (Cancel not forwarded to the native task) |

`seq` note (2a/2b): the gap and the repeat are guarded by a **single** `!=` comparison, not two
separate checks, so both are expressed as two mutations of that one comparison; each isolates one
half (the other twin correctly stays green, confirming the mutants are disjoint).

## The FFI-bridge survivors (4b, 4c, 6c) — host watchers added

`bolted-http-ffi` shipped with **zero** host tests (`cargo test -p bolted_http_ffi` → "running 0
tests" on the clean tree at `8b24f16`); the streaming-seam bridge logic — `finish_body`'s
remove-then-consume, `deliver_chunk`'s close-on-error, `NativeFlowObserver`'s signal forwarding —
was watched **only** by the Apple/Android platform tiers. So a defect there survives the entire host
suite (baseline `mise run test` was green with 0 FFI tests → any FFI-internal mutant survives it).

**Two-hypotheses discipline.** (b) semantically-identical was ruled out first: each mutant flips an
externally observable bridge output — `live_streams()`, whether the parked sink's terminal fires,
and whether the native task receives `signal(token, Cancel)`. (a) suite-blind is then the confirmed
cause (no host test exercises the bridge). Per the step doc's "if the fix is small and test-side
only, add the missing host test", I added a `#[cfg(test)] mod tests` to `bolted-http-ffi` (a fake
`HttpAdapter` that echoes the streaming token and records forwarded signals + a recording
`ChunkSink`). Each mutant was **watched red** against that test, then **green after revert**:

- 4b → `finish_body_removes_and_consumes_the_parked_sink` (asserts terminal fired **and**
  `live_streams()==0`).
- 4c → `deliver_chunk_closes_the_stream_on_a_typed_failure` (a sink that raises a typed failure on a
  chunk ⇒ bridge returns `false`, closes with `Failed`, `live_streams()==0`).
- 6c → `flow_signals_are_forwarded_to_the_native_task` (Pause/Resume/**Cancel** all reach the fake
  adapter's `signal`).

These are test-side-only additions; no shipped semantics were weakened to make them pass. The
platform tiers (Apple rows 12/13/14, Android rows 12/13/14) remain the **secondary** watchers for
the same bridge logic through a real native adapter.

## Survivor 6b — Linux observer ignores `Pause` (recorded finding)

**Mutant:** `LinuxFlowObserver::on_signal`'s `Pause` arm (`bolted-http-linux/src/lib.rs:541`) from
`self.paused.store(true, …)` to a no-op. **`mise run test` stayed green (exit 0)** — the Linux
streaming rows (`streaming_rows_pass_against_reqwest_adapter`, rows 12/13) all passed with the
adapter *ignoring* back-pressure.

**Two hypotheses:**

- **(b) semantically identical / unreachable — REFUTED.** A temporary probe
  (`eprintln!` in the `Pause` arm before `store(true)`) run against
  `cargo test -p bolted-http-linux streaming -- --nocapture` printed the probe **twice**: the driver
  *does* emit `Pause` to the Linux observer during the run (the ring reaches the ¾ high-water mark
  `HIGH = 192` at least twice against reqwest's delivery). The `Pause` arm is genuinely reached and
  the mutant genuinely changes runtime state (the `paused` atomic, hence whether `stream_perform`'s
  read-pacing loop parks). Not a no-op mutation.
- **(a) the suite is blind — CONFIRMED.** Despite `Pause` being delivered and ignored, the ring
  never reached `RING_CAPACITY = 256`, so no `StreamOverflow` fired and the body completed
  intact — the row passes **whether or not** Linux honours `Pause`. Reason: against a localhost
  reqwest transport, the harness's 1 ms-tick slow consumer drains fast enough relative to reqwest's
  network-paced delivery that the ring never overflows in the un-paced window between `HIGH` and
  `RING_CAPACITY`. The Linux streaming row asserts completeness, not that pausing was *load-bearing*.

**Resolution — recorded division, no Linux killer test added.** This is **not** the "no host-side
watcher exists at all" case: the contract-level property *an adapter that ignores `Pause` under a
slow consumer overflows the ring* **is** watched on the host — by
`conformance::stream::tests::row_12_red_on_ignored_back_pressure_is_overflow` (the `StreamMock`
`IgnorePause` twin drives `ROW_CHUNKS > RING_CAPACITY` discrete chunks through a real
`StreamingHttp` adapter and earns `StreamOverflow`). What is unwatched is the *Linux adapter's own*
`Pause`-honouring, because reqwest's coalesced, network-paced delivery means no host row forces its
ring to overflow. A Linux-specific killer test would need to make reqwest's chunk granularity and
the consumer's drain rate race deterministically toward overflow — inherently timing-dependent and
flaky. A flaky back-pressure assertion is worse than the recorded division (project memory: *"A
forbidding test can forbid nothing"* — a test that only sometimes forbids passes vacuously). M2's
notes already recorded this exact division ("reqwest coalesces … Linux's pushed-pause path is wired
but not stress-tested … Recorded division, not a gap"); M5 confirms it is a genuine **suite-blind
spot on the Linux impl**, mitigated by the mock's positive control for the same property. Left as a
recorded finding for planning; no shipped code touched.

## Friction log

- **`mise run test` runs conformance via feature unification.** Contrary to the `mise run check`
  comment ("`--workspace` does not enable [conformance]"), `cargo test --workspace` *does* build
  `bolted-http` with `conformance` on, because `bolted_http_ffi` depends on it as a **non-dev**
  dependency and the workspace unifies features. Empirically the conformance `stream` rows, the
  Linux tier, and the `compile_fail` doctests all appear in the `mise run test` log — so it was a
  sufficient superset watcher for every mutant except the FFI-internal ones (read from
  `cargo test -p bolted_http_ffi`, the same binary).
- **rustfmt owns the added tests.** The new `#[cfg(test)] mod tests` failed `cargo fmt --all
  --check` on first `mise run check`; `cargo fmt -p bolted_http_ffi` fixed it (no semantic change).
  Clippy `-D warnings` on `--all-targets` is clean.
- **Mutant 5b costs ~5 s.** Deleting the Linux ceiling makes `/redirect-loop` run until the 5 s
  request deadline (then `Timeout`); the buffered `perform` `select!` bounds it, so no hang — the
  workspace run just takes a few seconds longer while that mutant is applied (reverted after).

## Open questions (for planning)

1. **Linux back-pressure is a suite-blind spot (survivor 6b).** The Linux adapter's `Pause`-honouring
   read-pacing is not load-bearing in any host row (the mock's `IgnorePause` control watches the
   *property*, not the Linux impl). Options: (a) accept the mock as the property watcher and leave
   Linux's pause path proven only by the platform-analog device tiers / manual inspection; (b) a
   dedicated slow-consumer Linux row engineered for deterministic overflow-without-pause (risk:
   flaky). Recommend (a) unless the streaming RFC re-evaluation (streaming-seam §7) reopens the seam.
   Not resolved ad hoc here — it touches test-harness design, left for a planning session.

No other outstanding questions. No ARCHITECTURE §1–§7 / §9 surfaces were touched.
