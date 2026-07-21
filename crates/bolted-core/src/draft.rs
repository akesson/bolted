//! The draft contract (ARCHITECTURE §5): a multi-field edit session, checkout → edit → validate
//! → commit. `#[bolted::entity]` generates the impl in the real framework.

use crate::report::{ErrorData, ValidationReport};
use crate::single_flight::{CheckState, CheckToken};

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

    /// Keep your value, accept theirs as the new ancestor: the field stays dirty and returns to
    /// `InSync` (C09). A no-op on a field that is not conflicted.
    fn resolve_keep_mine(&mut self, field: Self::FieldId);

    /// Adopt their value and their ancestor: the field lands clean and `InSync` (C09).
    fn resolve_take_theirs(&mut self, field: Self::FieldId);

    /// The parse-don't-validate moment: a draft goes in, an always-valid entity comes out, or the
    /// draft comes *back* with a typed reason. `Ok` ⇔ every field `Valid`, none `Conflicted`, no
    /// rule violations, status `Live`.
    ///
    /// The draft is returned on failure because a refused commit must never destroy an edit session
    /// (step-01 friction F3). It is what lets `Store::submit` put the draft back under its id
    /// without a pre-check pass that duplicates these very gates.
    fn commit(self) -> Result<Self::Entity, (Self, CommitError<Self::FieldId>)>
    where
        Self: Sized;
}

/// C07's three gates, in order: orphaned, then conflicted, then invalid. `None` means `commit` may
/// proceed to build the entity.
///
/// Every [`Draft::commit`] runs exactly these checks against exactly the trait's own accessors, so
/// they belong here — written once, at rung 1 — rather than in each implementor. Both spikes
/// hand-wrote them identically, and `#[bolted::entity]` calls this instead of emitting the same
/// three `if`s per feature: a macro that decided *when a commit is refused* would be putting the
/// design's most consequential judgement in its least verifiable code (ARCHITECTURE §5).
///
/// Order matters and is normative. An orphaned draft's conflicts are meaningless (the entity is
/// gone), and a conflicted field's validation report would name errors the user cannot act on until
/// the conflict is resolved.
pub fn commit_gates<D: Draft>(draft: &D) -> Option<CommitError<D::FieldId>> {
    if matches!(draft.status(), DraftStatus::Orphaned) {
        return Some(CommitError::Orphaned);
    }
    let conflicts = draft.conflicts();
    if !conflicts.is_empty() {
        return Some(CommitError::Conflicted { fields: conflicts });
    }
    let report = draft.validate();
    if !report.is_ok() {
        return Some(CommitError::Validation(report));
    }
    None
}

/// A draft that can flatten itself to serializable data and come back (ARCHITECTURE §4, C20/C21).
///
/// A **subtrait** rather than part of [`Draft`], for two reasons. `from_stash` needs `Sized`, which
/// `Draft` deliberately does not require. And nothing in the contract compels a feature to have a
/// stash: only a shell whose process can be killed mid-edit needs one, and on that platform it is
/// the shell that decides when to call it.
///
/// Both halves are shell-facing — Android calls `stash()` from `SavedStateHandle` and hands the
/// result back to [`crate::Store::restore`] — which is why they live here and not on
/// [`crate::StoreDraft`].
pub trait Stashable: Draft {
    /// Raw, serializable data: per field, the last input attempt and the ancestor it was made over.
    /// Never the sync state, and never an async verdict — see [`crate::FieldStash`] and C20.
    type Stash: Clone + PartialEq + std::fmt::Debug;

    fn stash(&self) -> Self::Stash;

    /// Rebuild a draft. **Not yet reconciled with canonical** — hand it to [`crate::Store::restore`],
    /// which rebases it onto whatever the server says now.
    fn from_stash(stash: &Self::Stash) -> Self
    where
        Self: Sized;
}

/// A draft carrying one or more asynchronous, single-flight validation checks (ARCHITECTURE §2,
/// tier 3's client-side half; C10, C13, C16).
///
/// A **subtrait** for the same two reasons as [`Stashable`]: a feature with no async check owes
/// nothing, and a generic consumer that needs one says so in a bound.
///
/// Checks are **id-keyed**, exactly as [`Draft`]'s resolvers are field-keyed (D17, D18). A concrete
/// `CheckId` enum is monomorphic, so it crosses the FFI boundary as `FieldId` already does — §5's
/// ban on generic methods at the boundary does not bite. Until step 09 this surface lived on no
/// trait at all: two shells, `fixture-profile-ffi` and the conformance fixture each re-derived the
/// same three methods, which is how you learn that a contract is missing a name.
///
/// The verdict type is deliberately concrete rather than an associated type. A check either passes
/// or produces a localisable reason, and a verdict that carried anything else could not be projected
/// into a [`ValidationReport`].
///
/// Sequencing lives in [`crate::SingleFlight`] (rung 1, written once): the newest [`begin_check`]
/// supersedes any in-flight check, and a completion carrying a superseded token is discarded. An
/// implementor delegates; it does not reimplement.
///
/// [`begin_check`]: Checked::begin_check
pub trait Checked: Draft {
    /// Which check. One variant per declared check — usually exactly one.
    type CheckId: Copy + Eq + std::fmt::Debug;

    /// Start a check, superseding any in-flight one. The shell then performs the actual I/O and
    /// hands the token back to [`Self::complete_check`]: the effect is data, and the core stays
    /// sans-io (D10).
    fn begin_check(&mut self, check: Self::CheckId) -> CheckToken;

    /// Settle the check `token` began. Returns `false` — discarding `verdict` — if the token was
    /// superseded or the check already settled (C10).
    fn complete_check(
        &mut self,
        check: Self::CheckId,
        token: CheckToken,
        verdict: Result<(), ErrorData>,
    ) -> bool;

    /// Read a check's sub-state, so a shell can render a spinner. `validate()` folds the same state
    /// into the report; this getter never changes what a commit decides.
    fn check_state(&self, check: Self::CheckId) -> &CheckState<Result<(), ErrorData>>;

    /// The field this check endorses. C13's "verdicts are value-bound" is a claim about *this*
    /// field: any change to its value must reset the check to unchecked. C16's "an unrun check
    /// blocks a dirty field" is a claim about it too.
    ///
    /// An associated function rather than a method: which field a check pins to is a property of the
    /// feature's declaration, not of any particular draft's state.
    fn check_pins(check: Self::CheckId) -> Self::FieldId;
}
