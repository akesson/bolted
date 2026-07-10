# Step 08 — Report: extract `bolted-core` + the conformance suite

**Status: done. No kill criterion hit.** Plan: [step-08-extract-bolted-core.md](step-08-extract-bolted-core.md).

## Headline

**Phase 1 wrote the store loop three times, and the copies had drifted.** D16 deletes two of them.
`bolted_core::Store` now owns its drafts in a `BTreeMap<DraftId, _>`, ships **no lock**, and returns
its fan-out as data (`Vec<DraftId>`). It is `Send` by construction, so the same code serves a Rust
shell holding it by value and the FFI wrapper holding it behind the one `Mutex` step 02 demanded.
`spike-profile-ffi` no longer contains a draft registry, a rebase fan-out, or a submit transaction.

That answers §9's store-concurrency question and **dissolves** its weak-drafts question. "Should the
store hold drafts weakly?" presupposed that something other than the store owned the draft and might
drop it. Under D16 the store owns it outright and a handle is a `Copy` id, so there is nothing to hold
weakly. The question was not answered; it stopped being expressible.

**The second fixture found a hole that twenty-one invariants had missed.** `spike-note` exists to
falsify the claim "generic". Mutating its `StoreDraft::is_based` to consult a *single* field passed the
entire suite — and so did the same mutation on `spike-profile`. Every draft the other tests build has
an ancestor in all of its fields or in none, so nothing could tell a per-field `is_based` from a
one-field one. A partially-ancestored draft is not hypothetical: the stash is untrusted input (§9), and
a constraint tightened between app versions leaves exactly that shape. A draft misjudged create-flow is
never rebased, never orphaned, and silently overwrites the server on submit. C12 gained a second
sentence and `c12_an_ancestor_in_any_field_means_the_draft_is_entity_backed`, which fails under that
mutation on both features. **Step 09 will *generate* `is_based`.**

**A frozen document was amended for the second step running.** That deserves saying plainly rather
than burying: ARCHITECTURE is now v1.2. The difference from drift is that §9 *scheduled* both of these
changes for step 08, and both were put to the owner with their rejected alternatives before any code
was written. A freeze that never changes under evidence is not a freeze, it is a wish.

## What was built

| | |
|---|---|
| `bolted-core::store` | `DraftId` (opaque, `Copy`, never reused), `Store<D>` owning `BTreeMap<DraftId, Entry<D>>`, `draft`/`draft_mut`/`is_live`/`close`, `checkout`/`adopt`/`restore`, effects-as-data on `apply_canonical`/`delete_canonical`/`submit`. `Rc`, `RefCell`, `Weak` and `Mutex` appear nowhere in the crate's code (only in doc comments explaining why). |
| `bolted-core::draft` | `Draft` gains `resolve_keep_mine`/`resolve_take_theirs`; new `Stashable: Draft` subtrait with `type Stash` (D17). |
| C22 | `draft_count()` vs `rebasing_draft_count()`. Pulled forward from step 10, because after D16 there is exactly one implementation to pin. |
| `spike-profile-ffi` | Delegates. Keeps stream producers, the foreign checker, DTO projection, and the emit-outside-the-lock discipline. 674 → 612 code lines *while gaining* `rebasingDraftCount()`. |
| `profile-web` | Migrated to ids. `ProfileController::draft()` returns `Option<&ProfileDraft>` where it returned `Option<Ref<'_, ProfileDraft>>` — the store's `RefCell` had been leaking into the shell's public API. |
| `bolted-conformance` | New crate. 31 generic functions over 22 C-IDs, in three tiers, plus `field_suite!`/`feature_suite!`/`rule_suite!`/`async_check_suite!` and a three-way drift check. |
| `spike-note` | New crate. Two text fields, no tier-2 rule, no async check, no composite value. The falsifier. |

### The suite's shape

- **Value (C01) and field (C02–C06, C09, C14, C19, C20)** are claims about `Value` and `Field<V>`.
  They need no feature — only a `ValueFixture` — and now run **once per value type**. The spike's four
  types each carry ten invariants; before, `Username` carried them alone.
- **Feature (C06–C08, C10–C22)** needs a `ConformanceFeature`, which names **roles** — a *primary* text
  field, a *secondary* one, a *checked* one — and never a field. `RuleFeature` carries C08;
  `AsyncCheckFeature` carries C10, C13, C16 and C20's verdict clause. A feature with neither still
  satisfies the other eighteen, and the trait bounds are what say so rather than prose.
- **A fixture cannot skip an ID.** `manifest.rs` checks three things: every documented `CNN` has a
  function, every function is a documented `CNN`, and **every function is stamped by exactly one
  `*_suite!` macro**. The third is new and exists because the extraction created a place to hide: a
  generic function no macro stamps compiles, type-checks, is documented, and never runs.

## Kill criteria

