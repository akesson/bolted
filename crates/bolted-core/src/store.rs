//! Prototype store: canonical state + live drafts, single-threaded.
//!
//! This is deliberately throwaway plumbing — the real concurrency model is a step-08 decision
//! (ARCHITECTURE §9). What matters here is that canonical changes are *pushed* into live drafts so
//! the field/draft rebase semantics get exercised end to end. Interior mutability models the shared
//! reality: the core owns the draft, a shell holds a handle to the same draft, and canonical changes
//! mutate it underneath.

use crate::draft::{CommitError, Draft};
use crate::report::ValidationReport;
use std::cell::{Ref, RefCell, RefMut};
use std::rc::{Rc, Weak};

/// Store-facing capabilities a draft must provide so the [`Store`] can drive live rebase.
///
/// Deliberately SEPARATE from [`Draft`] (the design's public/FFI contract): these three methods are
/// plumbing the store needs and no shell ever calls. Four shells later, the split has cost nothing
/// and keeps the FFI surface minimal — ARCHITECTURE §8 froze it (step-01 Q1).
pub trait StoreDraft: Draft {
    /// Build a fresh draft: a checkout of `base` (existing entity), or a create-flow draft (`None`).
    fn from_canonical(base: Option<&Self::Entity>, base_version: u64) -> Self;
    /// Rebase every field onto `entity` (per-field adopt / converge / conflict), and record that
    /// the draft is now based on store version `version` (C15).
    fn rebase(&mut self, entity: &Self::Entity, version: u64);
    /// Mark the whole draft orphaned (its base entity was deleted).
    fn orphan(&mut self);
}

/// Why a submit was refused. `Conflicted` survives here for the outer core↔server loop; within one
/// device the UI has already surfaced conflicts, so it is never a surprise.
///
/// The first three arms are [`CommitError`]'s, verbatim. `AlreadySubmitted` is the one failure a
/// *handle* can have that a draft cannot: the handle outlives the draft it pointed at. Step 02 found
/// the FFI wrapper had to invent exactly this variant because the foreign handle outlives the core
/// draft; step 05 found Kotlin's handle outlives it even harder (the GC never frees it). The core
/// API now says so too.
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

/// A handle to a core-side draft: the sole owner of the draft, and a lifecycle object in its own
/// right. The store keeps only a [`Weak`] to the same cell, so it can rebase the draft without
/// keeping it alive.
///
/// The draft slot empties in exactly three ways — a successful [`Store::submit`], [`Self::close`],
/// or dropping the handle. Afterwards the handle is an inert **tombstone**: `is_live()` is false,
/// every borrow yields `None`, and a second submit is [`SubmitError::AlreadySubmitted`]. This is not
/// an invention: it is what BoltFFI already forces on every foreign shell (step 02's post-submit
/// tombstone, step 05's `AutoCloseable`), made honest in the core.
///
/// Not `Clone` — single ownership is what lets `submit` move the draft out to commit it.
pub struct DraftHandle<D: Draft> {
    inner: Rc<RefCell<Option<D>>>,
}

impl<D: Draft> DraftHandle<D> {
    /// Is the draft still there? False after submit / `close` (ARCHITECTURE §4).
    pub fn is_live(&self) -> bool {
        self.inner.borrow().is_some()
    }

    /// Shared access to the draft (snapshot, dirty/conflict queries). `None` on a tombstone.
    pub fn borrow(&self) -> Option<Ref<'_, D>> {
        Ref::filter_map(self.inner.borrow(), |slot| slot.as_ref()).ok()
    }

    /// Mutable access to the draft (setters, resolve, async-check drive). `None` on a tombstone.
    pub fn borrow_mut(&self) -> Option<RefMut<'_, D>> {
        RefMut::filter_map(self.inner.borrow_mut(), |slot| slot.as_mut()).ok()
    }

    /// Discard the draft. Idempotent. Dropping the handle does the same thing — in Rust `close()`
    /// is a convenience, not a requirement. It exists so that the contract reads identically in a
    /// GC language, where step 05 proved it is the *only* way a draft is ever freed.
    pub fn close(&mut self) {
        *self.inner.borrow_mut() = None;
    }
}

/// Prototype store over a single canonical entity and its live drafts.
pub struct Store<D: StoreDraft> {
    canonical: Option<D::Entity>,
    version: u64,
    live: Vec<Weak<RefCell<Option<D>>>>,
}

impl<D: StoreDraft> Store<D> {
    pub fn new(canonical: Option<D::Entity>) -> Self {
        Store {
            canonical,
            version: 0,
            live: Vec::new(),
        }
    }

    pub fn canonical(&self) -> Option<&D::Entity> {
        self.canonical.as_ref()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// How many drafts the store would rebase on the next canonical change. Falls when a handle is
    /// closed, submitted or dropped (C18) — the same registry semantics the FFI wrapper exposes as
    /// `liveDraftCount()`.
    pub fn live_draft_count(&self) -> usize {
        self.live
            .iter()
            .filter(|w| w.upgrade().is_some_and(|rc| rc.borrow().is_some()))
            .count()
    }

    /// Check out a draft. Existing-entity checkouts register for live rebase; create-flow drafts
    /// (no canonical) are NOT registered — they never rebase (conformance C12).
    pub fn checkout(&mut self) -> DraftHandle<D> {
        let draft = D::from_canonical(self.canonical.as_ref(), self.version);
        let rc = Rc::new(RefCell::new(Some(draft)));
        if self.canonical.is_some() {
            self.live.push(Rc::downgrade(&rc));
        }
        DraftHandle { inner: rc }
    }

    /// A new canonical version arrived: bump version, rebase every live draft onto it, then adopt
    /// it. Tombstoned slots are skipped; their handles are pruned once dropped.
    pub fn apply_canonical(&mut self, entity: D::Entity) {
        self.version += 1;
        let version = self.version;
        for weak in &self.live {
            if let Some(rc) = weak.upgrade() {
                let mut slot = rc.borrow_mut();
                if let Some(draft) = slot.as_mut() {
                    draft.rebase(&entity, version);
                }
            }
        }
        self.canonical = Some(entity);
        self.prune();
    }

    /// Canonical was deleted: orphan every live draft.
    pub fn delete_canonical(&mut self) {
        self.version += 1;
        for weak in &self.live {
            if let Some(rc) = weak.upgrade() {
                let mut slot = rc.borrow_mut();
                if let Some(draft) = slot.as_mut() {
                    draft.orphan();
                }
            }
        }
        self.canonical = None;
        self.prune();
    }

    /// Submit a draft transactionally. On success the committed entity becomes the new canonical,
    /// every other live draft rebases onto it, and `handle` becomes a tombstone. On refusal the
    /// draft goes straight back into the handle: the user's edit session survives (C17, F3).
    ///
    /// There are no pre-checks here. `commit` owns the gates and now reports them typed, so the
    /// store simply asks and reacts — which is why this function has no unreachable branch to
    /// apologise for (step-03 friction 1).
    pub fn submit(&mut self, handle: &mut DraftHandle<D>) -> Result<(), SubmitError<D::FieldId>> {
        let Some(draft) = handle.inner.borrow_mut().take() else {
            return Err(SubmitError::AlreadySubmitted);
        };
        match draft.commit() {
            Ok(entity) => {
                self.apply_canonical(entity);
                Ok(())
            }
            Err((draft, error)) => {
                *handle.inner.borrow_mut() = Some(draft);
                Err(error.into())
            }
        }
    }

    fn prune(&mut self) {
        self.live.retain(|w| w.strong_count() > 0);
    }
}
