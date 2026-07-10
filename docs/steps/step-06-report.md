# Step 06 — Design freeze — Report

**Status: done. No kill criterion hit.** [ARCHITECTURE.md](../ARCHITECTURE.md) is **frozen (v1.0)**.
Every §9 question Phase 1 could answer is answered, in §8, with the alternative it beat. The 13
invariants are promoted to [CONFORMANCE.md](../CONFORMANCE.md) as **C01–C18**, each with a named test
and a build-time check that the document and the suite cannot drift apart.

At the owner's direction the freeze also **conformed the reference implementation**, so the contract
and the code agree today rather than in step 08. That is the bulk of the diff, and it is where the
freeze earned its findings: a decision you only write down is a decision you have not tested.

**The one thing to read if you read nothing else:** decision **D9**, as we agreed and froze it, was
**wrong** — and only implementing it proved so. See *Deviations*, item 1.

## Green

| Verb | Result |
|------|--------|
| `mise run check` | green — fmt, clippy `-D warnings`, **73** workspace tests. `bolted-core` still zero-dependency, `#![forbid(unsafe_code)]` |
| `mise run test:web` | **8/8** headless wasm |
| `mise run test:apple` | **35** probe XCTests (was 28) + **13** VM tests (was 12) |
| `mise run test:android` | **39/39** on a headless ART emulator (was 34) |
| `mise run test:android:hazard` | **3/3** isolated H2 probes |
| `mise run test:apple:ui` | **not run** — needs Xcode + a logged-in GUI session with Accessibility permission. `test4` was rewritten for C14 but is **unverified**; flagged below |

## The decisions

Thirteen, D1–D13, each in ARCHITECTURE §8 with its losing alternative. Four were put to the project
owner before any code was written, because a freeze is a commitment and on these the evidence pointed
without compelling: the scope of this step, D6 (the check policy), D5 (the handle contract), and D9.
The rest follow from evidence already in the step 01–05 reports.

**What the freeze is really about.** Phase 1's four probes agreed with each other more than they
disagreed, and three separate wounds turned out to be one:

- step-01 **F3** — a failed `submit` destroyed the draft.
- step-01 **F5** / Q4 — `commit` re-encoded conflicts and orphan status as *synthetic rule violations*
  because its error channel was a `ValidationReport`.
- step-03 **friction 1** — `commit(self)` could not hand the draft back, so `Store::submit` had an
  unreachable branch it had to apologise for in a comment.
- step-04 **friction 1** — `submit(handle)` consuming a `!Clone` handle could not be called on a
  handle living in a struct field, so every Rust shell needed a throwaway `checkout()` to vacate the
  slot.

All four dissolve at once if `commit` returns the draft with a *typed* error (**D4**) and the handle
becomes a lifecycle object that `submit` borrows (**D5**). And the shape that fixes them is not
invented: **step 02 had already discovered it**, because BoltFFI forced the FFI wrapper to make
foreign handles tombstones and to invent an `AlreadySubmitted` variant the core did not have. The
core API was the one lying about how drafts die.

Likewise **C13 + C16** together, and only together, make a client-side async verdict trustworthy: C13
guarantees a surviving `Done(Ok)` was computed for the value now in the field, C16 guarantees a dirty
field has a verdict at all. Two shells had shown that "never ask" was the *default* path.

## Kill criteria — none hit

1. **D4 cannot cross BoltFFI.** *Cleared.* A self-consuming `commit(self) -> Result<Entity, (Self,
   CommitError)>` crosses fine, because the FFI wrapper owns the draft it moved out of its own
   `HashMap` rather than a foreign handle. It also let the wrapper **delete its `pre_submit_check`
   pass** — the same duplication D4 removed from the core store.
2. **D5 makes the Rust shell materially worse.** *Cleared, measured rather than asserted.*
   `controller.rs` went **389 → 396** code lines (+1.8 %) for D5 while *losing* the scratch-checkout
   dance. Unreachable branches went **2 → 0** in `Store::submit` and **1 → 0** in the FFI `submit`. The
   tombstone's whole cost is two private helpers (`with_draft`, `edit`) and four `?` sites. (It is
   405 lines now; the further +9 is D9's `touched` flag, below.)
3. **D6 needs the core to model check-to-field pinning.** *Cleared.* One `match` arm in the
   feature's `validate()`, guarded by `self.username.is_dirty()`. `bolted-core` learned nothing.
