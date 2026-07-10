//! The draft contract (ARCHITECTURE §5): a multi-field edit session, checkout → edit → validate
//! → commit. `#[bolted::entity]` generates the impl in the real framework.

use crate::report::ValidationReport;

/// Whole-draft lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftStatus {
    /// The base entity still exists; the draft tracks it (live rebase).
    Live,
    /// The base entity was deleted under the draft; submit becomes a typed outcome the app decides.
    Orphaned,
}

/// Why a commit was refused.
///
/// The three arms are the three gates of C07, each with its own shape — a validation report, the
/// conflicted field ids, or nothing at all. Before the freeze, `commit` returned only a
/// `ValidationReport` and re-encoded the other two as *synthetic rule violations*
/// (`unresolved_conflict`, `orphaned`) while the store's `submit` reported them as typed variants:
/// two divergent taxonomies for one set of failures (step-01 friction F5).
#[derive(Debug, Clone, PartialEq)]
pub enum CommitError<FieldId> {
    Validation(ValidationReport<FieldId>),
    Conflicted { fields: Vec<FieldId> },
    Orphaned,
}

/// The public draft contract. Kept as ARCHITECTURE §5 sketches it — the store-facing plumbing
/// (construction / rebase / orphan) lives on [`crate::store::StoreDraft`] instead.
pub trait Draft {
    type Entity;
    type FieldId: Copy + Eq + std::fmt::Debug;

    fn status(&self) -> DraftStatus;

    /// The store version this draft is based on: bumped by `checkout`, and by every rebase that
    /// moves it onto a newer canonical (conformance C15). A draft snapshot carries this so a stream
    /// consumer can drop a rebase snapshot it has already seen — the reconcile pattern step 02
    /// shipped for the future-only subscribe race.
    fn base_version(&self) -> u64;

    fn dirty_fields(&self) -> Vec<Self::FieldId>;
    fn conflicts(&self) -> Vec<Self::FieldId>;
    /// Tiers 1 + 2, in full.
    fn validate(&self) -> ValidationReport<Self::FieldId>;

    /// The parse-don't-validate moment: a draft goes in, an always-valid entity comes out, or the
    /// draft comes *back* with a typed reason. `Ok` ⇔ every field `Valid`, none `Conflicted`, no
    /// rule violations, status `Live`.
    ///
    /// The draft is returned on failure because a refused commit must never destroy an edit session
    /// (step-01 friction F3). It is what lets `Store::submit` put the draft back into its handle
    /// without a pre-check pass that duplicates these very gates.
    fn commit(self) -> Result<Self::Entity, (Self, CommitError<Self::FieldId>)>
    where
        Self: Sized;
}
