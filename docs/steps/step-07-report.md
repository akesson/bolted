# Step 07 — Kotlin/Compose spike app — Report

**Status: done. Four of five kill criteria cleared; the fifth (hardware chattiness) is
UNASSESSED — no device was attached, and no number was invented.**

The frozen contract now runs on a real Android app: a Compose form, a `ViewModel`-scoped draft, and
the one mechanism Phase 2 had left undesigned — **stash and restore across process death**.

**The two things to read if you read nothing else:**

1. Planning this step found a **verified defect in the frozen core**. `rebase` was a two-way merge
   wearing a three-way merge's name, and every one of five test tiers had been stepping over the bug
   for six steps. It is fixed (D14/C19).
2. **A Compose shell cannot read core state by calling a method on its ViewModel.** Two UI tests
   failed on that, and neither Swift's `@Observable` nor Leptos's signals could ever have taught us
   so. Step 10's generator needs the rule written down.

## Green

| Verb | Result |
|------|--------|
| `mise run check` | green — fmt, clippy `-D warnings`, **86** workspace tests (was 79). `bolted-core` still zero-dependency, `#![forbid(unsafe_code)]` |
| `mise run test:web` | **8/8** headless wasm |
| `mise run test:apple` | **39** probe XCTests (was 35) + **14** VM tests (was 13) |
| `mise run test:android` | **43/43** on the headless ART emulator (was 39) |
| `mise run test:android:app` | **35/35** — 15 VM, 8 **Compose UI**, 6 stash/restore, 6 localization |
| `mise run test:android:hazard` | **3/3** isolated H2 probes |
| `mise run bench:android:device` | **NOT RUN** — no physical device attached (kill criterion 4) |
| `mise run test:apple:ui` | **not run** — still GUI-gated (step-06 deviation 4 stands) |

Conformance: **21 normative IDs, 30 tests**, drift check green.

## The defect the step found before it started

`Field::rebase` compared `mine` against `theirs` but never `theirs` against `base`. Since the store
rebases *every* field of a draft on *every* canonical change, a field the server never touched was
routinely rebased onto its own ancestor — and entered `Conflicted { theirs }`, where `theirs` **was
that ancestor**. The user got a "keep mine / take theirs" banner whose two buttons did the same
thing, and a `commit` refused with `Conflicted` and nothing to resolve. That is C14's disease
arriving through a different door.

Edit `name`, let the server change `email`, and `name` conflicts. That is not an exotic path.

**Why six steps and five test tiers missed it** is the more useful finding:

- **C03's property test never sampled it.** It drew `base`, `mine` and `theirs` independently and
  assumed only `mine != base` and `theirs != mine`. Two random 3–20 character strings are essentially
  never equal, so `theirs == base` was never generated. *An `assume` set missing a precondition does
  not weaken the property — it silently asserts the bug.*
- **`c08_rebase_reruns_tier2_rule`, inside the frozen conformance suite, had been producing a
  spurious conflict on `email` since it was written**, and passing, because it only asserted on the
  rule.
- **`echo_rule_focused_buffer_is_never_rewritten_from_core` (web) has been dirtying `username` and
  then moving `name` on the server since step 04**, and only ever asserted on the buffers.

The fix (D14) is a four-line early-out: `theirs == base` ⇒ nobody else moved this field, so keep
mine, clear any conflict, stay `InSync`. It also clears a conflict when canonical moves *back* to the
ancestor. C03 gained its missing precondition; **C19** pins both halves; regression tests were added
at every tier that should have caught it — `bolted-core` units, the conformance suite, the web
controller, the Swift VM, and the Kotlin probe on ART.

Each was then **verified to fail with the fix reverted**. One did not: `rebase_is_idempotent` passes
either way. `rebase` was always idempotent, the plan's claim that D14 introduced it was wrong, and it
is corrected in place — in the step doc, in C19's wording, and in `Field::rebase`'s doc comment.

## Stash and restore (D15, C20 + C21)

ARCHITECTURE §9's last undesigned Phase-2 mechanism. The stash is
`{base_version, orphaned, per-field (raw, base)}` and restore is `Store::adopt(D::from_stash(..))`,
which rebases the reconstructed draft onto whatever canonical says **now**.

