# Step 06 — Design freeze

**Phase 2. Status: ready.** Closes Phase 1. Every §9 OPEN question is either decided (into §8, with
its losing alternative) or explicitly deferred with an owning step. The reference implementation is
brought into line with the decisions, and the 13 invariants are promoted into a named conformance
suite that later steps generate per-language contract tests from.

> **Process note.** ROADMAP calls step 06 *"a planning session, not an implementation session"*, and
> CLAUDE.md reserves §9 resolutions for a design session. This session was asked to plan **and**
> implement it. The four contested decisions below (scope, F2, handle lifecycle, focused-rebase)
> were put to the project owner before any code was written; the rest follow from evidence already
> in the reports. This document is the design session's output. Nothing here is resolved ad hoc.

## Why now

Phase 1 produced four independent probes of the same feature — pure Rust, Apple/ARC, Rust/wasm,
Android/ART — and their friction logs agree with each other more than they disagree. Three separate
wounds (step-01 F3/F5, step-03 friction 1, step-04 friction 1) turn out to be one wound. Two
independent votes (step-01 Q2, step-04 friction 3) ask for the same trait bound. F2 and F6 were each
confirmed on two shells, and F6 finally got a UX verdict from a running app. The design has been
falsified where it was going to be falsified; what remains is to write down what survived.

## The measurement that reframes the version stamp

Step 05 found the draft snapshot's `version` is frozen at checkout. Verified again here:
`ProfileDraft::base_version` is written once in `from_canonical` and never touched by `rebase`.
Therefore `ProfileViewModel.swift:295` —

```swift
if snap.version < snapshot.version { return }   // "drop a stale rebase snapshot"
```

— **can never fire on a draft stream.** The subscribe-race mitigation step 02 shipped and step 03
relied on has been dead code on drafts since the day it was written. It works on the *canonical*
stream, where the store stamps a live version. This is not a kill (a `snapshot()` read is always
current) but it means §4's observe contract has been describing something the code does not do.

## Decisions to land

Each decision names the evidence, the losing alternative, and the blast radius. Decisions marked
**doc-only** change ARCHITECTURE.md and nothing else.

| # | Decision | Evidence | Blast radius |
|---|----------|----------|--------------|
| D1 | `Value::Error: Into<ErrorData>` becomes a trait bound | step-01 Q2, step-04 friction 3 (two independent votes) | `bolted-core`, `spike-profile`, `profile-web` helpers |
| D2 | `try_set` landing on `theirs` auto-converges (clears the conflict) | step-01 F6, step-03 `test4`, step-04 UX verdict: *actively confusing*; C04 already makes the identical judgement when the rebase arrives second | `Field::try_set`; flips F6 tests on 3 shells |
| D3 | `SyncState::Conflicted { theirs }` — drop the duplicated `base` | step-01 F7 (always equal to `Field.base` while conflicted; verified) | `bolted-core`, FFI projections. **The FFI DTO keeps `{base, theirs}`** (projected from `Field::base()`), so Swift/Kotlin do not change |
| D4 | `Draft::commit(self) -> Result<Entity, (Self, CommitError)>`; typed `CommitError { Validation, Conflicted, Orphaned }`; no more synthetic rule violations | step-01 F5/Q4, step-03 friction 1 | `bolted-core`, `spike-profile`, FFI wrapper |
| D5 | The handle is a lifecycle object: `Store::submit(&mut self, &mut DraftHandle)`, tombstone on success, `is_live()`, `close()`. `SubmitError` gains `AlreadySubmitted` | step-01 F3, step-02 (the FFI *already* tombstones and *already* needed `AlreadySubmitted`), step-03 friction 1, step-04 friction 1, step-05 H1 | `bolted-core`, `profile-web` controller + tests |
| D6 | Unchecked async verdict + **dirty** pinned field ⇒ commit refused (`username_check_required`). Unchecked + clean ⇒ passes | step-01 F1/F2, step-03 (default path), step-04 (`f2_a_never_checked_username_submits_successfully`) | `spike-profile::validate`; flips F2 tests on 3 shells |
| D7 | `StoreDraft::rebase(&mut self, entity, version)`; `Draft::base_version()` joins the trait | step-05 structural finding + the dead guard above | `bolted-core`, `spike-profile`, FFI |
| D8 | Value objects must not be `Copy` | step-01 F4 (uniform generated `.clone()` vs `clippy::clone_on_copy`) | `DateRange` loses `Copy` |
| D9 | A focused **clean** field adopts a rebase live. §6's echo rule becomes *"the control owns its text while focused **and dirty**"* | step-03, step-04 (two views over one store visibly disagree) | `profile-web` controller, Swift VM; flips 2 tests |
| D10 | Capability callbacks are **synchronous**. Asynchrony is a shell-driven `begin`/`complete` effect pair | step-02 finding 7, step-04 §4 (`spawn_local` drove it), step-05 friction 7 | doc-only |
| D11 | `snapshots()` is an **FFI-boundary mechanism**, not a universal contract member. Rust shells read direct + tick | step-04 headline 3 | doc-only |
| D12 | Keep the `Draft` / `StoreDraft` split | step-01 Q1/D2; four shells, zero friction | doc-only |
| D13 | `Constraint::Required` stays in the same enum, prepended at the field layer | step-01 Q3/D3; shells consume one uniform list | doc-only |

