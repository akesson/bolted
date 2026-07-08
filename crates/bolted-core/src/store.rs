//! Prototype store: canonical state + live drafts, single-threaded.
//!
//! This is deliberately throwaway plumbing — the real concurrency model is a Phase-3 decision
//! (ARCHITECTURE §9). What matters here is that canonical changes are *pushed* into live drafts so
//! the field/draft rebase semantics get exercised end to end. Interior mutability (`Rc<RefCell>`)
//! models the shared reality: the core owns the draft, a shell holds a handle to the same draft,
//! and canonical changes mutate it underneath.

use crate::draft::{Draft, DraftStatus};
use crate::report::ValidationReport;
use std::cell::{Ref, RefCell, RefMut};
use std::rc::{Rc, Weak};

/// Store-facing capabilities a draft must provide so the [`Store`] can drive live rebase.
///
/// Deliberately SEPARATE from [`Draft`] (the design's public/FFI contract, kept exactly as
/// ARCHITECTURE §5 sketches it): these three methods are prototype plumbing. Open question for the
/// freeze — does live-rebase driving belong in the core contract, or a capability like this?
pub trait StoreDraft: Draft {
    /// Build a fresh draft: a checkout of `base` (existing entity), or a create-flow draft (`None`).
    fn from_canonical(base: Option<&Self::Entity>, base_version: u64) -> Self;
    /// Rebase every field onto `entity` (per-field adopt / converge / conflict).
    fn rebase(&mut self, entity: &Self::Entity);
    /// Mark the whole draft orphaned (its base entity was deleted).
    fn orphan(&mut self);
}

/// A handle to a live, core-side draft. The SOLE strong owner of the draft; the store keeps only a
/// [`Weak`], so dropping the handle frees the draft and lets the store prune its rebase registry.
/// Not `Clone` — single ownership is what lets `submit` move the draft out to commit it.
pub struct DraftHandle<D: Draft> {
    inner: Rc<RefCell<D>>,
}

impl<D: Draft> DraftHandle<D> {
    /// Shared access to the draft (snapshot, dirty/conflict queries).
    pub fn borrow(&self) -> Ref<'_, D> {
        self.inner.borrow()
    }

    /// Mutable access to the draft (setters, resolve, async-check drive).
    pub fn borrow_mut(&self) -> RefMut<'_, D> {
        self.inner.borrow_mut()
    }
}

/// Why a submit was refused. `Conflict` survives here for the outer core↔server loop; within one
/// device the UI has already surfaced conflicts, so it is never a surprise.
#[derive(Debug)]
pub enum SubmitError<FieldId> {
    Validation(ValidationReport<FieldId>),
    Conflicted { fields: Vec<FieldId> },
    Orphaned,
}

/// A refused submit: the caller gets its [`DraftHandle`] back alongside the reason, so a rejection
/// never destroys the edit session (step-01 friction F3 / ARCHITECTURE §8). Only a *successful*
/// submit consumes the handle — its draft has been committed and is gone.
pub struct SubmitFailure<D: StoreDraft> {
    pub handle: DraftHandle<D>,
    pub error: SubmitError<D::FieldId>,
}

// The handle carries no `Debug` (a live draft need not be printable) and is noise in a failure
// dump anyway; the `error` is what a test or log wants. Bound only on the field id, so this holds
// even when the draft type is not `Debug`.
impl<D: StoreDraft> std::fmt::Debug for SubmitFailure<D>
where
    D::FieldId: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmitFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// Prototype store over a single canonical entity and its live drafts.
pub struct Store<D: StoreDraft> {
    canonical: Option<D::Entity>,
    version: u64,
    live: Vec<Weak<RefCell<D>>>,
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

    /// Check out a draft. Existing-entity checkouts register for live rebase; create-flow drafts
    /// (no canonical) are NOT registered — they never rebase (invariant I12).
    pub fn checkout(&mut self) -> DraftHandle<D> {
        let draft = D::from_canonical(self.canonical.as_ref(), self.version);
        let rc = Rc::new(RefCell::new(draft));
        if self.canonical.is_some() {
            self.live.push(Rc::downgrade(&rc));
        }
        DraftHandle { inner: rc }
    }

    /// A new canonical version arrived: bump version, rebase every live draft, then adopt it.
    pub fn apply_canonical(&mut self, entity: D::Entity) {
        self.version += 1;
        for weak in &self.live {
            if let Some(rc) = weak.upgrade() {
                rc.borrow_mut().rebase(&entity);
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
                rc.borrow_mut().orphan();
            }
        }
        self.canonical = None;
        self.prune();
    }

    /// Submit a draft transactionally. Refuses on orphaned status, any conflict, or a failing
    /// validation report; on success the committed entity becomes the new canonical and every
    /// other live draft rebases onto it.
    pub fn submit(&mut self, handle: DraftHandle<D>) -> Result<(), SubmitFailure<D>> {
        // Pre-checks under a shared borrow; compute the refusal (if any) WITHOUT moving the handle,
        // so a rejected submit can hand the caller's edit session back (F3). The pre-checks are
        // identical to `commit`'s own gates, which is what makes `commit` infallible below.
        let refusal = {
            let d = handle.inner.borrow();
            match d.status() {
                DraftStatus::Orphaned => Some(SubmitError::Orphaned),
                DraftStatus::Live => {
                    let conflicts = d.conflicts();
                    if !conflicts.is_empty() {
                        Some(SubmitError::Conflicted { fields: conflicts })
                    } else {
                        let report = d.validate();
                        if report.is_ok() {
                            None
                        } else {
                            Some(SubmitError::Validation(report))
                        }
                    }
                }
            }
        };
        if let Some(error) = refusal {
            return Err(SubmitFailure { handle, error });
        }

        // The handle is the sole strong owner (store holds only Weak, handle is not Clone), so
        // `try_unwrap` succeeds here — moving the draft out to run the consuming `commit`.
        match Rc::try_unwrap(handle.inner) {
            Ok(cell) => {
                // The pre-checks above are identical to `commit`'s gates and nothing rebases in
                // between (single-threaded), so `commit` cannot fail. On the unreachable failure
                // `commit(self)` has already consumed the draft — there is no handle to return —
                // so nothing is applied and the store is left unchanged.
                if let Ok(entity) = cell.into_inner().commit() {
                    self.apply_canonical(entity);
                }
                Ok(())
            }
            // Unreachable under single ownership: hand the handle back rather than drop it.
            Err(rc) => {
                let error = SubmitError::Validation(rc.borrow().validate());
                Err(SubmitFailure {
                    handle: DraftHandle { inner: rc },
                    error,
                })
            }
        }
    }

    fn prune(&mut self) {
        self.live.retain(|w| w.strong_count() > 0);
    }
}
