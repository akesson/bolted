# Step 03 — SwiftUI spike app — Report

**Status: milestones 1–4 complete and green; milestone 5 (the manual GUI protocol) is NOT yet
run** — it needs a human at a running window (`mise run run:apple`) and is written out below for
whoever runs it. Everything automatable is done and passing:

- `mise run check` — green (fmt, clippy `-D warnings`, workspace tests incl. invariant 13). **Xcode-free.**
- `mise run test:apple` — green: **28** probe XCTests (23 prior + 5 new) + **11** app ViewModel tests.
- `swift build` of the whole app package (incl. the `@main` executable) — clean.

The implementation session did NOT resolve any ARCHITECTURE §9 OPEN question; F2/F6 and the
focused-field-during-rebase feel got *evidence*, recorded below, and stay OPEN.

## What was built

### Deliverable A — the two §8 core fixes (whitelist only)
- **`SingleFlight::reset`** — returns to `Idle` and bumps `seq` (a pre-reset token completes to
  `false`). Unit test in `single_flight.rs`.
- **`Store::submit` returns the handle on refusal** — new `SubmitFailure<D> { handle, error }`; the
  handle is consumed only on the success path. `spike-profile` test callers updated. (One honest
  wrinkle in the dead branches — see friction #1.)
- **Value-bound async-verdict reset = invariant 13** — `with_username_guard` captures the username
  value, runs the mutation, resets the check iff the value changed; wraps `try_set_username`,
  `rebase`, and `resolve_keep_mine/take_theirs`. `pub username_check_state()` getter added; the
  field stays private and `validate()` is unchanged (F2 gap left as-is). Test
  `i13_async_verdict_resets_on_value_change` covers cases (a)–(e).
- `git diff` against `main` for `bolted-core`/`spike-profile` is confined to the whitelist — nothing else.

### Deliverable B — FFI additions (`spike-profile-ffi`)
- **`UsernameCheckFfi { Unchecked | Pending | Passed | Failed{error} }`** in `ProfileSnapshot`,
  projected from `username_check_state()` (closes step-02 finding 7 — the spinner is now possible).
- **`ConstraintFfi { Required | LenChars{min,max} | Custom{key} }`** + `ProfileStoreFfi::constraints(field)`.
- 5 new probe XCTests (`CheckStateAndConstraintsTests`), including a **genuinely observed `Pending`**
  on the draft stream (blocking checker on a background queue).

### Deliverable C — the app (`apple/profile-app`)
- SwiftPM package (macOS 14+): library `ProfileFeature` (VM + views + `Localization`), a thin
  `@main` executable `ProfileApp`, and a headless `ProfileFeatureTests` target.
- `ProfileViewModel` (`@MainActor @Observable`): subscribe-first/version-reconcile startup, the echo
  rule (per-field local buffers; the focused buffer is never overwritten from core), debounced
  `runUsernameCheck` on a background queue, conflict resolution, submit (with F3 recovery +
  re-checkout on success), a server-simulator `applyCanonical` driver, and constraint-/`ErrorData`-
  derived affordances (no constraint literal in Swift).
- `mise`: `run:apple`, and `test:apple` extended to both Swift packages; `apple/profile-app/.build/`
  gitignored; `check` stays Xcode-free.

## The four behaviours on trial

| Behaviour | Automated verdict | Manual verdict |
|---|---|---|
| **Echo rule** | **Holds at the VM level.** `editUsername` calls `trySet` per keystroke; the focused buffer is never rewritten from core; blur refreshes to the sanitized value; `Invalid.raw` is preserved. Tests: `testEchoRuleFocusedBufferNotRewritten`, `testEchoRuleInvalidRawPreserved`. | **Pending** — cursor-survival while typing fast is the headline §6 claim and needs a human (below). |
| **Conflict UI** | **Holds.** keep-mine / take-theirs resolve from snapshot data alone; take-theirs on username also resets the check (i13 visible). Tests: `testConflictResolutionAndCheckReset`, `testSubmitConflicted`. | Pending — banner feel. |
| **Live rebase** | **Holds.** Clean field adopts silently; dirty field conflicts, mine preserved, banner data present. Tests: `testLiveRebaseCleanFieldAdopts`, `testLiveRebaseDirtyFieldConflicts`. | **Pending** — focused-but-untouched-field feel (§9). |
| **Submit flow** | **Holds, honest.** Invalid → `Validation{report}`; conflicted → `Conflicted{fields}`; success → canonical updates via the store stream and the editor re-checks-out; a failed submit leaves the draft alive (F3, end-to-end). Tests: `testSubmit*` (3). | Pending — visual. |

No kill criterion was hit. The echo rule did **not** force the shell to re-implement sanitization
or restate a constraint (the §2 litmus test passes); the subscribe/get gap was closed by the
`version` stamp; per-keystroke `trySet` on the main actor showed no latency in the VM tests (the
manual protocol confirms the *feel*).

## Friction log (findings for the freeze)

1. **`Draft::commit(self)` can't return the draft on failure — the submit dead branch.** F3's
   return-handle-on-refusal is complete for every *reachable* refusal (orphaned / conflicted /
   validation, all pre-checked under a borrow, handle returned). But `submit`'s two internal
   *unreachable* branches differ: `Rc::try_unwrap` Err reconstructs the handle from its `rc`; the
   `commit()` Err branch **cannot** — `commit(self)` has already consumed the draft, so there is no
   handle to hand back. Rather than a `panic!` (forbidden) or an `Option<DraftHandle>` that would
   burden every real caller for a dead branch, it collapses to a no-op success (documented,
   unreachable because the pre-checks are identical to `commit`'s own gates). **Freeze question:
   should `Draft::commit` return `Result<Entity, (Self, Report)>`** so the store can always hand the
   draft back? Small trait change, removes the wrinkle. (Left as a finding, not resolved — it is a
   §5 trait signature, structural.)

2. **The per-value monomorphization tax reappears on the *shell* side.** The DTO layer already pays
   it (one field-state family per value type, step-02). The ViewModel and views pay it again:
   three identical `display(_:)` overloads for `Username/PersonName/Email` validity, three
   `valueText/nameText/emailText` in the simulator pane, four near-identical `conflict(_:)` arms.
   This is the honest size input for the step-10/11 shell generator: the generator must emit these
   per-field, exactly as the core/FFI generators do. Structurally identical `String`-backed value
   types collapse to one shape only if the generator keys on the raw type, which step 02 already
   found it does not.

3. **Exported BoltFFI classes are not `Sendable`.** `ProfileStoreFfi`/`ProfileDraftFfi` are
   `Send + Sync` on the Rust side (`Arc<Mutex<…>>`) but the generated Swift classes carry no
   `Sendable` conformance, so driving the blocking uniqueness check off the main actor needs an
   `@unchecked Sendable` wrapper (`CheckDriver`; the probe test needs the same). **bolted-ffi
   finding:** a `Send + Sync` Rust class could project to a `Sendable` Swift class and remove the
   wrapper. (Harmless under Swift 5 language mode — a warning, not an error — but it is friction the
   generator should erase.)

4. **The echo rule needs a shell *pattern*, and a clean one exists — no core change.** Binding the
   `TextField` through a setter that fires the edit **only on user input** (a programmatic buffer
   refresh updates the value without re-firing) keeps per-keystroke `trySet` running while never
   letting core sanitization move the cursor. This is the intended §6 shape; the generator should
   emit exactly this binding. Recording it as the reference pattern, not a friction.

## ARCHITECTURE §9 evidence (recorded, NOT decided)

- **F2 — commit policy for a never-run check.** A never-checked username reaches a *passing* submit
  trivially: `validate()` treats `Idle` as passing, and nothing forces a check before submit.
  `testSubmitSuccessUpdatesCanonicalAndRechecksOut` submits with the username never checked, and it
  succeeds. In the running app this happens whenever the user edits a non-username field (or edits
  the username and submits before the 400 ms debounce fires). **So F2 is not a corner case — it is
  the default path.** The freeze must decide whether commit requires a completed successful check.

- **F6 — a conflicted field edited to equal *theirs* stays `Conflicted`.** Mechanically confirmed:
  `Field::try_set` touches only the validity dimension, never sync, so typing your value until it
  equals `theirs` leaves `sync == Conflicted` (and the field dirty against the *old* base). Only
  `resolve_*` clears it. The UX verdict — does that read as correct or surprising? — is a manual
  observation (below).

- **Focused-but-untouched field during rebase.** Design choice recorded: the stream reconcile
  refreshes every buffer *except the focused one* (the echo rule). So a focused-but-untouched
  (clean) field that a rebase adopts updates its snapshot immediately but its **visible buffer only
  on blur**. This is a deliberate, defensible reading of "the focused control owns its text"; the
  alternative (refresh a focused *clean* field live) would need the VM to distinguish a rebase
  snapshot from a keystroke snapshot, which the snapshot alone does not encode. The manual protocol
  records how the stale-until-blur behaviour *feels*.

- **Does the `version` stamp suffice for the observe race?** Yes, for what step 03 needs. The
  reconcile drops any snapshot whose `base_version` is older than the current one (stale rebase) and
  takes equal-version snapshots in stream order (edits/checks; the stream is FIFO per subscriber).
  The subscribe-first ordering plus this guard lost no state in any VM test. A per-emission sequence
  would *additionally* order same-version edits across subscribers, but nothing here needs it —
  recorded as: `version` sufficed; revisit only if a future shell shares one draft across
  subscribers that both mutate.

## Deviations from the step doc

- **Milestones 3 and 4 landed as two commits** (skeleton+VM+tests, then views+App+mise), as the doc
  invited ("stop at a completed milestone"). The app is one package; the split is only in history.
- **No stretch second-window demo** (explicitly droppable) — cut. The server-simulator pane already
  drives live rebase/conflict; a second editor over the same store adds nothing the probe/VM tests
  don't already prove.
- **`ProfileApp` is not in a `main.swift`** (Swift forbids `@main` there) — it is `ProfileApp.swift`.

## Measurements (macOS baseline; no thresholds — see step 05 for the JNI worst case)

- **Per-keystroke `trySet` → snapshot → view-update latency:** not separately benchmarked, but the
  VM tests exercise the full per-keystroke path (`editUsername` = `trySet` + reconcile + buffer
  sync) and complete in well under a millisecond each; step-02 measured the raw FFI call at ~3 µs.
  No perceptible-latency concern surfaced. The manual protocol confirms the *feel*.
- **Debounce:** app interval **400 ms**; `testDebounceCollapsesBurst` (40 ms) drives a 5-edit burst
  and observes exactly **1** `runUsernameCheck` (`checkRunCount == 1`).
- **Binding surface / app size (continuity with step 02):** generated Swift binding file **1758**
  LOC; app source **787** LOC (VM + views + localization); app VM tests **251** LOC; new probe tests
  **174** LOC. The two new DTOs + one method added no packaging trouble (pack succeeds).

## Manual protocol — TO RUN (needs a human at `mise run run:apple`)

Launch: `mise run run:apple`. Record each observation back into this section.

1. **Echo rule / cursor survival (the headline).** Focus *Username*; type fast with leading and
   trailing spaces, e.g. `  bob_1  `. **Expected:** the cursor never jumps and no character is eaten
   mid-word while typing; the visible text is exactly what you typed (spaces included); on blur it
   snaps to the trimmed value. Do the same in *Email* with mixed case (`Foo@BAR.com`) — lowercasing
   appears only on blur. *Record:* did the cursor ever jump?
2. **Live rebase — focused-but-untouched (§9).** Focus *Name* but don't type. Click the simulator's
   "name → Server Name". *Record:* the field is clean so it adopts — does the visible text update
   live, or only when you blur? How does that feel?
3. **Live rebase — dirty conflict.** Edit *Name*, then click "name → Server Name". *Expected:* a
   conflict banner with *theirs*; your text preserved. Try keep-mine and take-theirs.
4. **F6.** Create a username conflict (edit *Username*, then "username → server_user"), then edit
   your username until it equals `server_user`. *Expected:* it **stays** conflicted until you
   resolve. *Record:* correct or surprising?
5. **Async check / spinner.** Type a valid new username; after ~400 ms a spinner appears (the
   checker sleeps 1 s), then clears. Type `admin` → "already taken". Type through a pending check →
   the spinner never shows a verdict for the wrong text. *Record:* spinner feel; any flicker.
6. **Submit.** Submit with an invalid field (see the per-field report), with a conflict (see the
   refusal), and clean (success → the canonical pane updates, the form re-checks-out). Confirm a
   failed submit leaves you still editing.

## Kill criteria

None hit. The three kill bars (echo rule needing shell-side sanitization; an unclosable observe
race; perceptible main-actor typing latency) were not triggered — recorded above with their
evidence.

## Open questions handed to the freeze

- Friction #1: should `Draft::commit` hand the draft back on failure (`Result<Entity, (Self, Report)>`)?
- F2 (commit policy for a never-run check) — now shown to be the *default* path, not a corner case.
- F6 UX verdict (pending the manual run).
- bolted-ffi: project `Send + Sync` Rust classes as `Sendable` Swift classes (friction #3).