### What D6 looks like

`validate()` gains one arm. It is *not* a new error channel — a required-but-unrun check is a
validation failure, so `commit` succeeds ⇔ `validate().is_ok()` (C07) stays true.

```rust
match self.username_check.state() {
    CheckState::Idle if self.username.is_dirty() => push("username_check_required"),
    CheckState::Pending { .. }                   => push("username_check_pending"),
    CheckState::Done { verdict: Err(e) }         => push(e),
    CheckState::Idle | CheckState::Done { verdict: Ok(()) } => {}
}
```

A clean field holds the canonical value, which was verified when it was committed — so `Idle` + clean
must pass, or a user who only edits their email can never submit. C13 guarantees a surviving
`Done(Ok)` was computed for the value now in the field. Together, D6 + C13 close F1 and F2 by
construction rather than by shell convention.

### What D5 looks like

```rust
store.submit(&mut handle)?;   // Ok: draft committed; handle is now a tombstone
handle.is_live();             // false
handle.close();               // idempotent; dropping the handle does the same

pub struct DraftHandle<D: Draft> { inner: Option<Rc<RefCell<D>>>, registered: bool }
impl<D: Draft> DraftHandle<D> {
    pub fn borrow(&self) -> Option<Ref<'_, D>>;
    pub fn borrow_mut(&self) -> Option<RefMut<'_, D>>;
}
```

`submit` no longer pre-checks and then re-derives the same gates inside `commit`: it calls `commit`,
and on the (now typed) failure puts the draft back into the handle — re-registering it for rebase iff
it was registered. Step-03's unreachable dead branch and step-04's scratch-`checkout()` both
disappear. The core API stops disagreeing with what BoltFFI forced the FFI wrapper to do in step 02.

## Non-goals

- **No new features.** Nothing enters the contract that four probes did not ask for.
- **No stash/restore, no one-shot effects/navigation, no process topology.** They stay OPEN with
  owning steps (07, its own session, its own spike).
- **No store concurrency decision.** §9 already licenses deferring it to step 08; step 02 and step 04
  supply the constraints, not the answer.
- **No `bolted-macros`, no `bolted-ffi`, no extraction.** Steps 09–10.
- **No BoltFFI changes.** The `pack android` bug and the codegen papercuts become step-10 requirements
  and an upstream report; the workaround stays where it is.
- **No new probe.** This step measures nothing.

## Milestones

Each ends green on `mise run check`. M6 additionally needs `test:apple` / `test:android` / `test:web`.

- **M1 — `bolted-core`.** D1, D2, D3, D4, D5, D7, and the `Copy` rule's core half. `Store::submit`
  rewritten; `SubmitFailure` deleted. Unit tests updated.
- **M2 — `spike-profile`.** D6 (the `check_required` arm), D4's `commit`, D7's `rebase(entity,
  version)`, D8 (`DateRange` drops `Copy`; the `from_canonical`/`rebase` clones become uniform).
- **M3 — the conformance suite.** `docs/CONFORMANCE.md` (normative statements C01–C18, stable IDs)
  + `crates/spike-profile/tests/conformance.rs` (renamed from `invariants.rs`, `c01_*`…`c18_*`) +
  a **manifest drift test** that parses `CONFORMANCE.md` and fails if an ID has no test or a test has
  no ID. That drift check is the suite's rung-3 claim; without it the doc rots.
