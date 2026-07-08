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

/// The public draft contract. Kept exactly as ARCHITECTURE §5 sketches it — the store-facing
/// plumbing (construction / rebase / orphan) lives on [`crate::store::StoreDraft`] instead.
pub trait Draft {
    type Entity;
    type FieldId: Copy + Eq + std::fmt::Debug;

    fn status(&self) -> DraftStatus;
    fn dirty_fields(&self) -> Vec<Self::FieldId>;
    fn conflicts(&self) -> Vec<Self::FieldId>;
    /// Tiers 1 + 2, in full.
    fn validate(&self) -> ValidationReport<Self::FieldId>;
    /// The parse-don't-validate moment: a draft goes in, an always-valid entity comes out, or a
    /// report. `Ok` ⇔ every field `Valid`, none `Conflicted`, no rule violations, status `Live`.
    fn commit(self) -> Result<Self::Entity, ValidationReport<Self::FieldId>>;
}