| # | Criterion | Outcome |
|---|---|---|
| 1 | `Store<ProfileDraft>` is not `Send` | **Cleared.** Verified *before* planning, in a scratch test, and pinned at rung 1 by a `const _` assertion in `spike-profile`. |
| 2 | The FFI cannot delegate without re-owning part of the store loop | **Cleared.** Not one line of registration, fan-out or submit remains wrapper-side. |
| 3 | "Never emit or call out under the lock" cannot be preserved | **Cleared, and strengthened.** Because `apply_canonical`/`submit` return ids instead of calling back, there is no longer a *way* to emit from inside a fan-out. Step 02's hardest-won discipline became a property of the signature. |
| 4 | The fixture trait needs a `spike-profile`-specific concept | **Cleared, narrowly.** No field, rule or check is named in the suite. Two things came close and are recorded below as friction: `RuleFeature::arrange_rule_flip` is a callback (the suite fixes the *shape*, the feature supplies the two fields), and `AsyncCheckFeature` had to declare `begin`/`complete`/`state` itself, because no `bolted-core` trait carries them. The latter is now a §9 question. |
| 5 | An existing conformance test's *meaning* changes to keep it green | **Not hit, but read this carefully.** C17 and C18 changed meaning. They changed because D16 changed the design, decided and owner-approved before a line of code; no test was edited to agree with code that had already been written. C18's old "dropping the handle must do the same" clause is *deleted*, not reinterpreted, and its replacement asserts the opposite (see below). The distinction matters and I am not going to pretend the row is untouched. |

## The one that will bite: `close()` is now mandatory in Rust

An id is not an owner. Nothing reaps a draft a shell forgets — on **every** platform now, not just the
GC ones. `c18_release_is_explicit_and_idempotent` ends by asserting the leak on purpose:

```rust
{ let _forgotten = store.checkout(); }
assert_eq!(store.draft_count(), 1, "an id is not an owner");
assert_eq!(store.rebasing_draft_count(), 1, "and the store goes on rebasing a draft nobody can reach");
```

This is a real ergonomic regression and an improvement in honesty. Before D16, the Rust reference
implementation reaped that zombie on `Drop`, so a lifecycle bug written against it could only surface
for the first time on Android — which is exactly the class of thing step 05 spent itself discovering.

The RAII alternative was not dismissed on taste. It was built and run: `LocalHandle::drop` must take
the `RefCell` to reach the store, and ordinary safe user code reaches it while the store is borrowed
(`let g = store.borrow_mut(); drop(handle);` → `RefCell already borrowed`). `try_borrow_mut()` converts
the panic into a silently leaked draft. Either way it is a framework mechanic that can only fail at
runtime, which VISION's ladder forbids at rung 4.

**How much does the regression cost in practice? Almost nothing, on the only evidence we have, and the
evidence is thin.** `the_controller_never_accumulates_drafts_across_submits` proves the web shell holds
exactly one draft across ten submit round-trips and never calls `close`. One Rust shell, one draft, no
cancel button. A shell that opened a draft per row would have to close them, exactly as Kotlin does.

## Deviations from the plan

1. **M1–M3 landed as one commit, not three.** `spike-profile-ffi` is a workspace member, so
   `mise run check` cannot be green with the core rewritten and the wrapper not. The plan's
   "each milestone is a commit and must leave `check` green" was unsatisfiable as written.
2. **`bolted-conformance` panics on purpose.** `CLAUDE.md` forbids `unwrap`/`expect`/`panic!` in
   library code. This library's product *is* a failing test process. The exception is stated in the
   crate doc and taken nowhere else.
3. **C22 was pulled forward from step 10**, which ROADMAP had assigned "pin `liveDraftCount`'s
   semantics to a C-ID". Once D16 left one implementation, there was one answer to pin, and leaving the
   two counts sharing a name for two more steps served nobody.
4. **C12 gained a clause and a test mid-step**, from the `is_based` mutation finding. This was not in
   the plan; the plan did not know the hole existed.
5. **`ConformanceFeature::PRIMARY_OTHER` was added mid-step.** With only three primary texts,
   `c20_sync_is_not_stashed_and_re_derives` could not distinguish a re-derived conflict from a restored
   one — both would name `PRIMARY_THEIRS`. The test would have passed for the wrong reason. A fourth
   text makes the falsifier real.
6. **`rebasing_draft_count()` is a new exported FFI method**, so the Swift and Kotlin surfaces did grow
   by one. Nothing else about them changed; BoltFFI regenerated both bindings with no hand edits.
7. **A `draft_count()` accessor was added to `ProfileController`** so that the "no `close()` needed"
   claim above could be a test rather than a sentence.

## Friction log (input to steps 09–10)