**Two things are deliberately absent, and their absence is the design.**

- **`sync` is not stashed.** A conflict names a canonical value the server may no longer hold;
  restoring it restores a lie. It re-derives on the restoring rebase, against fresh canonical, and so
  names the right value. `c20_sync_is_not_stashed_and_re_derives_against_fresh_canonical` proves the
  restored conflict carries `their2`, not the `their1` we died holding.
- **The async verdict is not stashed.** It endorses a value against a server state that may have
  moved. A restored checked field is `Unchecked`, and **C16 then refuses to submit it while dirty**.

That second one is the nicest thing this step found: **C13 and C16, written for a different reason,
make restore safe with no new invariant.** Stash/restore needed no rule of its own about verdicts.

The **ancestor** is stashed, and it carries every prior resolution with it. A `resolve_keep_mine`d
field has `base == old theirs`; if canonical still holds that value, D14's early-out leaves the
restored field dirty and `InSync` and the user is not asked to decide twice
(`c21_a_resolved_conflict_stays_resolved_across_restore`). That single test is the whole argument for
stashing the ancestor instead of replaying raw text onto a fresh checkout — the option that would
have silently overwritten the server.

`Store::adopt` is the store's only draft entry point; `checkout()` is `adopt(D::from_canonical(..))`,
which is only *true* because D14 made rebasing a draft onto the canonical it was just built from a
no-op.

Every new claim was **mutation-tested before it was trusted**: skip `adopt`'s rebase, skip the
orphan, keep the verdict, drop the ancestor — each mutant is killed by exactly the test that names
it. The drift check earned its keep too, failing the build for `c20_*` before C20 was a documented
row.

## Kill criteria

| # | Criterion | Verdict |
|---|---|---|
| 1 | Compose UI tests cannot run on a headless GMD | **Cleared, in M4, deliberately first.** |
| 2 | A `ViewModel`-scoped draft does not survive a config change | **Cleared.** |
| 3 | `from_stash` + `adopt` cannot reproduce the pre-death draft | **Cleared.** C20 and C21 pass, on the core, across BoltFFI, and through a real `Bundle`/`Parcel`. |
| 4 | The chattiness bar breaks on physical hardware | **UNASSESSED.** No device. See below. |
| 5 | `Store::adopt` cannot express `checkout` | **Cleared.** `checkout()` *is* `adopt(from_canonical(..))`, in the core and in the FFI wrapper. |

**Kill criterion 1 is the headline of the step after the rebase fix.** Android has a headless UI
tier. `ProfileFormTest` launches a real Activity, composes a real tree, and asserts against real
semantics nodes on the same `aosp-atd` Gradle-Managed Device the probe uses — no window server, no
Accessibility permission, no GUI session, no human. Proven rather than observed: the skeleton
assertion was inverted and the suite failed with `Failed to assert the following: (Text +
EditableText = [pong: NOT-THIS])`.

That is what step 03 could not do. `test:apple:ui` drives a real window, needs a logged-in GUI
session plus Accessibility permission, and **has still never run in this project**. Two of Bolted's
three shells can now verify their UI without a human. The one that cannot is Apple's.

**Kill criterion 2**: rotation destroys the Activity (`configChanges` is deliberately *not* declared,
or the app would pass by never taking the test) and the `ViewModelStore` survives, so the edit session
and the core-side handle simply persist — no stash, no `close()`, no re-checkout.
`onCleared()` closes the draft: `live_drafts_after_close = 0` (C18).

**Kill criterion 4 is open, and it is the one number this project still owes itself.** `adb devices`
was empty for the whole session. `bench:android:device` and `PhysicalChattinessProbe` are written and
double-gated — the mise verb rejects an `emulator-*` serial before Gradle starts, and the probe
rejects an emulator `Build.FINGERPRINT`. Both gates were *tested*: pointing the suite at the headless
emulator with `-Pbolted.hw` selects exactly the four hardware tests and fails all four with
*"refuses to run on an emulator … Step 05 already measured that, and it is a lower bound, not a
result."* The chattiness kill criterion therefore still rests on step 05's 12–13 µs, measured on the
right VM and the wrong CPU.

