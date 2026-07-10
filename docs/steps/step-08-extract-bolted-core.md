# Step 08 — Extract `bolted-core` + the conformance suite

**Phase 3 — Framework extraction.** Status: **ready**.

Read first: [ARCHITECTURE.md](../ARCHITECTURE.md) §5 (crate layout, trait sketches), §8 (D1–D15), §9
(the two store questions this step owns), and [CONFORMANCE.md](../CONFORMANCE.md) C01–C21.

> **Process note.** ARCHITECTURE §9 says its OPEN questions must not be resolved ad hoc, and
> `CLAUDE.md` splits planning (Fable) from implementation (Opus). At the owner's instruction this
> step was planned and implemented in one session, as step 06 and step 07 were. The three
> structural decisions below were put to the owner **before any code was written**, each with the
> alternative it beat; the owner chose all three. Everything else is ordinary implementation
> latitude. The bending of the rule is recorded here rather than in the report, because a rule
> quietly bent twice is a rule that has been repealed.

## Why this step exists

Phase 1 wrote the store loop **three times**:

| Where | Shape | Registered by |
|---|---|---|
| `bolted_core::Store` | `Rc<RefCell<Option<D>>>` + `Vec<Weak<…>>` | `Rc` clones |
| `spike-profile-ffi::StoreCore` | `Arc<Mutex<…>>` + `HashMap<u64, DraftEntry>` | ids |
| step 07's `restore` | *both of the above, again* | — |

Nothing enforces that they agree, and they already do not: `ProfileStoreFfi::live_draft_count()`
returns **1** for a create-flow draft where `bolted_core::Store::live_draft_count()` returns **0**.
Step 07 proved this with `testLiveDraftCountDisagreesWithTheCoreOnACreateFlowDraft` — a test written
to *document a divergence*, because at the time nothing could fix it. This step fixes it by deleting
two of the three loops.

The conformance suite has the mirror-image problem. It is 30 tests over 21 normative IDs, and every
one of them names `spike_profile::ProfileDraft`. CONFORMANCE.md's own roadmap says step 08 makes it
"generic over a feature", adding: *"Doing it now would mean inventing a fixture trait with exactly
one implementor."* That is still true — so this step also builds the second implementor.

---

## Decisions (each with the alternative it beat)

### D16 — the store is id-keyed and lock-free; the core ships no lock

`Store<D>` owns its drafts in a `BTreeMap<DraftId, Entry<D>>`. A handle is a plain `DraftId` — `Copy`,
never reused. There is no `Rc`, no `RefCell`, no `Weak`, and no `Mutex` **anywhere in `bolted-core`**.
`Store<D>` is therefore `Send` by construction whenever `D` and `D::Entity` are, which they are.

The lock is the *shell's* choice, not the framework's:

```rust
// Rust shell (web, Linux):   no lock at all — &mut self suffices
store: Store<ProfileDraft>

// FFI shell (Swift/Kotlin):  one lock, exactly as step 02 demanded
Mutex<FfiState { store: Store<ProfileDraft>, producers: BTreeMap<DraftId, Arc<StreamProducer<..>>> }>
```

Mutations return their fan-out **as data**, so a shell can emit outside its own lock without the core
knowing that locks or streams exist:

```rust
fn apply_canonical(&mut self, entity: D::Entity) -> Vec<DraftId>;  // the drafts it rebased
fn delete_canonical(&mut self)                  -> Vec<DraftId>;  // the drafts it orphaned
fn submit(&mut self, id: DraftId) -> Result<Vec<DraftId>, SubmitError<D::FieldId>>;
```

This is the sans-io principle (§5) applied to the store: effects as data, driven by the platform
layer. It satisfies all three of step 02's non-negotiable constraints — `Send` state behind one lock,
id-keyed handles rather than `Rc` clones, and never emit or call out under the lock — and it does so
by making the first two structural and the third *expressible*.

**Rejected: an `Rc<RefCell<Store<D>>>` RAII wrapper (`LocalStore` / `LocalHandle`) so Rust shells keep
`close()`-on-`Drop`.** This was tried first, because it preserves C18's "dropping the handle must do
the same" clause verbatim. It cannot work. `LocalHandle::drop` must take the `RefCell` to reach the
store, and ordinary safe user code reaches it while the store is already borrowed:

```rust
let _guard = store.borrow_mut();
drop(cancelled_handle);      // => thread 'main' panicked: RefCell already borrowed
```

Verified in a scratch crate before this decision was made. `try_borrow_mut()` instead of `borrow_mut()`
converts the panic into a silently leaked draft. Either way it is a framework mechanic that can only
fail at runtime, which is precisely what VISION's verification ladder forbids at rung 4. A convenience
that costs a rung is not a convenience.

