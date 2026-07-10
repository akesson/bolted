//! The store: canonical state + the drafts checked out against it.
//!
//! **The core ships no lock** (ARCHITECTURE §8, D16). [`Store`] is a plain owned struct with no
//! interior mutability, so it is `Send` whenever its draft and entity are, and the *shell* chooses
//! the sharing discipline: a Rust shell holds it by value and needs none; the FFI wrapper holds it
//! behind the one `Mutex` step 02 said it must.
//!
//! That is what makes the third store loop unnecessary. Phase 1 wrote this logic three times — an
//! `Rc<RefCell>` version here, an `Arc<Mutex>` version in `spike-profile-ffi`, and step 07's
//! `restore` in both — and the copies had already drifted (see [`Store::draft_count`]).
//!
//! Two consequences follow from owning the drafts outright:
//!
//! - **A handle is a [`DraftId`]**, not an owning smart pointer. Ids are issued monotonically and
//!   never reused, so a stale id is permanently dead rather than dangerously recycled.
//! - **[`Store::close`] is the only release path, on every platform.** There is no owner to drop.
//!   Step 05 proved Kotlin already worked this way and that pretending otherwise was the lie; the
//!   reference implementation now stops being forgiving in the one way the GC platforms are not.
//!
//! Mutations return their fan-out **as data** (`Vec<DraftId>`) rather than calling out to a
//! subscriber. That is the sans-io principle applied to the store, and it is what lets a shell obey
//! "never emit or call out under the lock" without the core knowing that locks or streams exist.

use crate::draft::{CommitError, Draft, Stashable};
use crate::report::ValidationReport;
use std::collections::BTreeMap;

/// A draft's identity within one [`Store`]. `Copy`, monotonically issued, **never reused**.
///
/// The inner `u64` cannot be constructed from outside: a foreign shell may pass an id back
/// (BoltFFI marshals it as a `u64`) but cannot forge one that was never issued. Unknown ids are
/// simply not live, which is what makes a post-submit call a typed refusal instead of a hazard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DraftId(u64);

impl DraftId {
    /// The wire form, for shells that must marshal an id across a language boundary.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Store-facing capabilities a draft must provide so the [`Store`] can drive live rebase.
///
/// Deliberately SEPARATE from [`Draft`] (the design's public/FFI contract): these methods are
/// plumbing the store needs and no shell ever calls. Four shells later, the split has cost nothing
/// and keeps the FFI surface minimal — ARCHITECTURE §8 froze it as D12 (step-01 Q1).
pub trait StoreDraft: Draft {
    /// Build a fresh draft: a checkout of `base` (existing entity), or a create-flow draft (`None`).
    fn from_canonical(base: Option<&Self::Entity>, base_version: u64) -> Self;
    /// Rebase every field onto `entity` (per-field adopt / converge / conflict), and record that
    /// the draft is now based on store version `version` (C15).
    fn rebase(&mut self, entity: &Self::Entity, version: u64);
    /// Mark the whole draft orphaned (its base entity was deleted).
    fn orphan(&mut self);
    /// Was this draft checked out from an existing entity? A create-flow draft has no base entity,
    /// so it never rebases and never orphans (C12).
    ///
    /// [`Store::checkout`] used to read this off the store (`self.canonical.is_some()`), which
    /// [`Store::adopt`] cannot do: a draft restored from a stash carries its own answer. **Derived
    /// from the fields' bases, never stored** — two copies of one fact are two facts to keep
    /// consistent (ARCHITECTURE §8, D3/F7).
    fn is_based(&self) -> bool;
}

/// Why a submit was refused. `Conflicted` survives here for the outer core↔server loop; within one
/// device the UI has already surfaced conflicts, so it is never a surprise.
///
/// The first three arms are [`CommitError`]'s, verbatim. `AlreadySubmitted` is the one failure a
/// *handle* can have that a draft cannot: the id outlives the draft it named. Step 02 found the FFI
/// wrapper had to invent exactly this variant because the foreign handle outlives the core draft;
/// step 05 found Kotlin's handle outlives it even harder (the GC never frees it). The core API says
/// so too. A `close`d id refuses the same way: from the outside, "gone" is one fact.
#[derive(Debug, Clone, PartialEq)]
pub enum SubmitError<FieldId> {
    Validation(ValidationReport<FieldId>),
    Conflicted { fields: Vec<FieldId> },
    Orphaned,
    AlreadySubmitted,
}

impl<FieldId> From<CommitError<FieldId>> for SubmitError<FieldId> {
    fn from(e: CommitError<FieldId>) -> Self {
        match e {
            CommitError::Validation(r) => SubmitError::Validation(r),
            CommitError::Conflicted { fields } => SubmitError::Conflicted { fields },
            CommitError::Orphaned => SubmitError::Orphaned,
        }
    }
}

/// One checked-out draft and whether canonical changes move it.
///
/// `rebases` is set once, by [`Store::adopt`], from `is_based()` × the presence of canonical. It is
/// cleared when the draft orphans, because an orphan is based on no canonical at all (C11/C15).
struct Entry<D> {
    draft: D,
    rebases: bool,
}

/// Canonical state and the drafts checked out against it.
///
/// `BTreeMap` rather than `HashMap`: the fan-out order of a rebase is then deterministic, which
/// costs nothing at these sizes and makes the returned `Vec<DraftId>` reproducible for tests and
/// for a future replay log.
pub struct Store<D: StoreDraft> {
    canonical: Option<D::Entity>,
    version: u64,
    drafts: BTreeMap<DraftId, Entry<D>>,
    next_id: u64,
}

impl<D: StoreDraft> Store<D> {
    pub fn new(canonical: Option<D::Entity>) -> Self {
        Store {
            canonical,
            version: 0,
            drafts: BTreeMap::new(),
            next_id: 0,
        }
    }