> **To close it: connect a phone with USB debugging on and run `mise run bench:android:device`.**

## Deviations from the step doc

1. **The plan claimed D14 "makes `rebase` idempotent".** It does not; `rebase` already was. Corrected
   in the step doc, in C19's statement, and in the code comment. `rebase_is_idempotent` is a guard,
   not a regression test, and says so — `Store::adopt` leans on the property.

2. **§9 went from 8 entries to 8, not to 7.** Stash/restore is resolved and removed. But D15 *opened*
   one: **stash schema evolution.** The stash is the framework's first untrusted input — bytes the OS
   kept while we were dead, possibly written by an older build. C01 says raw forms roundtrip, so an
   ancestor that no longer parses means a constraint was tightened between releases. Step 07 degrades
   that field to create-flow, which is safe but silent. Recorded in §9, owned by step 10 and
   `bolted-check`. Closing a question honestly sometimes costs a new one.

3. **`ProfileViewModel` carries one test-only observable**, `liveDraftsAfterClose`, read between
   `draft.close()` and `store.close()`. There is nowhere else it can live: querying a *closed* store
   is itself use-after-close, which is silent UB today (§9). A `bolted-ffi` that raised `DraftClosed`
   would let C18's assertion live outside the ViewModel. Recorded as an argument for that fix.

4. **The FFI wrapper grew a third copy of store logic** (`adopt_locked`). `Store` is `Rc<RefCell<_>>`
   and cannot be `Send`, so `spike-profile-ffi` re-owns the loop; `restore` doubled the surface that
   must agree with `bolted_core::Store` by discipline alone. Left as-is — step 08 owns the
   concurrency decision — but the duplication is now larger, and that is the point of measuring it.

5. **`StashCodec.kt` is hand-rolled JSON.** BoltFFI emits DTOs with no `Parcelable`, no
   `Serializable`, no `kotlinx.serialization`. Its length is the argument for step 10 emitting
   `@Parcelize` on Android and `Codable` on Apple.

## Friction log (input to steps 08–11)

1. **A Compose shell must never read core state by calling a method on the ViewModel.** This cost two
   red tests, and it is the most transferable thing in the step.

   `vm.conflict(field)` reached into a `StateFlow`. Compose observes `State` reads made *during
   composition*; a `StateFlow` read is invisible to it. Worse, **strong skipping** — on by default
   since the Compose compiler moved into Kotlin 2.x — makes a row skippable when its parameters are
   unchanged, and `vm` is the same instance forever. So the core conflicted, the ViewModel knew, and
   the UI never asked again. Both conflict-banner tests timed out waiting for a banner that could
   never appear.

   Fixed by threading the snapshot through as a parameter, so the dependency is something Compose can
   see. **Neither sibling shell could have found this**: Swift's `@Observable` tracks property reads,
   Leptos has signals. It is Compose-specific, invisible to unit tests, and only a UI test on a real
   render tree catches it — which is precisely why kill criterion 1 mattered.

   *Generator rule (step 10): a Compose shell takes core state as a parameter or reads it through
   `collectAsStateWithLifecycle`. It never calls a method that reads state.*

2. **Step-06 friction 7 is now a test, and it should become a `bolted-check` rule.**
   `LocalizationCoverageTest` drives every error key **through the real core** and fails if any
   renders as its own identifier. It asserts nothing against a hardcoded list, because a list is only
   as good as the person maintaining it. All 11 keys covered. This is the shape the check should
   have per target.

3. **C16's cost lands exactly where step 06 predicted.** A dirty username with no verdict blocks
   submit, and on the frame after a keystroke — and on the frame after a **restore**, where C20 drops
   the verdict on purpose — that is a form still being filled in, not a mistake. `username_check_required`
   renders as progress, never in red. `Localization.isProgress` exists for that, and both the VM and
   the Compose tiers assert it. A shell that got this wrong would teach users to ignore red.

