# Step 03 — SwiftUI spike app

**Phase 1 · Spike.** Read first: [VISION.md](../VISION.md) (verification ladder — this app is
rung 2: hand-written *as-if-generated*; the "no constraint literals in shells" rule),
[ARCHITECTURE.md](../ARCHITECTURE.md) (§1 the three verbs, §2 validation timing + value-bound
async verdicts, §4 drafts + live rebase + conflict ceiling, §6 the **text echo rule**, §8 the
two decisions this step lands), [ROADMAP.md](../ROADMAP.md) (working agreement), and the two
prior handoffs ([step-01 report](step-01-report.md) · [step-02 report](step-02-report.md)).
This step puts a real editing surface on the bindings step 02 confirmed.

## Goal

Put a **real SwiftUI form on the contract** and observe whether the UX promises hold *without
the shell restating any "what"* (ARCHITECTURE §2 litmus test: shells add *when*, never *what*).
Four behaviors are on trial, plus the two core fixes the design owes since step 01:

1. **Text echo rule** — the cursor survives per-keystroke `try_set` + core sanitization
   (ARCHITECTURE §6; "Validated in the Swift spike" — this is that spike).
2. **Conflict UI** — keep-mine / take-theirs rendered and resolved from snapshot data alone.
3. **Live rebase** — a background canonical change flows into an open draft; clean fields adopt
   silently, dirty fields conflict.