1. **`Draft` cannot expose typed field access, and the fixture must.** `dirty_fields()` and
   `conflicts()` are id-keyed and generic; `field.theirs()` is `Option<&V>` and heterogeneous across a
   draft's fields. No trait can carry it. So `ConformanceFeature` supplies `primary(&draft) ->
   &Field<Self::Primary>`. **Step 10's per-language contract tests will need generated typed
   accessors** for exactly this reason — the C-IDs cannot be emitted against `Draft` alone.
2. **The async check's surface lives on no trait.** `begin_username_check` / `complete_username_check` /
   `username_check_state` are inherent methods that every shell and every binding re-derives.
   `AsyncCheckFeature` had to declare its own versions to state C10, C13 and C16. Recorded as a new §9
   question for step 09: if a macro is to emit them, the contract should name them.
3. **The orphan rule shaped `ValueFixture` for the better.** `impl ValueFixture for Username` cannot be
   written in `spike-profile`'s test crate — both are foreign to it. So a fixture is a *marker type*
   naming its value (`struct UsernameFixture; type Value = Username;`), which is what
   `ConformanceFeature` already was. A generated fixture (step 09) will have the same shape.
4. **`dirty_fields()` order is field-declaration order**, and a generic test must not depend on it.
   `c20_a_draft_stashes_and_restores` asserts membership and length, not sequence. Worth an explicit
   note in `bolted-macros`: the emitted order is observable.
5. **`drop(draft_id)` earns `dropping_copy_types`.** The compiler says "calls to `std::mem::drop` with
   a value that implements `Copy` does nothing" — which is precisely the fact C18 now asserts. The lint
   is the proof; the test uses scope exit to say it quietly.
6. **proptest's programmatic runner** (`TestRunner::run`) is what lets a property live in a library
   function rather than a `proptest!` block. `failure_persistence` must be set to `None`: there is no
   source file beside `bolted-conformance` to write a regression seed into, because the failing input
   belongs to whichever feature stamped the suite.
7. **`BTreeMap` over `HashMap`**, so the fan-out order — and therefore the returned `Vec<DraftId>` — is
   deterministic. Costs nothing at these sizes; makes tests and a future replay log reproducible.

## Verification

Every new or changed claim was made to fail on purpose before it was trusted.

| Mutation | Tests that caught it |
|---|---|
| Revert D14 (`rebase`'s three-way early-out) | **16, workspace-wide** — `c19_*` on all **six** value types across both features, `c19_the_store_does_not_conflict_an_unmoved_field` on both, all four `c21` restore tests, `c08_rebase_reruns_tier2`, two `bolted-core` unit tests, and the web shell's echo-rule test |
| `rebasing_draft_count()` answers the other question | `c22_*`, `c21_restore_into_a_deleted_canonical_orphans_the_draft`, `c21_a_restored_create_flow_draft_is_never_moved` |
| `NoteDraft::rebase` forgets a field | `c19_the_store_does_not_conflict_an_unmoved_field` |
| `commit` forgets the orphan gate | `c07`, `c11`, `c21_restore_into_a_deleted_canonical_orphans_the_draft` |
| `is_based()` consults one field (**both** features) | `c12_an_ancestor_in_any_field_means_the_draft_is_entity_backed` — and, before it existed, **nothing at all** |

| Verb | Result |
|---|---|
| `mise run check` | **158 tests** (86 at step 07) |
| `mise run test:web` | 8/8 headless |
| `mise run test:apple` | 39 probe + 14 VM |
| `mise run test:android` | 44/44 on ART |
| `mise run test:android:app` | 35/35 headless |
| `mise run test:android:hazard` | 3/3 |
| `mise run bench:android:device` | **NOT RUN** — no device attached; the verb refused, as designed |
| `mise run test:apple:ui` | not run — still GUI-gated (step 06 deviation 4 stands) |

Of the 158: `spike-profile` stamps 62 conformance tests (4 value types × 10, plus 16 feature + 1 rule +
4 async + 1 fixture self-check), `spike-note` stamps 37, `bolted-conformance` runs 3 manifest checks.

**Kill criterion 4 of step 07 — the per-keystroke round-trip on physical silicon — is still
unassessed, and D16 makes it matter more, not less.** The hot path changed: `draft_mut(id)` is a
`BTreeMap` lookup where it was a `RefCell` borrow. The emulator figure (12–13 µs against a 1.0 ms bar)
has ~80× of headroom, so nothing here is plausibly at risk, but *plausibly* is not *measured*.

## Open questions

ARCHITECTURE §9 went **8 → 7**: two store questions closed (one answered, one dissolved), one opened.

- **New: where does the async check's surface live?** — *step 09.* Friction 2 above.
- **Unchanged and now more urgent: stash schema evolution** — *step 10 / `bolted-check`.* The
  `is_based` finding is a consequence of it. A partial stash is what a tightened constraint produces,
  and C12's new clause is the only thing standing between that and a silent server overwrite.
- **Unchanged: use-after-close must become a typed error** — *step 10.* Note what D16 hands it: a stale
  `DraftId` is simply not live. The remaining UB belongs to BoltFFI's raw-pointer *handles*, not to the
  draft registry. An id-indexed foreign handle would make `DraftClosed` a typed error for free.
