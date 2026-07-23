# Step 27 — report: bolted-http IV, the ruled contract implemented

**Status: done — no kill criteria hit; every exit-checklist item met; the mutation pass found
one genuine suite-blind spot (recorded) and one watcher gap (fixed test-side).**
Fifth step under the Fable-orchestrates model; six milestones (M0–M5), each one Opus sub-agent
run with independent verification between (gates re-run on the branch, diffs read in full,
mutants spot-checked by re-applying them — never trusting the agent report or the IDE
diagnostics alone; the stale-diagnostics-vs-green-gates pattern recurred on four of six
milestones, always resolved in the gates' favour). PR-per-milestone, rebase-merged:
M0 #19 → M1 #20 → M2 #21 → M3 #23 → M4 #24 → M5+report on `step-27/m5`.
Gating tiers: host (`mise run check` / `mise run test`), macOS XCTest **by count**,
instrumented ART **by JUnit XML**.

## What was built

- **M0 — the probe, then the merge.** The note-08 runtime probe confirmed BoltFFI's bindgen
  union claim (a `#[cfg(target_os = "ios")]`-gated `#[data]` item lands in the Kotlin
  bindings), so the single-crate merge was safe: the two mirror bridge crates became one
  multi-target `bolted-http-ffi`. `PriorityHint` went uniform (Q10 — a plain advisory request
  field on every adapter, legally a no-op where the engine can't honour it; its C3 column and
  marker trait deleted).
- **M1 — the core seam, host-side.** The chunk-input family: `BodyChunk` with `seq` verified
  ascending/gapless, the bounded ring with **core-owned capacity** (a constraint value, never
  a shell literal), `StreamOverflow` as a typed `HttpError` variant, the `BodyEnd` terminal
  with the completeness gate (`total == ingested`, else typed failure), and
  terminal-exactly-once **by construction** (`finish(self)` consumes; two `compile_fail`
  doctests are the watchers — M5 proved they really fail when the signature is weakened).
  Plus the small rulings: `RedirectCeiling` CFG with core-counted `TooManyRedirects`
  (at-ceiling is OK, strictly-beyond fires), `Into<ErrorData>` for `HttpError` (Q6),
  `content_length` rustdoc wording, file-sink verified-total.
- **M2 — the one mid-flight signal + Linux + the host rows.** `FlowSignal { Pause, Resume,
  Cancel }` / `FlowObserver` / `FlowSignals` (Q4, streaming-seam §3b option C): one shape,
  two uses — back-pressure and pushed cancel — sans-io and lock-free in the contract type
  (the only machinery lives adapter-side, behind the observer; kill criterion 2 never
  engaged). `ChunkSink` (repeatable `deliver_chunk` + consuming `finish`) and `StreamingHttp`
  (opt-in, never widens `Http`). `bolted-http-linux` onto the full seam: `bytes_stream`
  chunked delivery, read-pacing on Pause (tokio `Notify`, no-lost-wakeup pattern),
  `wait_cancelled` poll-watcher deleted, `redirect_ceiling` as sole redirect authority
  (reqwest native follow off). New rows 12 (slow-consumer completeness) and 13
  (terminal-exactly-once) on mock + Linux; rule 11 gained the final-`total` assertion (Q8).
  **No truncation key minted** — row 12's red is key-agnostic and `Transport` sufficed; the
  revisit trigger (a consumer that must *branch* on truncation) is recorded in the code.
- **M3 — Apple graduates.** The step-25 A1 probe machinery became shipped contract-path code:
  `FfiBodyChunk`/`FfiBodyEnd`/`FfiFlowSignal` mirrors, `HttpAdapter::cancel` →
  `signal(token, flow)` plus `execute_streaming`, a token-keyed **parked-`ChunkSink`
  registry** whose terminal removes-and-consumes (exactly-once across the FFI too), and
  `live_streams()` as the hygiene observable. `BoltedHttp.swift`: `didReceive data` →
  `deliverChunk` per transport read (outside the lock), `didCompleteWithError` → `finishBody`,
  cancel/pause/resume → `task.cancel/suspend/resume`; the 10 ms poll-watcher thread deleted
  on both sides of the boundary. Rows 12/13 on the macOS tier + **row 14 (subscription
  hygiene)** with the F-M3-1 leak as its red case.
- **M4 — Android graduates.** `BoltedHttp.kt` onto the same shape: the OkHttp body **source**
  read in the adapter's own loop, one JNI push per read; pause/resume pace the loop with a
  lost-wake-safe guarded monitor wait (OkHttp has no task-suspend — the Linux mechanics,
  mirrored); cancel pushed by cause. The N2 probe (`StreamProbe.kt`, including its stale
  pre-0.28.0 `trySend` commentary) deleted, replaced by real rows 12/13/14. The
  `TOO_MANY_REDIRECTS_PREFIX` exception-text match deleted — redirect exhaustion is now
  classified **structurally** (a per-hop interceptor records whether the last hop was a 3xx).
- **M5 — the mutation pass.** Twelve mutants across the six mandated enforcement points
  (ring bound, seq gap + repeat, completeness gate, terminal-once type-level + two
  bridge-side, ceiling boundary + adapter enforcement, three signal-wiring sites). Eight
  caught outright by existing watchers; three FFI-bridge survivors killed by a new host test
  module; one genuine survivor recorded (below). Full table with verbatim watchers in
  `step-27-m5-notes.md`.

## Results

- **The watched-red matrix is complete per implementor.** Row 12: mock `Truncate` →
  `StreamFailed(Transport)`, mock `IgnorePause` → `StreamFailed(StreamOverflow)`, Linux
  `DropChunk`, Apple `.dropChunk`, Android `DROP_CHUNK`. Row 13: mock/Linux/Apple/Android
  `SkipTerminal` → `NoTerminal`. Row 14: Apple `.skipTerminal` and Android `SKIP_TERMINAL`
  each leave `liveStreams() == 2 > 0`, with the zero baseline asserted first as the positive
  control. Rule 11's total: socket-mock `honest_progress_total = false` →
  `ProgressWrongTotal`. Double-terminal remains compile-impossible on every leg.
- **All poll-watchers are gone.** Linux `wait_cancelled`, the Apple bridge's 10 ms thread,
  and the Kotlin side all replaced by the pushed `Cancel`; grep-verified per milestone.
- **The mutation pass earned two findings** (both are the point of running it):
  1. **`bolted-http-ffi` had zero host tests** — `finish_body`'s remove-then-consume,
     `deliver_chunk`'s close-on-error, and the observer's signal forwarding were watched only
     by the device tiers. Fixed test-side: a host `#[cfg(test)]` module (fake `HttpAdapter` +
     recording `ChunkSink`) now kills all three mutants, each watched red then green.
  2. **Survivor 6b: the Linux adapter's `Pause`-honouring is suite-blind.** Two-hypotheses
     discipline applied: a probe proved Pause *is* delivered and the mutant genuinely changes
     state (hypothesis b refuted); the row still passes because reqwest's network-paced
     delivery never overflows the ring against the 1 ms-tick consumer (hypothesis a
     confirmed). The contract *property* stays watched by the mock's `IgnorePause` row; the
     Linux-specific gap is recorded, not papered over with a timing-flaky test.
- **Back-pressure stress lives on the mock, deliberately.** All three adapters wire
  pause/resume end-to-end, but URLSession/OkHttp/reqwest all coalesce reads on the small
  `/chunked` body, so the ring never fills against real transports. The mock's
  `ROW_CHUNKS > RING_CAPACITY` discrete chunks make the signal load-bearing there. Recorded
  division on all three legs, confirmed (not just assumed) by M5.

## Deviations and judgment calls (all recorded in the milestone notes; none structural)

- **Chunks re-enter synchronously via `ChunkSink`, not `ffi_stream`** (M3, the load-bearing
  call of the step). The deterministic-close *discipline* graduated while transport uses the
  M2 reference-driver shape — so no live native consumer exists in the shipped path, and the
  F-M3-1/F-M0-5 leak (an unfixed `ffi_stream` runtime defect at boltffi 0.28.0) **cannot
  occur in the contract path at all**; it reduces to a parked registry entry, which row 14
  counts deterministically (no ARC/GC probe — the ART-GC-control lesson honoured by
  sidestepping GC entirely). The upstream defect still exists and still deserves the
  filing (Henrik's action); the shipped path just no longer depends on that machinery.
- **"Core-counted" redirects literally hold on Linux only** (M4). No redirect ceiling crosses
  the FFI surface (the `0` sentinel was the ruled M1 shape); Apple rides URLSession's native
  cap, and Android now rides OkHttp's with structural classification instead of the deleted
  text match. Literal core-counting on the FFI legs would have required new bridge surface
  plus a manual-follow rewrite on both platforms — flagged at M4 review as a planning
  decision if ever wanted; the fragile part (text matching) is gone either way.
- `deliver_chunk` returns `bool` across the FFI (harness owns close-on-error) — a recorded
  divergence from the Rust `ChunkSink` signature that avoids an error round-trip.
- `DropChunk` red twins drop the *first* read on Apple/Android (a delegate can't know the
  last mid-stream); identical gate failure to Linux's last-chunk `Truncate`.
- `StreamOverflow`'s C2 classification stays `ContractGap` even though the `IgnorePause`
  control now exists — no *conformant* adapter produces the key, and a `Reachable` row would
  assert a lie.

## Friction log (cross-milestone)

- **Stale IDE diagnostics contradicted green gates on four milestones** (M0, M1, M2, M3 —
  M3's eleven SourceKit errors were indexed against old gitignored bindings and disproven by
  the tier compiling). The discipline that held: never trust the diagnostics *or* the agent
  report; grep + re-run the gates on the branch.
- `mise run test` watches conformance via workspace feature unification (the `mise.toml`
  comment claiming otherwise is stale — M5 notes have the detail); worth a comment fix in a
  housekeeping pass.
- GMD JUnit XML carries no `<system-out>` (F-M1-6 continued), so Android watched-red reasons
  are read from per-test logcat.
- Rebase merges rewrite hashes — every milestone re-synced main before branching; no
  incident, but the workflow depends on it.

## Open questions (for planning / the freeze agenda)

1. **Linux back-pressure blind spot (M5 survivor 6b).** Accept the mock as the property
   watcher (recommended — a deterministic Linux overflow row would be timing-flaky), or
   engineer a dedicated slow-consumer Linux row. Revisit only if the streaming RFC
   re-evaluation (streaming-seam §7) reopens the seam.
2. **The synchronous-`ChunkSink` re-entry choice** (M3 call 1) deserves a planning glance:
   "subscription lifecycle" in the shipped path now means a registry entry, not a
   boltffi-runtime subscription. It stays within the adopted seam shape; streaming-seam §7's
   upstream-RFC trigger is unaffected.
3. **Upstream filing** (Henrik's alone): the `ffi_stream` overflow-drop (F-M0-4) and
   abandoned-subscription leak (F-M3-1/F-M0-5) remain real upstream defects even though the
   contract path no longer exercises them.
4. Deferred intact: S-WIN C# resume (own step; SkipReason keep-or-delete rides it), the
   harness-hardening track, cookie capability (Q9 shape defined), `BackgroundTransfer`.