4. **Submit flow** — validation report, conflict refusal, and success (canonical updates via
   the stream, never the shell's own echo) are all honest.

And it **lands the two decisions from ARCHITECTURE §8** that were explicitly deferred here
(step-02 Non-goals; ROADMAP step 03): **value-bound async-verdict reset (invariant 13, with its
test)** and **failed `submit` returns the draft handle**. It also closes the observability gap
step-02 found: the async-check sub-state must reach the snapshot so a spinner is possible
(step-02 report finding 7).

**The output that matters is evidence.** A green suite and a running app are necessary but not
the deliverable — the answered probe matrix, the executed manual protocol, and the friction log
in `docs/steps/step-03-report.md` are. Every place the UI is forced to restate a constraint,
re-implement sanitization, or work around the contract is a **freeze finding** — record it,
don't paper over it.

## Non-goals (hard boundaries)

- **No iOS target.** macOS only (the `swift test` + `swift run` path from mise). Keep the
  SwiftUI code free of macOS-only assumptions where it's free to, but don't build/verify iOS.
- **No create-flow, no persistence, no real server, no i18n infrastructure.** A `key → English
  template` dictionary in Swift is fine for rendering errors (the *params* come from
  `ErrorData`; the shell only owns the sentence, never the numbers). No stash/restore (Phase 2).
- **No Xcode project, no XCUITest.** SwiftPM only, driven by mise, so the repo stays
  CLI-buildable and `mise run check` stays Xcode-free.
- **No Kotlin/Android, no macros, no perf optimization, no published crates.**
- **No core changes beyond the whitelist in Deliverable A.** Everything else in `bolted-core`
  and `spike-profile` stays frozen; if the UI seems to need more, that is a finding to record,
  not a change to make (smallest-reversible-choice rule, or stop-and-report if structural).
- **Do not resolve any ARCHITECTURE §9 OPEN question.** Two of them get *evidence* here and
  must be left OPEN: **F2** (commit policy for a never-run check — an edit that resets to
  `Idle` makes `validate()` pass again; record how often that happens naturally) and **F6**
  (a conflicted field edited to equal *theirs* stays `Conflicted` — record the UX verdict).
  Also record, don't decide: the focused-but-untouched-field-during-rebase feel (§9).

## Deliverable A — core fixes (the whitelist; everything else in core is frozen)

These are the two ARCHITECTURE §8 decisions. Land them **with their tests**; they are the only
edits permitted to `bolted-core` and `spike-profile`.

### A1. `bolted-core` — two additive changes

- **`SingleFlight::reset(&mut self)`** — return the check to `Idle` **and bump `seq`** (symmetric
  with `begin`), so any still-outstanding `CheckToken` is stale by sequence as well as by state.
  This reuses the invariant-10 staleness machinery; a completion arriving after a `reset` is
  ignored exactly as a superseded one is. *(Note: `SingleFlight::state()` already exists — the
  getter step-02 finding 7 asks for is present; only `reset` is missing. Do not re-add `state`.)*
  Add a focused unit test in `single_flight.rs` (reset → `Idle`; a `complete` of a pre-reset
  token returns `false`).
- **`Store::submit` returns the handle on failure.** Today `submit(&mut self, handle:
  DraftHandle<D>)` takes the handle by value and, on every refusal path (`Orphaned` /
  `Conflicted` / `Validation`), returns early — dropping the handle and **destroying the user's
  edit session** (step-01 friction F3; the pre-checks at `store.rs:119-133` already run under a
  borrow, so only the success path needs ownership). Change the signature so the caller gets the
  handle back on refusal. Recommended shape (naming is implementer latitude, **semantics are
  not**):

  ```rust
  pub struct SubmitFailure<D: StoreDraft> {
      pub handle: DraftHandle<D>,
      pub error: SubmitError<D::FieldId>,
  }
  pub fn submit(&mut self, handle: DraftHandle<D>) -> Result<(), SubmitFailure<D>>;
  ```

  Consume the handle only on the success path; on each refusal, return `Err(SubmitFailure {
  handle, error })`. The defensive `Rc::try_unwrap` `Err` branch (`store.rs:137-142`, unreachable
  under single-ownership) must also hand the handle back rather than drop it — reconstruct a
  `DraftHandle` from the `rc` it returns (the field is module-private, so `store.rs` may). Update
  the store callers in `spike-profile`'s tests (`invariants.rs`, `behaviors.rs`) to the new shape.

  **No FFI ripple:** the step-02 wrapper re-owns its own store loop and does *not* use
  `bolted_core::Store` (it already implements return-handle semantics via the tombstone). This
  fix keeps the pure-Rust `Store` honest for the Rust-web shell (step 04) that consumes it
  directly.

### A2. `spike-profile` — value-bound reset (invariant 13) + a check-state getter

- **Value-bound reset.** ARCHITECTURE §2/§8: *any change to the checked field's value — edit or
  rebase — resets the check to unchecked.* The checked field is `username`. Implement this
  **uniformly by value comparison**, not by enumerating call sites (enumerating risks missing a
  path):

  ```rust
  // capture the username's effective value, run the mutation, reset the check iff it changed
  fn with_username_guard<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
      let before = self.username.value().cloned();       // Option<Username>
      let out = f(self);
      if self.username.value() != before.as_ref() {
          self.username_check.reset();
      }
      out
  }
  ```

  Wrap the three mutation entry points that can move the username value: `try_set_username`,
  the `StoreDraft::rebase` body, and `resolve_keep_mine` / `resolve_take_theirs`. Comparing
  `value()` (an `Option<&Username>`, `None` when `Unset`/`Invalid`) gets every case right for
  free: an edit to a **different** valid value resets; an edit to the **same** value does **not**
  (consistent with value-based dirty); an edit to **invalid** resets (`Some→None`); a
  rebase that **adopts** theirs resets; a rebase that **conflicts** (yours preserved, value
  unchanged) does **not** reset — your verdict still endorses your value; `take_theirs` resets;
  `keep_mine` (value unchanged) does not.
- **`pub fn username_check_state(&self) -> &CheckState<Result<(), ErrorData>>`** on
  `ProfileDraft` — the getter that lets the FFI layer project the sub-state (finding 7). The
  field stays private; only a read getter is added. `validate()` is **unchanged** (a `Pending`
  or `Done(Err)` check still blocks as the `username_unique` rule; `Idle`/`Done(Ok)` still pass
  — the F2 gap is deliberately left as-is). The value-based reading governs the conflict case
  below: §2's "change to the checked field's value" is the precise form of §7's looser "rebase
  of the pinned field."
- **Invariant 13 test** — `i13_async_verdict_resets_on_value_change` in
  `crates/spike-profile/tests/invariants.rs`, immediately after `i12_*` (example-based). Cover
  every branch above: (a) complete a check to `Done(Ok)`, edit username to a *different* value →
  state is `Idle`; (b) edit to the *same* value → state still `Done(Ok)`; (c) non-dirty field,
  `apply_canonical`/`rebase` to a new username → `Idle`; (d) **dirty** field, rebase to a
  *conflicting* username (yours preserved) → still `Done(Ok)`; (e) `resolve_take_theirs(Username)`
  → `Idle`, `resolve_keep_mine(Username)` → still `Done(Ok)`. Touching ARCHITECTURE §7's
  invariant-13 parenthetical is not required here — leave that wording to the report/freeze.

## Deliverable B — FFI additions (`spike-profile-ffi`, still the only crate importing boltffi)

Additive projection only; the wrapper's re-owned store loop and emit-outside-lock discipline
(step-02) are unchanged.

### B1. Async-check sub-state in the snapshot

- New `#[data] UsernameCheckFfi { Unchecked | Pending | Passed | Failed { error: ErrorData } }`,
  a projection of core `CheckState<Result<(), ErrorData>>` (`Idle→Unchecked`,
  `Pending→Pending`, `Done(Ok)→Passed`, `Done(Err(e))→Failed{e}`), read via the new
  `username_check_state()` getter. Add `username_check: UsernameCheckFfi` to `ProfileSnapshot`,
  populated wherever snapshots are built.
- **This needs no new FFI method to make `Pending` observable.** The wrapper already emits a
  `Pending` snapshot inside `run_username_check` *after* `begin` and *before* the lock-free
  foreign call-out (step-02 report, feature 4). So a Swift checker that **blocks until the test
  releases it**, invoked via `runUsernameCheck()` on a **background** task, lets a main-actor
  stream consumer observe `Pending` before the verdict lands (see the probe test).

### B2. Constraint metadata over the boundary (kills shell-side literals)

- New `#[data] ConstraintFfi { Required | LenChars { min: u32, max: u32 } | Custom { key: String } }`
  mirroring core `Constraint` (the `&'static str` of `Custom` projects to `String`).
- New export `ProfileStoreFfi::constraints(field: ProfileFieldId) -> Vec<ConstraintFfi>`, a pure
  projection of the existing `ProfileField::constraints()` (already public, `profile.rs:38`). The
  app derives `maxLength`, char counters, and required markers **only** from this — there must be
  no numeric constraint literal anywhere in the Swift (ARCHITECTURE §1; greppable rule).

### B3. New probe XCTests in `apple/profile-probe`

A few tests proving the additions cross correctly (these belong with the existing XCTest package,
not the app): check-state transitions visible in snapshots including a genuinely observed
`Pending` (blocking-checker-on-a-background-task pattern); reset-on-edit visible *through FFI*
(complete a check → `trySetUsername` a new value → snapshot shows `Unchecked`); `constraints()`
round-trips the expected `Required`/`LenChars` for each field.

## Deliverable C — the SwiftUI app (`apple/profile-app`, a new SwiftPM package)

A macOS SwiftUI app that is the **hand-written stand-in for generated ViewModel + View glue** —
the same "write what the codegen would emit" discipline steps 01–02 used, now for the shell side.
It sizes step-10/11 (the per-language generator).

### C1. Package shape

- SwiftPM package under `apple/profile-app` (sibling of `apple/profile-probe`), **macOS 14+**
  (needs `@Observable`), depending on the generated `crates/spike-profile-ffi/dist/apple` package
  exactly as the probe does.
- Targets: a **library** `ProfileFeature` (the ViewModel + views + the error/localization map),
  a thin **`.executableTarget`** with the `@main` SwiftUI `App` (run via `swift run`), and a
  **`.testTarget`** for headless ViewModel tests. Automated coverage lives in the library/VM
  tests; the executable exists for the manual protocol (a GUI window is never run in CI).

### C2. ViewModel (`@Observable`, main-actor)

- **Owns the checkout.** On start: **subscribe to the draft's `snapshots()` first, *then* read
  one `snapshot()`**, and reconcile by `version`. Ordering matters — step-02 proved fresh
  subscriptions are **future-only** (they replay nothing); subscribing first guarantees no
  external change is lost in the gap, and the `version` stamp (draft `base_version`, which moves
  only on rebase/adopt) dedups a snapshot that also arrives on the stream. The store's canonical
  stream is subscribed the same way for the "server" pane.
- **The echo rule (the headline).** Per text field, the `TextField` is bound to a **local
  editing buffer** the user types into freely. On each keystroke the VM calls the matching
  `trySet*` (so validation, counters, and the debounced check all run per keystroke — the
  per-keystroke `try_set` bet is *exercised*, not bypassed), **but the core's sanitized value is
  never written back into the buffer while the field is focused** — that write-back is what moves
  the cursor. The buffer is refreshed from the snapshot (`Valid.value`, or `Invalid.raw` to keep
  the user's rejected text) **only** on blur or on an external change (rebase / take-theirs). This
  is ARCHITECTURE §6 made concrete: "the native control owns its text while focused; core `raw`
  is authoritative on blur/programmatic change." (Use `@FocusState` + `onChange`, or equivalent —
  the *mechanism* is latitude; the *invariant* "focused buffer is never overwritten from core" is
  not.)
- **Async check.** Debounce (a shell-taste constant — allowed; it's *when*, not *what*) then
  invoke `runUsernameCheck()` on a background task when the username is valid and dirty. Bind a
  spinner to `usernameCheck == .pending`; render `.failed` as the `username_unique` message.
  Because a value change resets the check (A2), typing during a pending check invalidates it —
  the spinner behavior falls out of the contract, not shell bookkeeping.

### C3. Views

The profile form — username / name / email text fields + the availability date pair — with, per
field: the error text (from the `key → template` map + `ErrorData` params), a char counter and
required marker **derived from `constraints()`**, and a dirty indicator. Plus: a **conflict
banner** per conflicted field showing mine (the field's own validity) vs theirs (and base) with
**keep-mine / take-theirs** buttons; a **submit** button rendering the returned report per-field
(and the conflict/orphaned outcomes); the spinner. And a **"server simulator" pane**: it shows
the canonical state (from the store stream) and offers buttons that call `applyCanonical(...)`
with preset mutations — this is the live-rebase / conflict driver, standing in for a backend.

After a successful submit the draft handle tombstones; the VM re-`checkout()`s and re-subscribes
(record the ergonomics of that hand-off — it informs the §9 draft-lifecycle question).

**Stretch (droppable, record if cut):** a second editor window over the same store (submit in one
→ watch the other rebase/conflict live) — the most convincing live-rebase demo, but not required.

### C4. mise wiring

- `[tasks."run:apple"]` → `swift run` in `apple/profile-app` (doctor-fail clearly if the Swift
  toolchain/SDK is absent, like `test:apple`).
- Extend `[tasks."test:apple"]` to run **both** Swift packages' tests (probe + app VM tests),
  each after its `pack:apple` dependency.
- Gitignore `apple/profile-app/.build/`. **`mise run check` stays Xcode-free** — do not fold any
  Swift task into it.

## Ordered milestones (milestone 1 is a clean, standalone checkpoint)

This step is deliberately large (two core fixes **and** a UI). Treat **milestone 1 as a
self-contained unit** — it is pure Rust, Xcode-free, green under `mise run check`, and could
stand as its own commit/PR before any Mac tooling is touched. If the session runs long, stop
after a completed milestone and report — a partial-but-clean result beats a rushed whole.

1. **Core fixes + invariant 13** (Deliverable A). `mise run check` green. *No Mac needed.*
2. **FFI additions + probe tests** (Deliverable B). `mise run test:apple` green.
3. **App skeleton**: package + ViewModel + subscribe-first/version reconcile + the echo-rule
   binding; VM tests green.
4. **Full UI**: conflicts + resolution, the simulator pane, submit flow, spinner + debounce.
5. **Manual protocol** executed on a Mac; observations + measurements recorded.

## Probe matrix

Automated rows are VM-level `swift test` (no window) unless marked **Manual** — those need a
human at a running app (`mise run run:apple`) and are recorded in the report as observations.

**Echo rule**
- *Auto:* typing calls `trySet*` and updates core validity/counter, but the VM's focused buffer
  is not rewritten from core; on blur the buffer refreshes to the sanitized value; a rejected
  edit keeps `Invalid.raw` in the buffer.
- **Manual (the headline):** type fast into username with leading/trailing spaces and mixed
  case into email — the **cursor never jumps** and no character is eaten mid-word. This is the
  §6 claim on trial.

**Subscribe-race / observe contract**
- *Auto:* a deterministic test that applies a canonical change in the subscribe→get window and
  asserts the reconciled state is correct (nothing lost, no stale overwrite). Record whether the
  `version` stamp alone suffices or a per-emission sequence is wanted — a bolted-ffi pattern
  finding for the freeze.

**Live rebase**
- *Auto:* simulator mutation while a field is clean → the field adopts silently (snapshot
  updates, still `InSync`); while a field is dirty → `Conflicted`, mine preserved, banner data
  present.
- **Manual:** a focused-but-untouched field updating live under a rebase — record how it *feels*
  (§9 open item).

**Conflict resolution**
- *Auto:* `resolveKeepMine` → value=mine, base=theirs, still dirty, `InSync`; `resolveTakeTheirs`
  → value=theirs, clean; take-theirs on username also **resets the check** (i13 visible through
  the snapshot's `usernameCheck`).
- **Manual:** edit a conflicted field until it equals *theirs* — it **stays `Conflicted`** (F6);
  record whether that reads as correct or surprising.

**Async check**
- *Auto:* a debounce collapses a burst into a single check (single-flight); a value change during
  `Pending` invalidates the in-flight verdict (reset + stale seq — no late endorsement); `Failed`
  surfaces the `username_unique` violation; `Unchecked`↔`Pending`↔`Passed/Failed` transitions are
  observable in snapshots.
- **Manual:** with a ~1 s-delay checker, the spinner appears and clears; typing through it never
  shows a verdict for the wrong text.

**Submit flow**
- *Auto:* an invalid field → `Validation { report }` rendered per-field + rule errors; a
  conflicted draft → `Conflicted { fields }`; success → the canonical/server pane updates **via
  the store stream** (not the shell's own input echoed back), and the editor re-checks-out; a
  **failed** submit leaves the draft alive and editable (F3 end-to-end).
- Record how often a **never-checked** username reaches a passing submit naturally (F2 evidence).

## Measurements (record; **no pass/fail thresholds** — a macOS baseline, not a gate)

Apple numbers carry no weight for the JNI bet (step 05 re-measures the worst case). Record:

- Per-keystroke main-actor `trySet*` → snapshot → view-update latency under real typing (does
  per-keystroke `try_set` *feel* instant on the main actor? — see the latency kill below).
- Debounce interval chosen and the observed number of `runUsernameCheck` calls per burst.
- Any `dist/apple` size / pack-time deltas from the new DTOs (continuity with step-02's baseline).

## Kill criteria ("broken" = no reasonable shell-side pattern fixes it)

Wrapper/VM awkwardness, an ugly binding, or a design rule you had to adopt = **friction finding**,
not a kill. A kill is one of these; on hitting one, **stop, write the minimal repro, and report**
— do not engineer around it.

- **Echo rule** — keeping the cursor stable is impossible without the shell **re-implementing
  sanitization or restating a constraint** (the §2 litmus test fails). That breaks the core
  premise that shells add *when*, never *what* → architecture session.
- **Observe contract** — the subscribe/get gap **provably cannot be closed** with the `version`
  stamp (a test demonstrates a lost or unorderable external change) → design session on the
  `observe` verb / snapshot sequencing.
- **Typing latency** — per-keystroke `try_set` on the main actor causes **perceptible lag**
  (orders above step-02's ~3 µs/call), i.e. the per-keystroke bet fails even on Apple, the best
  case → stop and report (it previews the step-05 JNI kill and is a VISION bet-1 concern).

Softer misfires — blur-refresh feels abrupt, the conflict banner is clunky, the spinner flickers —
are friction findings for the freeze, not kills.

## Exit checklist

- [ ] `mise run check` green (workspace incl. the two core edits; fmt, clippy `-D warnings`,
      tests) — **Xcode-free**.
- [ ] `mise run test:apple` green from a clean clone on a Mac with Xcode (both Swift packages).
- [ ] `mise run run:apple` launches the app.
- [ ] Invariant 13 present as a named passing test; `SingleFlight::reset` and the `Store::submit`
      return-handle change landed with their tests; `bolted-core`/`spike-profile` changes are
      confined to Deliverable A (`git diff` shows nothing else).
- [ ] All probe-matrix rows answered with *observed* behavior; the **manual protocol** executed
      and its observations recorded.
- [ ] `docs/steps/step-03-report.md` written: what was built; the four behaviors' verdicts (echo
      rule especially — did the cursor survive?); §9 evidence for **F2**, **F6**, and
      focused-field-during-rebase; whether the `version` stamp sufficed for the observe race;
      the two core fixes and any friction landing them; measurements.
- [ ] ROADMAP.md status updated (03 → done, 04 → ready).

## If you hit a wall

Same rule as steps 01–02 (CLAUDE.md): an omitted decision → the smallest reversible choice,
recorded in the report. A **structural** conflict — needing a trait/visibility change beyond the
Deliverable-A whitelist, a new invariant, an ARCHITECTURE edit, or resolving a §9 OPEN question —
means **stop and record the question** for a design session; do not resolve it here. The kill
criteria above are the explicit stop-and-report triggers, and hitting one is a *successful* probe
outcome, not a failure of the step.