4. **`liveDraftCount()` means two different things on the two sides of the FFI.** In Rust it is
   "drafts the store would rebase"; in the wrapper it is "un-submitted drafts". They agree everywhere
   C18 looks, and disagree on exactly the two drafts that are present-but-never-rebased: a **restored
   orphan** and a **create-flow draft** — 0 in the core, 1 in the wrapper, both now asserted on both
   sides. Same name, two semantics, across a boundary that step 10 will *generate*. Pin it to a
   C-ID.

5. **Dedup-by-raw-type (§9, step 09) is settled by example, and the answer is "it depends on the
   shape".** The snapshot DTOs need one struct per *value* type because `Validity<V>` mentions `V`.
   The stash DTOs mention only `V::Raw`, so three of four fields collapse onto one
   `TextFieldStashFfi`. Dedup is trivially right there and impossible here. A macro cannot answer the
   question once for the whole crate.

6. **`Option<record>` is the first optional record to cross on ART** (`DateRangeFieldStashFfi.raw`).
   It works; `StashProbe` pins it.

7. **A `ViewModel`'s `onCleared()` is the only place a Kotlin shell can free a draft**, and a test
   can only reach it by clearing a `ViewModelStore` — which is what `VmHost` does. Any generated
   Kotlin ViewModel must close in `onCleared()`, and `bolted-check` should refuse one that does not.

8. **Process death cannot be *caused* headlessly, only simulated faithfully.** `SavedStateHandle
   .savedStateProvider().saveState()` gives the exact `Bundle` the framework persists; pushing it
   through a real `Parcel` exercises the exact serialization the framework performs; destroying the
   old `ViewModel` and its store removes every core-side trace. What no headless test can prove is
   that Android *chose* to kill us. Stated plainly rather than papered over.

## What steps 08–11 inherit

- **Step 08** — make the conformance suite generic (now 21 IDs / 30 tests); decide the store
  concurrency model under step-02's three constraints, with the FFI wrapper's *third* hand-written
  copy of the store loop as fresh evidence; decide whether the store holds drafts weakly. `Store` now
  has `adopt` and `StoreDraft` has `is_based` — both must survive the extraction.
- **Step 09** — `#[bolted::value]` must never emit `Copy` (D8). Dedup by raw type is **per-shape**,
  not per-crate (friction 5). `#[bolted::entity]` must emit `stash`/`from_stash`/`is_based`.
- **Step 10** — the largest list. **Use-after-close must raise a typed error** (silent UB today, and
  friction 3 shows it distorting a ViewModel's shape). Emit `@Parcelize`/`Codable` for DTOs
  (deviation 5). Emit the Compose parameter-passing rule (friction 1). Pin `liveDraftCount`'s
  semantics to a C-ID (friction 4). Verify l10n key coverage per target (friction 2). Expose the
  split `begin`/`complete` so `Pending` is observable to a `snapshot()` caller. **Report the
  `boltffi pack android` bug upstream** and delete the workaround in `mise run pack:android`.
- **Step 11** — C# `IDisposable`: C18 is not optional there either, and `onCleared()`'s analogue is
  the one place a WinUI shell can free a draft.

## Exit checklist

- [x] `mise run check` green; `bolted-core` zero-dependency and `#![forbid(unsafe_code)]`.
- [x] `test:web`, `test:apple`, `test:android`, `test:android:app` green; `test:android:hazard` 3/3.
- [x] C19, C20, C21 in `docs/CONFORMANCE.md`, each with a test; drift test passes; C03's amended
      statement matches its amended proptest.
- [x] The spurious conflict has a regression test at **every** tier that should have caught it, and
      each was verified to fail with the fix reverted.
- [x] Kill criterion 1 answered with a green Compose UI test on a headless device — and the test was
      made to fail on purpose first.
- [ ] **Kill criterion 4 — no number from physical silicon. Explicitly unrun; nothing invented.**
- [x] No `unwrap`/`expect`/`panic!` in library code; **no constraint literal in `ProfileForm.kt`**
      (audited: the only `20`/`30` in the file are inside the comment daring you to look).
- [x] Every core error key has a `Localization.kt` template, enforced by a test that drives the core.
- [x] This report written and its numbers verified against the code.
- [x] ROADMAP updated; ARCHITECTURE §9 loses stash/restore and gains stash schema evolution.
