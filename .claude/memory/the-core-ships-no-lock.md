---
name: the-core-ships-no-lock
description: "bolted-core owns drafts, hands out Copy DraftIds, and ships no lock or interior mutability — the shell picks the sharing discipline, and close(id) is the only release path"
metadata:
  node_type: memory
  type: project
---

`bolted_core::Store<D>` owns its drafts in a `BTreeMap<DraftId, _>` and contains **no `Rc`, `RefCell`,
`Weak` or `Mutex`** (ARCHITECTURE §8, D16, step 08). It is therefore `Send` whenever `D` is, and the
*shell* chooses the sharing discipline: the web shell holds it by value and needs no lock at all;
`spike-profile-ffi` holds it behind the one `Mutex` step 02 demanded. Do not add interior mutability
to the core to make a shell convenient — that is what created three drifting copies of the store loop.

Mutations return their fan-out **as data** (`apply_canonical` / `delete_canonical` / `submit` →
`Vec<DraftId>`), never a callback. This is sans-io applied to the store, and it is what makes "never
emit or call out under the lock" a property of the signature rather than of a comment.

**Consequences that surprise people:**

- **A handle is a `DraftId`: `Copy`, monotonically issued, never reused. It is not an owner.**
  `close(id)` is the *only* release path, in Rust exactly as in Kotlin (C18). Forgetting it leaks an
  edit session the store goes on rebasing; `c18_release_is_explicit_and_idempotent` asserts that leak
  on purpose. `drop(id)` earns `dropping_copy_types` — the lint is the proof.
- An RAII `Rc<RefCell<Store>>` handle **cannot** be added back: its `Drop` must take the `RefCell`, and
  safe user code reaches it while the store is borrowed (`let g = store.borrow_mut(); drop(handle);`
  panics). `try_borrow_mut` leaks instead. Rung 4 either way, which VISION forbids.
- `draft_count()` and `rebasing_draft_count()` answer **different questions** (C22). A create-flow
  draft and an orphan exist but do not rebase. One count standing for both was a real bug that spanned
  five steps across the FFI boundary.
- A stale `DraftId` is simply not live — no aliasing hazard. The remaining use-after-close UB (§9,
  step 10) belongs to BoltFFI's raw-pointer handles, not to the draft registry.

Related: [[a-suite-with-one-implementor-is-shaped-like-it]] (how step 08's suite was extracted).