**Rejected: keep both loops** (parameterise nothing, ship two stores). This is the status quo, whose
price step 07 measured: `restore` had to be written twice, and the `liveDraftCount` divergence went
unnoticed for five steps.

**Consequence, stated plainly: `close(id)` becomes mandatory on every platform, Rust included.** A
Rust shell that abandons a draft without closing it leaks exactly the rebasing zombie Kotlin leaks.
This is a regression in Rust ergonomics and an *improvement* in contract honesty: the reference
implementation stops being forgiving in the one way the GC platforms are not, so a lifecycle bug
written against it can no longer surface for the first time on Android. C18 is amended accordingly.

**This also dissolves §9's second store question rather than answering it.** "Should the store hold
drafts weakly?" presupposes that something *else* owns the draft and may drop it. Under D16 the store
owns the draft outright and the id is not an owner, so there is nothing to hold weakly. Weakness is
unrepresentable, and the leak it was meant to reap is now closed by `close()` on every platform, with
`bolted-ffi`'s `Cleaner` backstop (§9, step 10) as the remaining safety net for GC languages.

### D17 — `Draft` gains the resolvers; `Stashable` is a new subtrait

Making the suite generic is what exposed this. `resolve_keep_mine` / `resolve_take_theirs` are called
by every shell across the FFI, yet they live as *inherent* methods on `ProfileDraft`, invisible to any
code generic over `Draft`. Same for `stash` / `from_stash`, which step 07 added.

```rust
pub trait Draft {
    // …
    fn resolve_keep_mine(&mut self, field: Self::FieldId);
    fn resolve_take_theirs(&mut self, field: Self::FieldId);
}

pub trait Stashable: Draft {
    type Stash: Clone + PartialEq + std::fmt::Debug;
    fn stash(&self) -> Self::Stash;
    fn from_stash(stash: &Self::Stash) -> Self where Self: Sized;
}

impl<D: StoreDraft + Stashable> Store<D> {
    pub fn restore(&mut self, stash: &D::Stash) -> DraftId { self.adopt(D::from_stash(stash)) }
}
```

`Stashable` is a *subtrait* and not part of `Draft` because a feature may legitimately have no stash
(nothing in the contract compels one), and because `from_stash` needs `Sized` while `Draft` does not.

**Rejected: leave the traits alone and let the conformance fixture supply resolve/stash as function
pointers.** That makes the fixture trait a mirror of `spike-profile`'s inherent API, which means the
"generic" suite is generic over a trait shaped exactly like the one feature it has — and step 10's
generated per-language contract tests would inherit that shape. D12 froze the `Draft` / `StoreDraft`
split on the principle that `Draft` is what shells call. Shells call the resolvers. They belong on
`Draft`.

This grows ARCHITECTURE §5's frozen trait sketch. That is a **scheduled** change, not drift: §9
explicitly handed the extraction to step 08.

---

## Deliverables

1. **`bolted-core::store` rewritten** per D16. `DraftId`, `BTreeMap`, effects-as-data,
   `close`/`is_live`/`draft`/`draft_mut`. `DraftHandle` is deleted. `Store::adopt` and
   `StoreDraft::is_based` (D15) survive unchanged. A `const _` assertion proves `Store<ProfileDraft>:
   Send` at rung 1.
2. **`Draft` + `Stashable`** per D17; `Store::restore`.
3. **`draft_count()` vs `rebasing_draft_count()`** — two names for the two questions the one
   `live_draft_count()` was conflating. Promoted to a normative statement (**C22**), pulled forward
   from step 10, because after D16 there is exactly one implementation to pin.
4. **`spike-profile-ffi` delegates.** `StoreCore`'s re-owned loop is deleted; the wrapper holds
   `Mutex<FfiState>` and keeps only what is genuinely FFI: stream producers, the checker, DTO
   projection, and the emit-outside-the-lock discipline. **The exported class surface does not
   change**, so no Swift or Kotlin source needs rewriting except where `liveDraftCount` was asserted.
5. **`profile-web` migrates to ids.** Expected to get *simpler* (`Option<&D>` instead of
   `Option<Ref<'_, D>>`).