- **M4 — `spike-profile-ffi`.** The four `SyncState` projections read `Field::base()`; `commit`'s new
  return type; `rebase(entity, version)`; `base_version()` via the trait. **`dist/` DTOs unchanged** —
  verify by diffing the generated Swift/Kotlin before and after.
- **M5 — `profile-web`.** D5's `Option`-returning borrows through the controller, `submit(&mut handle)`
  (scratch checkout deleted), D9 (focused clean field adopts live), D1/D3 helper simplifications.
  Host tests updated; the F2/F6/focus tests flip and are renamed to say what now happens.
- **M6 — the two native shells.** Swift VM + probe tests and the Kotlin probe: the F2 and F6
  expectations flip. `pack` + run all three tiers. Swift's dead version guard becomes live (D7).
- **M7 — freeze the documents.** ARCHITECTURE.md → **frozen**: §8 absorbs D1–D13 with losing
  alternatives, §7 grows C14–C18 and points at CONFORMANCE.md, §9 shrinks to the genuinely deferred
  with an owning step each, §2/§3/§4/§6 corrected where a decision changed them. ROADMAP: 06 → done,
  07 → ready, and step 08/10's entries grow the constraints this freeze hands them.

## Conformance suite (M3)

C01–C13 are today's I1–I13, unchanged in meaning. New:

| ID | Statement |
|----|-----------|
| C14 | `try_set` to a value equal to `theirs` on a conflicted field lands clean and `InSync` (D2) |
| C15 | After `apply_canonical`, a live draft's `base_version` equals the store's version (D7) |
| C16 | A dirty check-pinned field with an unrun check refuses commit; a clean one does not (D6) |
| C17 | A successful submit leaves the handle a tombstone: `!is_live()`, no draft access, a second submit is `AlreadySubmitted` (D5) |
| C18 | `close()` is idempotent, frees the draft, and the store prunes it from its rebase registry (D5) |

Each ID gets one test function named for it. The suite stays in `spike-profile` for this step;
**making it generic over a feature is step 08's job** — that is what "extract the conformance suite"
means, and doing it now would be inventing a fixture trait with one implementor.

## Kill criteria

Real. If one fires, stop and report; do not work around it.

1. **D4 cannot cross BoltFFI.** If `commit(self) -> Result<Entity, (Self, CommitError)>` — a
   self-consuming method returning `Self` in the error arm — cannot be expressed in the FFI wrapper
   (which calls `commit()` on an owned draft it moved out of its registry), D4 is wrong and the
   freeze must pick a different shape for F5/Q4.
2. **D5 makes the Rust shell materially worse.** The tombstone forces `Option` into every read. If
   `profile-web`'s controller comes out meaningfully uglier than the scratch-`checkout()` it replaces
   — the wound D5 exists to heal — then the lifecycle handle is the wrong answer for zero-FFI targets
   and the contract is asymmetric on purpose. Judge on the diff, not in advance.
3. **D6 cannot be stated without the core knowing which fields are check-pinned.** The spike
   hand-writes one check on one field. If expressing "dirty + unrun ⇒ refuse" generically requires
   `bolted-core` to model check-to-field pinning, that is a §5 trait change and belongs to a design
   session, not to this one's implementation half.
4. **D2 breaks C04's symmetry.** If auto-converge on `try_set` and convergent rebase disagree in any
   reachable state, one of the two is wrong and the conflict model needs re-derivation.

## Exit checklist

- [ ] `mise run check` green; `bolted-core` still zero-dependency and `#![forbid(unsafe_code)]`.
- [ ] `mise run test:apple`, `mise run test:android`, `mise run test:web` green.
- [ ] `docs/CONFORMANCE.md` exists; every C-ID has exactly one test; the drift test enforces it.
- [ ] ARCHITECTURE.md says **frozen**, and every §9 entry is either gone (decided, in §8 with its
      losing alternative) or carries an owning step.
- [ ] No `unwrap`/`expect`/`panic!` in library code; no constraint literal in shell code.
- [ ] The generated Swift/Kotlin bindings are byte-identical before and after M4 (D3 is invisible at
      the boundary) — or the diff is explained.
- [ ] `docs/steps/step-06-report.md` written: decisions, kill criteria, friction, what step 07 and 08
      inherit.
- [ ] ROADMAP updated (06 → done, 07 → ready).