4. **D2 breaks C04's symmetry.** *Cleared.* C04 and C14 are the same judgement with the two events in
   either order, and both land clean + `InSync` + base adopted.

## The conformance suite

`docs/CONFORMANCE.md` states C01–C18 normatively; `crates/spike-profile/tests/conformance.rs`
implements them in **21** test functions (C01 has four, one per value type), plus the drift test —
22 in the binary.

The load-bearing part is `conformance_manifest_has_a_test_for_every_id`, which parses the document
and fails if an ID has no test or a test has no ID. **A drift check that cannot fail is theatre**, so
before trusting it I made it fail in both directions: added `| C99 |` to the doc (→ *"C99 is normative
in docs/CONFORMANCE.md but has no `c99_*` test"*) and added a `c42_undocumented_claim` test (→ *"`c42_*`
exists but C42 is not a normative row"*). This is the suite's own rung-3 claim on VISION's ladder.

Making the suite **generic over a feature** is step 08's job, and deliberately not this step's: today
it would mean inventing a fixture trait with exactly one implementor.

## The stale version stamp, closed

Step 05 recorded that a draft snapshot's `version` never advanced. Verified here, and the consequence
is sharper than "stale": `ProfileViewModel.swift:295` reads

```swift
if snap.version < snapshot.version { return }   // "drop a stale rebase snapshot"
```

and **could never fire on a draft stream**. The subscribe-race mitigation step 02 shipped and step 03
relied on was dead code on drafts from the day it was written; it worked only on the canonical stream,
where the store stamps a live version. D7 threads the store version through `rebase`, and the Android
probe that *pinned the bug as expected behaviour* is now the probe that guards the fix: on ART,
`draft: 1 → 2` against `store: 1 → 2`.

## Deviations from the step doc

1. **D9's predicate is `touched`, not `dirty` — the frozen wording was wrong.**

   D9 was agreed as *"the control owns its text while focused **and dirty**"*. Implementing it, I
   wrote the obvious test and it failed:

   ```
   focus(username); edit_username("  alice  ")   // core trims -> "alice" == base -> CLEAN
   sim_set_name("Server Name")                   // an unrelated field moves on the server
   assert_eq!(username_buf(), "  alice  ");      // left: "alice"   right: "  alice  "
   ```

   Sanitization can make a field **clean while the control holds live keystrokes**. Keyed on `dirty`,
   any external refresh repaints it, eats the user's spaces and jumps the caret — precisely the defect
   §6's echo rule exists to prevent. `dirty` and `touched` agree in every other reachable state.

   The shipped rule is *"focused **and typed into** since the core last wrote the buffer"*: one
   shell-local `bool`. That is presentation state about a text control, not the core-side `touched`
   flag §8 rejects — §6 already says the control owns its text. Both shells carry a regression test
   named for the case. **This is a shell-level implementation detail, not a §9 structural question**,
   so I decided it rather than stopping; recording it here is the price.

   Cost: +9 code lines in the web controller (389 → 405 total across D5 and D9).

2. **`commit` clones four values on the success path.** Returning `Self` in the error arm means the
   draft cannot be dismembered with `into_valid()` before the last fallible step, or there is nothing
   to hand back. Four small clones buy the promise that a refused commit never destroys an edit
   session. `Field::into_valid()` is now unused by the spike; kept on the public surface.

3. **`SubmitError` and `CommitError` are two enums, not one.** They differ by exactly
   `AlreadySubmitted`, which a *handle* can be and a *draft* cannot. A `From` impl bridges them. The
   FFI DTO already had all four variants flat, so this cost nothing at the boundary.

4. **`test:apple:ui` not run.** XCUITest needs Xcode plus a logged-in GUI session holding
   Accessibility permission (step-03 finding 7) — structurally impossible here. `test4` was rewritten
   from *"F6: the conflict must persist"* to *"C14: the banner must clear"*, and its assertions follow
   from behaviour the VM, probe, controller and Kotlin tiers all verify. **It is nevertheless
   unverified. Someone should run `mise run test:apple:ui` on a GUI session before step 07.**

5. **Milestones M1–M3 landed as one commit**, as did M4–M5: the conformance suite cannot compile until
   the core semantics exist, and the FFI/web changes are one type-level ripple.

## Friction log (input to steps 07–10)

1. **A decision that is only written down is untested.** D9 read plausibly in a table, survived four
   reviewers' attention in the question I asked, and was falsified by the first test written against
   it. The freeze conforming the reference implementation is what caught it. Had step 06 been
   docs-only — the option I recommended — `dirty` would have gone into ARCHITECTURE §6, and step 07 or
   08 would have implemented a caret bug from the spec.

2. **The FFI wrapper and the core store had the same bug, independently.** Both pre-checked the commit
   gates and then called `commit`, which re-checked them; both therefore had an unreachable
   `commit`-failed branch. D4 deleted the branch in both places by making `commit` the single owner of
   its own gates. *Generator note (step 10): `submit` should be emitted as take → ask → put back, never
   as check → take → ask.*

3. **The `Option`-returning borrow is where a tombstone meets a Rust shell**, and it lands softly:
   four `?` sites and two helpers. But `ProfileController::draft()` now returns `Option<Ref<..>>`, and
   the test suite needed one `expect`-ing helper to stay readable. A generated Rust shell should emit
   that helper.

4. **C16 changes every shell's submit path**, exactly as predicted. Three test suites needed a
   `pass_check()` fixture before they could submit an edited username; the Kotlin `ErrorProbe`'s
   tier-2 assertion broke because `ruleErrors.single()` became two violations. This is the contract
   working — but *step 07's Compose shell must surface `username_check_required` as "checking…", not as
   an error*, or the first submit inside the debounce window will look like a failure to the user.

5. **D3 was invisible at the FFI boundary, and that is worth stating precisely.** Repacking Apple
   changed exactly one thing in 1758 lines of generated Swift: a Rust doc comment. Dropping
   `Conflicted.base` from the core while the DTO keeps projecting `{base, theirs}` is the shape of
   every future core refactor that must not break generated bindings.

6. **`ProfileDraft::commit` still has one unreachable arm** (an `is_ok()` report implies all four
   fields are `Valid`). It is honest, it is in the "as-if-generated" layer, and a macro will emit it
   verbatim. Worth a `bolted-check` lint rather than a comment.

7. **Two spike-only helpers went dead, and one of them was still wired up.** `Field::into_valid` is
   unused (deviation 2). The `field_conflicted` l10n key is gone with the synthetic rule violations —
   but verifying the report caught that only the *Rust* shell had dropped it, while
   `Localization.swift` still carried it **and had no template for the new `username_check_required`
   key**, so the Swift app would have rendered a raw identifier to a user on the most common refusal
   path C16 introduces. Both shells fixed. *A shell's l10n table is a place where a core error key
   silently goes missing; `bolted-check` should verify key coverage per target.*

8. **`Value::Error: Into<ErrorData>` paid for itself immediately.** `Field::invalid_error()` now exists
   once in the core and deleted a three-line match plus a restated `where` clause from two crates. The
   two independent votes (step-01 Q2, step-04 friction 3) were right.

## What steps 07–10 inherit

- **Step 07** owns **stash/restore** (undesigned), re-measures the keystroke round-trip on **physical
  hardware** (step 05's 12–13 µs is an emulator lower bound), scopes the draft handle to a `ViewModel`
  now that `close()` is mandatory, and must render `username_check_required` as progress, not failure.
- **Step 08** makes the conformance suite generic; decides the store concurrency model under step-02's
  three constraints; decides whether the store holds drafts **weakly**.
- **Step 09**: `#[bolted::value]` must **never emit `Copy`** (D8).
- **Step 10** carries the largest inherited list — see ROADMAP. The headline: **use-after-close is
  silent UB today** and must become a typed error. Also: **report the `boltffi pack android` bug
  upstream** and delete the workaround in `mise run pack:android`.

## Exit checklist

- [x] `mise run check` green; `bolted-core` zero-dependency and `#![forbid(unsafe_code)]`.
- [x] `mise run test:apple`, `mise run test:android`, `mise run test:web` green.
      (`test:apple:ui` is GUI-gated and **not run** — deviation 4.)
- [x] `docs/CONFORMANCE.md` exists; every C-ID has a test; the drift test enforces it **and was
      proven to fail in both directions**.
- [x] ARCHITECTURE.md says **frozen**; every §9 entry is either gone (decided, in §8 with its losing
      alternative) or carries an owning step. §9 went from **14** entries to **8**, and none of the 8
      blocks Phase 3.
- [x] No `unwrap`/`expect`/`panic!` in library code; no constraint literal in shell code.
- [x] Generated Swift diff after M4: one doc comment in 1758 lines. D3 is invisible at the boundary.
- [x] This report written.
- [x] ROADMAP updated (06 → done, 07 → ready; steps 08–10 grew the constraints the freeze hands them).