6. **`bolted-conformance`** — a new crate. Two tiers:
   - **Value- and field-level** (C01–C05, C09, C14, and C19's field half): generic over `V: Value`
     alone, no feature needed. Run against **all four** of the spike's value types — strictly more
     coverage than the single-type tests they replace.
   - **Feature-level** (C06–C08, C10–C13, C15–C22): a `ConformanceFeature` fixture trait, with
     `RuleFeature` and `AsyncCheckFeature` subtraits for the invariants that presuppose a tier-2 rule
     or an async check.
   - `macro_rules!` suite stampers so a fixture **cannot silently skip an ID**. Names only, no logic —
     the same doctrine §5 sets for `bolted-macros`.
7. **`spike-note`** — a second, deliberately minimal fixture (two text fields, no rule, no async
   check). It exists to falsify "generic": without it, the fixture trait has one implementor and its
   genericity is an untested claim of exactly the kind C03's missing `prop_assume!` was.
8. **The drift test survives the move**: every `CNN` in CONFORMANCE.md still has a `cNN_*` and vice
   versa, now across the `bolted-conformance` crate.
9. **Amendments**: ARCHITECTURE §5 (traits, crate layout), §7 (C-list → C22), §8 (D16, D17), §9 (two
   store questions closed); CONFORMANCE C17/C18 reworded, C22 added.
10. **`docs/steps/step-08-report.md`** + ROADMAP status.

## Kill criteria (real — if hit, stop and report; do not work around)

1. **`Store<ProfileDraft>` is not `Send`.** The entire point of D16. *(Pre-verified before planning;
   a regression here means a `Value` impl acquired an `Rc`.)*
2. **The FFI cannot delegate without re-owning any part of the store loop.** If even one of
   checkout-registration / rebase fan-out / submit has to be rewritten wrapper-side, the "one store"
   thesis is false and D16 is wrong.
3. **"Never emit or call out under the lock" cannot be preserved** while delegating to a core store.
   Step 02 made this non-negotiable; a core that forces a violation is a core that cannot go behind
   FFI.
4. **The fixture trait needs a `spike-profile`-specific concept** — a named field, a named rule, a
   named check — to express an invariant. Then that invariant is not generic. Stop and report; do not
   widen the trait until it fits.
5. **An existing conformance test's *meaning* must change to keep it green.** The suite is the
   contract. A test that is edited to agree with new code has stopped being a test. (Reworded
   assertions against renamed APIs are fine; assertions about different *facts* are not.)

## Milestones

| # | What | Verified by |
|---|---|---|
| M1 | `bolted-core`: `DraftId` store, effects-as-data, `Draft` resolvers, `Stashable`, `C22` counters. `spike-profile` + `conformance.rs` follow. | `mise run check` |
| M2 | `profile-web` migrates to ids. | `mise run check`, `mise run test:web` |
| M3 | `spike-profile-ffi` delegates to `bolted_core::Store`; the third loop is deleted. | `mise run check` |
| M4 | Apple + Android suites; `liveDraftCount` divergence closed and its step-07 test inverted. | `test:apple`, `test:android`, `test:android:app` |
| M5 | `bolted-conformance`: value + field tier, over all four value types. | `mise run check` |
| M6 | Feature tier + `ConformanceFeature`; `spike-profile` implements it. | `mise run check` |
| M7 | `spike-note` implements it. Whatever breaks is the finding. | `mise run check` |
| M8 | Drift test; CONFORMANCE + ARCHITECTURE amendments. | `mise run check` |
| M9 | Report, ROADMAP, full sweep. | all six runnable verbs |

Each milestone is a commit, and each must leave `mise run check` green.

## Non-goals

- **`bolted-macros`** (step 09). Nothing here may require a proc macro. The `macro_rules!` suite
  stampers stamp names only.
- **`bolted-ffi`** as its own crate (step 10). `spike-profile-ffi` stays the only boltffi importer.
- **Use-after-close as a typed error**, and the `Cleaner` backstop (§9, step 10). D16 makes the leak
  uniform across platforms; it does not make misuse typed.
- **Stash schema evolution** (§9, step 10).
- **Publishing anything.** `publish = false` throughout.
- `checkout_frozen`, one-shot effects, C#.
- **The hardware chattiness benchmark** (step 07's kill criterion 4) stays unassessed until a device
  is attached. D16 touches the per-keystroke path (`draft_mut(id)` is a `BTreeMap` lookup where it was
  a `RefCell` borrow), so `bench:android:device` matters *more* after this step, not less.

## Exit checklist

- [ ] `mise run check` green; no `unwrap`/`expect`/`panic!` in library code.
- [ ] `Rc`, `RefCell`, `Weak`, `Mutex` appear **nowhere** in `bolted-core`.
- [ ] `spike-profile-ffi` contains no draft registry, no rebase fan-out, no submit loop.
- [ ] The conformance suite runs against **two** features, and a fixture cannot skip an ID.
- [ ] Every C-ID's test was made to **fail on purpose** at least once before being trusted.
- [ ] ARCHITECTURE §9 shrinks by exactly the two questions this step owns.
- [ ] Report written and its every number checked against the code.