    pub fn canonical(&self) -> Option<&D::Entity> {
        self.canonical.as_ref()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    // ---- the two draft counts (conformance C22) --------------------------------------------

    /// How many drafts exist: checked out, restored, not yet submitted or closed.
    ///
    /// **This is not [`Self::rebasing_draft_count`]**, and conflating the two is a real bug with a
    /// real history. Until step 08 the core exposed one `live_draft_count()` meaning *"would be
    /// rebased"* while the FFI wrapper exposed one of the same name meaning *"exists"*; they
    /// disagreed by 1 on every create-flow draft, and step 07 shipped a test to document it because
    /// nothing at the time could fix it. Two questions, two names (C22).
    pub fn draft_count(&self) -> usize {
        self.drafts.len()
    }

    /// How many drafts the next canonical change would rebase. A create-flow draft is not one of
    /// them (C12), and neither is an orphan (C11).
    pub fn rebasing_draft_count(&self) -> usize {
        self.drafts.values().filter(|e| e.rebases).count()
    }

    // ---- draft access ------------------------------------------------------------------------

    /// Is `id` still a draft? False once it has been submitted (C17) or closed (C18), and false for
    /// an id this store never issued.
    pub fn is_live(&self, id: DraftId) -> bool {
        self.drafts.contains_key(&id)
    }

    /// Shared access to a draft (snapshot, dirty/conflict queries). `None` once it is gone.
    pub fn draft(&self, id: DraftId) -> Option<&D> {
        self.drafts.get(&id).map(|e| &e.draft)
    }

    /// Mutable access to a draft (setters, resolve, async-check drive). `None` once it is gone.
    pub fn draft_mut(&mut self, id: DraftId) -> Option<&mut D> {
        self.drafts.get_mut(&id).map(|e| &mut e.draft)
    }

    // ---- lifecycle ---------------------------------------------------------------------------

    /// Check out a draft over the current canonical, or a create-flow draft if there is none.
    pub fn checkout(&mut self) -> DraftId {
        self.adopt(D::from_canonical(self.canonical.as_ref(), self.version))
    }

    /// Take ownership of an **externally built** draft — one restored from a stash after process
    /// death — and bring it up to date with the store.
    ///
    /// This is the store's only draft entry point; `checkout` is `adopt` of a freshly built draft,
    /// which works because rebasing a draft onto the canonical it was just built from is a no-op
    /// (C19's idempotence).
    ///
    /// | `draft.is_based()` | canonical | result |
    /// |---|---|---|
    /// | true | `Some` | rebase onto it, register for live rebase |
    /// | true | `None` | **orphan**: the entity was deleted while we were dead (C11) |
    /// | false | either | untouched, unregistered — create-flow never rebases (C12) |
    ///
    /// The rebase is what makes restore correct rather than merely convenient: a field whose
    /// canonical moved while the process was dead comes back **conflicted**, not silently dirty over
    /// a base it never saw (conformance C21).
    pub fn adopt(&mut self, mut draft: D) -> DraftId {
        let rebases = match (draft.is_based(), &self.canonical) {
            (true, Some(entity)) => {
                draft.rebase(entity, self.version);
                true
            }
            (true, None) => {
                draft.orphan();
                false
            }
            (false, _) => false,
        };
        let id = DraftId(self.next_id);
        self.next_id += 1;
        self.drafts.insert(id, Entry { draft, rebases });
        id
    }

    /// Release a draft. Idempotent, and closing an id the store never issued is a no-op (C18).
    ///
    /// **There is no other release path.** The id is not an owner, so nothing reaps a draft when a
    /// shell forgets it: the store keeps rebasing an edit session no one can see. Kotlin has always
    /// been this way (step 05, H1); since D16 every platform is, and the contract reads the same
    /// everywhere.
    pub fn close(&mut self, id: DraftId) {
        self.drafts.remove(&id);
    }

    // ---- canonical changes (effects returned as data) ------------------------------------------

    /// A new canonical version arrived: bump the version, rebase every registered draft onto it,
    /// adopt it. Returns the ids it moved, in id order, so a shell can emit one snapshot per
    /// affected draft **after** dropping whatever lock it holds.
    pub fn apply_canonical(&mut self, entity: D::Entity) -> Vec<DraftId> {
        self.version += 1;
        let version = self.version;
        let mut rebased = Vec::new();
        for (id, entry) in self.drafts.iter_mut() {
            if entry.rebases {
                entry.draft.rebase(&entity, version);
                rebased.push(*id);
            }
        }
        self.canonical = Some(entity);
        rebased
    }

    /// Canonical was deleted: orphan every registered draft. Returns the ids it orphaned.
    ///
    /// An orphan stops rebasing, so a later `apply_canonical` that recreates the entity does not
    /// resurrect it — orphan is terminal, and its `base_version` stops moving (C11, C15).
    pub fn delete_canonical(&mut self) -> Vec<DraftId> {
        self.version += 1;
        let mut orphaned = Vec::new();
        for (id, entry) in self.drafts.iter_mut() {
            if entry.rebases {
                entry.draft.orphan();
                entry.rebases = false;
                orphaned.push(*id);
            }
        }
        self.canonical = None;
        orphaned
    }

    /// Submit a draft transactionally. On success the committed entity becomes the new canonical,
    /// every *other* registered draft rebases onto it (their ids are returned), and `id` is released
    /// — a second submit is `AlreadySubmitted` (C17). On refusal the draft goes straight back under
    /// the same id: the user's edit session survives (F3).
    ///
    /// There are no pre-checks here. `commit` owns the gates and reports them typed, so the store
    /// simply asks and reacts — which is why this function has no unreachable branch to apologise
    /// for (step-03 friction 1). Taking the draft *out* of the map is also what lets `commit`
    /// consume it by value while the store keeps its id.
    pub fn submit(&mut self, id: DraftId) -> Result<Vec<DraftId>, SubmitError<D::FieldId>> {
        let Some(entry) = self.drafts.remove(&id) else {
            return Err(SubmitError::AlreadySubmitted);
        };
        match entry.draft.commit() {
            Ok(entity) => Ok(self.apply_canonical(entity)),
            Err((draft, error)) => {
                self.drafts.insert(
                    id,
                    Entry {
                        draft,
                        rebases: entry.rebases,
                    },
                );
                Err(error.into())
            }
        }
    }
}

impl<D: StoreDraft + Stashable> Store<D> {
    /// Restore a draft the shell stashed before its process was killed, and rebase it onto whatever
    /// canonical says *now* (C21). Exactly `adopt(D::from_stash(..))`, named for the shell.
    pub fn restore(&mut self, stash: &D::Stash) -> DraftId {
        self.adopt(D::from_stash(stash))
    }
}
