//! The feature tier: C06–C08, C10–C22 — claims about a `Draft` living inside a `Store`.
//!
//! The fixture trait names **roles**, never fields: a *primary* and a *secondary* text field, and
//! (for [`AsyncCheckFeature`]) a *checked* one. That is the line kill criterion 4 of step 08 drew —
//! if an invariant could only be stated by naming `username`, it would not be an invariant of Bolted
//! but of `spike-profile`.

use bolted_core::{
    CheckState, CheckToken, CommitError, Draft, DraftId, DraftStatus, ErrorData, Field,
    SingleFlight, Stashable, Store, StoreDraft, SubmitError, ValidationReport, Value,
};

/// Shorthand for the field-id type of a fixture's draft.
pub type FieldIdOf<F> = <<F as ConformanceFeature>::Draft as Draft>::FieldId;

/// Everything the suite needs in order to drive a feature it has never seen.
///
/// Two editable text fields are required, in distinct roles: the **primary** is the one the suite
/// edits, and the **secondary** is the one it moves on the server in order to prove C19 (a rebase
/// must not conflict a field whose own canonical never moved). They may have different value types —
/// `spike-profile`'s are `PersonName` and `Email` — which is why there are two associated types and
/// not one.
pub trait ConformanceFeature {
    type Entity: Clone + PartialEq + std::fmt::Debug;
    type Draft: StoreDraft<Entity = Self::Entity> + Stashable;
    type Primary: Value<Raw = String>;
    type Secondary: Value<Raw = String>;

    /// The canonical entity every store in the suite starts from. Its primary field must read
    /// [`Self::PRIMARY_BASE`] and its secondary [`Self::SECONDARY_BASE`].
    fn entity() -> Self::Entity;

    /// `entity()`'s primary/secondary text, as raw. Asserted, not assumed.
    const PRIMARY_BASE: &'static str;
    const SECONDARY_BASE: &'static str;
    /// A valid primary text that is a different *value* from `PRIMARY_BASE` — the user's edit.
    const PRIMARY_MINE: &'static str;
    /// A valid primary text different from both — the server's edit.
    const PRIMARY_THEIRS: &'static str;
    /// A fourth valid primary text, distinct from all of the above. Needed to prove that a restored
    /// conflict names the *current* canonical and not the one the process died holding: with only
    /// three texts, a stashed `theirs` and a re-derived one are indistinguishable.
    const PRIMARY_OTHER: &'static str;
    /// A primary text that must fail `try_new`.
    const PRIMARY_INVALID: &'static str;
    /// A valid secondary text different from `SECONDARY_BASE`.
    const SECONDARY_THEIRS: &'static str;
    /// A secondary text that must fail `try_new`.
    const SECONDARY_INVALID: &'static str;

    fn with_primary(entity: &Self::Entity, raw: &str) -> Self::Entity;
    fn with_secondary(entity: &Self::Entity, raw: &str) -> Self::Entity;

    fn primary_id() -> FieldIdOf<Self>;
    fn secondary_id() -> FieldIdOf<Self>;

    fn primary(draft: &Self::Draft) -> &Field<Self::Primary>;
    fn secondary(draft: &Self::Draft) -> &Field<Self::Secondary>;

    fn set_primary(
        draft: &mut Self::Draft,
        raw: &str,
    ) -> Result<(), <Self::Primary as Value>::Error>;
    fn set_secondary(
        draft: &mut Self::Draft,
        raw: &str,
    ) -> Result<(), <Self::Secondary as Value>::Error>;

    /// Leave `draft` in a state where `commit()` succeeds: every field valid, and any async check
    /// C16 would demand already satisfied. Used for the create-flow paths, which start `Unset`.
    fn fill_valid(draft: &mut Self::Draft);

    /// Remove the **secondary** field's ancestor from `stash`, leaving the primary's in place.
    ///
    /// This is not a hypothetical. The stash is the framework's first untrusted input (§9), and an
    /// ancestor that no longer parses — because a constraint was tightened between app versions —
    /// arrives as exactly this: a draft with a base in some fields and not others. `is_based` must
    /// still call it entity-backed, or the store will neither rebase nor orphan it and a stale edit
    /// will silently overwrite the server.
    fn forget_secondary_ancestor(stash: &mut <Self::Draft as Stashable>::Stash);
}

/// A feature with at least one tier-2 rule. C08 is its invariant.
pub trait RuleFeature: ConformanceFeature {
    /// The rule's stable name, as it appears in [`bolted_core::RuleViolation::rule`].
    const RULE: &'static str;
    /// The field ids the rule pins its errors to.
    fn rule_pins() -> Vec<FieldIdOf<Self>>;

    /// Arrange `draft` — freshly checked out from [`ConformanceFeature::entity`] — so that `RULE`
    /// does **not** fire, and return an entity whose rebase makes it fire by moving a field the rule
    /// does not pin to.
    ///
    /// A callback and not a datum, because "a rule that a rebase can flip" is a relationship between
    /// two fields, and only the feature knows which two. What the suite fixes is the *shape*: after
    /// the rebase, the pinned field must be dirty, its own canonical must not have moved, and it
    /// must therefore not be conflicted (C19).
    fn arrange_rule_flip(draft: &mut Self::Draft) -> Self::Entity;
}

/// A feature with an async, single-flight check pinned to one field. C10, C13 and C16 are its
/// invariants.
///
/// Note what this trait has to declare that no `bolted-core` trait does: `begin`/`complete`/`state`
/// for the check. Every generated shell re-derives that surface today. Step 09/10 should promote it.
pub trait AsyncCheckFeature: ConformanceFeature {
    type Checked: Value<Raw = String>;

    /// `entity()`'s checked-field text, and two distinct valid alternatives.
    const CHECKED_BASE: &'static str;
    const CHECKED_MINE: &'static str;
    const CHECKED_THEIRS: &'static str;

    /// The rule name the unrun/pending/failed check reports under, and the error key C16 raises.
    const CHECK_RULE: &'static str;
    const CHECK_REQUIRED_KEY: &'static str;

    fn with_checked(entity: &Self::Entity, raw: &str) -> Self::Entity;
    fn checked_id() -> FieldIdOf<Self>;
    fn checked(draft: &Self::Draft) -> &Field<Self::Checked>;
    fn set_checked(
        draft: &mut Self::Draft,
        raw: &str,
    ) -> Result<(), <Self::Checked as Value>::Error>;

    fn begin_check(draft: &mut Self::Draft) -> CheckToken;
    fn complete_check(
        draft: &mut Self::Draft,
        token: CheckToken,
        verdict: Result<(), ErrorData>,
    ) -> bool;
    fn check_state(draft: &Self::Draft) -> &CheckState<Result<(), ErrorData>>;
}

// =================================================================================================
// helpers
// =================================================================================================

fn rule_present<Id>(report: &ValidationReport<Id>, rule: &str) -> bool {
    report.rule_errors.iter().any(|v| v.rule == rule)
}

fn text<V: Value<Raw = String>>(field: &Field<V>) -> Option<String> {
    field.value().cloned().map(Value::into_raw)
}

fn base_text<V: Value<Raw = String>>(field: &Field<V>) -> Option<String> {
    field.base().cloned().map(Value::into_raw)
}

/// Drive the async check to a pass, if the feature has one. The base suite cannot call this, which
/// is exactly why [`ConformanceFeature::fill_valid`] exists.
fn pass_check<F: AsyncCheckFeature>(draft: &mut F::Draft) {
    let token = F::begin_check(draft);
    assert!(
        F::complete_check(draft, token, Ok(())),
        "a fresh token's completion must land"
    );
}

fn checked_out<F: ConformanceFeature>() -> (Store<F::Draft>, DraftId) {
    let mut store = Store::new(Some(F::entity()));
    let id = store.checkout();
    (store, id)
}

// =================================================================================================
// C06, C07 — validity and the parse moment
// =================================================================================================

/// C06, feature half — a failed `try_set` blocks submit, and the previous valid value is never
/// silently committed in its place.
pub fn c06_no_stale_value_submit<F: ConformanceFeature>() {
    let (mut store, id) = checked_out::<F>();
    {
        let draft = store.draft_mut(id).expect("live");
        assert!(F::set_primary(draft, F::PRIMARY_INVALID).is_err());
    }
    match store.submit(id) {
        Err(SubmitError::Validation(report)) => assert!(
            report
                .field_errors
                .iter()
                .any(|(f, _)| *f == F::primary_id()),
            "the invalid field must appear in the report"
        ),
        other => panic!("expected a validation refusal, got {:?}", other.map(|_| ())),
    }
    assert_eq!(
        store.canonical(),
        Some(&F::entity()),
        "the stale valid value was NOT silently submitted"
    );
    assert!(store.is_live(id), "the refusal did not destroy the session");
}

/// C07 — commit succeeds **iff** every field is `Valid`, none is `Conflicted`, no rule is violated,
/// and the status is `Live`. Each refusal is typed and hands the draft back.
pub fn c07_commit_is_the_parse_moment<F: ConformanceFeature>() {
    let entity = F::entity();

    // an untouched checkout commits, and the entity it yields equals the one it came from
    let clean = F::Draft::from_canonical(Some(&entity), 0);
    match clean.commit() {
        Ok(committed) => assert_eq!(committed, entity),
        Err((_, e)) => panic!("a clean checkout must commit, got {e:?}"),
    }

    // a create-flow draft, filled, commits
    let mut created = F::Draft::from_canonical(None, 0);
    F::fill_valid(&mut created);
    assert!(
        created.commit().is_ok(),
        "fill_valid must make it committable"
    );

    // an invalid field -> Validation
    let mut invalid = F::Draft::from_canonical(Some(&entity), 0);
    let _ = F::set_primary(&mut invalid, F::PRIMARY_INVALID);
    assert!(matches!(
        invalid.commit(),
        Err((_, CommitError::Validation(_)))
    ));

    // an unset field -> Validation (create flow, unfilled)
    let empty = F::Draft::from_canonical(None, 0);
    assert!(matches!(
        empty.commit(),
        Err((_, CommitError::Validation(_)))
    ));

    // an unresolved conflict -> Conflicted, NOT a synthetic rule violation
    let mut conflicted = F::Draft::from_canonical(Some(&entity), 0);
    F::set_primary(&mut conflicted, F::PRIMARY_MINE).expect("valid");
    conflicted.rebase(&F::with_primary(&entity, F::PRIMARY_THEIRS), 1);
    assert_eq!(conflicted.conflicts(), vec![F::primary_id()]);
    match conflicted.commit() {
        Err((_, CommitError::Conflicted { fields })) => assert_eq!(fields, vec![F::primary_id()]),
        other => panic!("expected Conflicted, got {:?}", other.err().map(|(_, e)| e)),
    }

    // an orphaned draft -> Orphaned
    let mut orphaned = F::Draft::from_canonical(Some(&entity), 0);
    orphaned.orphan();
    assert!(matches!(orphaned.commit(), Err((_, CommitError::Orphaned))));
}

// =================================================================================================
// C08 — tier 2 (RuleFeature)
// =================================================================================================

/// C08 — validation is a pure function of current draft state, so a rebase that moves any field must
/// change the next `validate()` accordingly, including rules that pin to a field the rebase did not
/// touch.
pub fn c08_rebase_reruns_tier2<F: RuleFeature>() {
    let entity = F::entity();
    let mut draft = F::Draft::from_canonical(Some(&entity), 0);
    let moved = F::arrange_rule_flip(&mut draft);
    assert!(
        !rule_present(&draft.validate(), F::RULE),
        "arrange_rule_flip must leave the rule satisfied"
    );

    draft.rebase(&moved, 1);

    let report = draft.validate();
    let violation = report
        .rule_errors
        .iter()
        .find(|v| v.rule == F::RULE)
        .expect("the rebase must make the rule fire");
    assert_eq!(violation.pins, F::rule_pins());

    // ...and the pinned field, dirty but with an unmoved canonical, is NOT conflicted. Until step 07
    // it was, and this very test passed anyway, because it only asserted on the rule (C19).
    assert_eq!(draft.conflicts(), vec![]);
}

// =================================================================================================
// C10, C13, C16 — the async check (AsyncCheckFeature)
// =================================================================================================

/// C10 — a completion carrying a superseded token is discarded; at most one check is in flight.
pub fn c10_latest_check_wins<F: AsyncCheckFeature>() {
    // the mechanism, feature-free
    let mut sf: SingleFlight<i32> = SingleFlight::new();
    let first = sf.begin();
    let second = sf.begin();
    assert!(
        !sf.complete(first, 1),
        "a superseded token must be discarded"
    );
    assert!(sf.complete(second, 2));
    assert_eq!(sf.state(), &CheckState::Done { verdict: 2 });

    // ...and on a draft: a stale FAILING verdict must not resurrect after a fresh PASSING one
    let mut draft = F::Draft::from_canonical(None, 0);
    let first = F::begin_check(&mut draft);
    let second = F::begin_check(&mut draft);
    assert!(!F::complete_check(
        &mut draft,
        first,
        Err(ErrorData::new("stale"))
    ));
    assert!(F::complete_check(&mut draft, second, Ok(())));
    assert!(!rule_present(&draft.validate(), F::CHECK_RULE));
}

/// C13 — any change to a checked field's *value* resets its verdict to unchecked; a mutation that
/// leaves the value unchanged leaves the verdict standing.
pub fn c13_verdicts_are_value_bound<F: AsyncCheckFeature>() {
    let entity = F::entity();
    let passed = |d: &F::Draft| matches!(F::check_state(d), CheckState::Done { verdict: Ok(()) });

    // (a) passed, then edit to a DIFFERENT value -> reset
    let mut a = F::Draft::from_canonical(Some(&entity), 0);
    pass_check::<F>(&mut a);
    F::set_checked(&mut a, F::CHECKED_MINE).expect("valid");
    assert!(matches!(F::check_state(&a), CheckState::Idle));

    // (b) passed, then edit to the SAME value -> the verdict stands (value-based, like dirty)
    let mut b = F::Draft::from_canonical(Some(&entity), 0);
    pass_check::<F>(&mut b);
    F::set_checked(&mut b, F::CHECKED_BASE).expect("valid");
    assert!(passed(&b));

    // (c) clean field, rebase adopts a new canonical -> the value moves -> reset
    let mut c = F::Draft::from_canonical(Some(&entity), 0);
    pass_check::<F>(&mut c);
    c.rebase(&F::with_checked(&entity, F::CHECKED_THEIRS), 1);
    assert!(matches!(F::check_state(&c), CheckState::Idle));

    // (d) dirty field, rebase CONFLICTS (yours preserved) -> value unchanged -> verdict stands
    let mut d = F::Draft::from_canonical(Some(&entity), 0);
    F::set_checked(&mut d, F::CHECKED_MINE).expect("valid");
    pass_check::<F>(&mut d);
    d.rebase(&F::with_checked(&entity, F::CHECKED_THEIRS), 1);
    assert!(F::checked(&d).is_conflicted());
    assert_eq!(text(F::checked(&d)).as_deref(), Some(F::CHECKED_MINE));
    assert!(passed(&d));

    // (e) take-theirs moves the value -> reset; keep-mine does not
    let mut take = F::Draft::from_canonical(Some(&entity), 0);
    F::set_checked(&mut take, F::CHECKED_MINE).expect("valid");
    pass_check::<F>(&mut take);
    take.rebase(&F::with_checked(&entity, F::CHECKED_THEIRS), 1);
    take.resolve_take_theirs(F::checked_id());
    assert_eq!(text(F::checked(&take)).as_deref(), Some(F::CHECKED_THEIRS));
    assert!(matches!(F::check_state(&take), CheckState::Idle));

    let mut keep = F::Draft::from_canonical(Some(&entity), 0);
    F::set_checked(&mut keep, F::CHECKED_MINE).expect("valid");
    pass_check::<F>(&mut keep);
    keep.rebase(&F::with_checked(&entity, F::CHECKED_THEIRS), 1);
    keep.resolve_keep_mine(F::checked_id());
    assert_eq!(text(F::checked(&keep)).as_deref(), Some(F::CHECKED_MINE));
    assert!(passed(&keep));
}

/// C16 — an unrun check blocks commit only while its pinned field is dirty. A clean field still holds
/// the canonical value, which was verified when it was committed.
pub fn c16_an_unrun_check_blocks_a_dirty_field<F: AsyncCheckFeature>() {
    let entity = F::entity();

    // clean checked field, check never run, an unrelated edit -> commits
    let mut clean = F::Draft::from_canonical(Some(&entity), 0);
    F::set_primary(&mut clean, F::PRIMARY_MINE).expect("valid");
    assert!(matches!(F::check_state(&clean), CheckState::Idle));
    assert!(clean.validate().is_ok());
    assert!(clean.commit().is_ok());

    // dirty checked field, check never run -> refused, pinned to the checked field
    let mut dirty = F::Draft::from_canonical(Some(&entity), 0);
    F::set_checked(&mut dirty, F::CHECKED_MINE).expect("valid");
    let report = dirty.validate();
    let violation = report
        .rule_errors
        .iter()
        .find(|v| v.rule == F::CHECK_RULE)
        .expect("an unrun check on a dirty field must block");
    assert_eq!(violation.error.key, F::CHECK_REQUIRED_KEY);
    assert_eq!(violation.pins, vec![F::checked_id()]);
    assert!(matches!(
        dirty.commit(),
        Err((_, CommitError::Validation(_)))
    ));

    // ...and a passing check unblocks it
    let mut checked = F::Draft::from_canonical(Some(&entity), 0);
    F::set_checked(&mut checked, F::CHECKED_MINE).expect("valid");
    pass_check::<F>(&mut checked);
    assert!(checked.validate().is_ok());
    assert!(checked.commit().is_ok());

    // reverting to the canonical value makes it clean again, so no check is demanded (C05 + C16)
    let mut reverted = F::Draft::from_canonical(Some(&entity), 0);
    F::set_checked(&mut reverted, F::CHECKED_MINE).expect("valid");
    F::set_checked(&mut reverted, F::CHECKED_BASE).expect("valid");
    assert!(!F::checked(&reverted).is_dirty());
    assert!(reverted.validate().is_ok());
}

/// C20 — an async verdict does not survive the stash: it endorses a value against a server state that
/// may have moved. C13 + C16 then make the restored draft safe with no new invariant.
pub fn c20_an_async_verdict_does_not_survive_the_stash<F: AsyncCheckFeature>() {
    let entity = F::entity();
    let mut draft = F::Draft::from_canonical(Some(&entity), 0);
    F::set_checked(&mut draft, F::CHECKED_MINE).expect("valid");
    pass_check::<F>(&mut draft);
    assert!(draft.validate().is_ok());

    let restored = F::Draft::from_stash(&draft.stash());

    assert!(matches!(F::check_state(&restored), CheckState::Idle));
    assert!(F::checked(&restored).is_dirty());
    let report = restored.validate();
    let violation = report
        .rule_errors
        .iter()
        .find(|v| v.rule == F::CHECK_RULE)
        .expect("C16 must demand a fresh check for a restored dirty field");
    assert_eq!(violation.error.key, F::CHECK_REQUIRED_KEY);
}

// =================================================================================================
// C11, C12, C15, C17, C18, C19, C22 — the store
// =================================================================================================

/// C11 — deleting the canonical entity under a live draft orphans it, and submitting an orphaned
/// draft is a typed outcome.
pub fn c11_deletion_orphans<F: ConformanceFeature>() {
    let (mut store, id) = checked_out::<F>();
    assert_eq!(store.delete_canonical(), vec![id]);
    assert_eq!(
        store.draft(id).expect("live").status(),
        DraftStatus::Orphaned
    );
    assert_eq!(store.submit(id), Err(SubmitError::Orphaned));
    assert!(store.canonical().is_none());
    assert!(store.is_live(id), "the refusal handed the draft back");
}

/// C12 — a draft with no base entity is not moved by any canonical change, and commits normally.
pub fn c12_create_flow_never_rebases<F: ConformanceFeature>() {
    let mut store: Store<F::Draft> = Store::new(None);
    let id = store.checkout();

    assert_eq!(
        store.apply_canonical(F::entity()),
        vec![],
        "a create-flow draft is not in the fan-out"
    );
    {
        let draft = store.draft(id).expect("live");
        assert!(F::primary(draft).value().is_none());
        assert!(F::secondary(draft).value().is_none());
        assert!(draft.dirty_fields().is_empty());
    }

    F::fill_valid(store.draft_mut(id).expect("live"));
    store.submit(id).expect("a create-flow draft commits");
}

/// C12 — the contrapositive, and the one `is_based` actually has to get right: a draft that retains
/// an ancestor in **any** field is entity-backed, however few it has left.
///
/// Without this test, a `StoreDraft::is_based` that consults a single field passes the entire suite:
/// every draft the other tests build has an ancestor in all fields or in none. Found in step 08 by
/// mutating the second fixture, which is what the second fixture was for.
pub fn c12_an_ancestor_in_any_field_means_the_draft_is_entity_backed<F: ConformanceFeature>() {
    let entity = F::entity();
    let mut draft = F::Draft::from_canonical(Some(&entity), 0);
    F::set_primary(&mut draft, F::PRIMARY_MINE).expect("valid");

    let mut stash = draft.stash();
    F::forget_secondary_ancestor(&mut stash);
    let restored = F::Draft::from_stash(&stash);
    assert!(
        restored.is_based(),
        "one surviving ancestor still means entity-backed"
    );

    // so the store registers it for live rebase...
    let mut store: Store<F::Draft> = Store::new(Some(entity));
    let id = store.restore(&stash);
    assert_eq!(store.rebasing_draft_count(), 1);
    assert_eq!(store.draft(id).expect("live").status(), DraftStatus::Live);

    // ...and orphans it when the entity is gone (C11), rather than letting it commit as a new one
    let mut deleted: Store<F::Draft> = Store::new(None);
    let orphan = deleted.restore(&stash);
    assert_eq!(
        deleted.draft(orphan).expect("live").status(),
        DraftStatus::Orphaned,
        "a partially-stashed draft must not become a create-flow draft"
    );
    assert_eq!(deleted.submit(orphan), Err(SubmitError::Orphaned));
}

/// C15 — after a canonical change rebases a draft, its `base_version` equals the store's. An orphan
/// is based on no canonical, and its stamp stops moving.
pub fn c15_the_base_version_tracks_the_rebase<F: ConformanceFeature>() {
    let (mut store, id) = checked_out::<F>();
    assert_eq!(store.version(), 0);
    assert_eq!(store.draft(id).expect("live").base_version(), 0);

    store.apply_canonical(F::with_primary(&F::entity(), F::PRIMARY_THEIRS));
    assert_eq!(store.version(), 1);
    assert_eq!(store.draft(id).expect("live").base_version(), 1);

    store.apply_canonical(F::with_secondary(&F::entity(), F::SECONDARY_THEIRS));
    assert_eq!(store.version(), 2);
    assert_eq!(store.draft(id).expect("live").base_version(), 2);

    store.delete_canonical();
    assert_eq!(store.version(), 3);
    assert_eq!(store.draft(id).expect("live").base_version(), 2);
}

/// C17 — a successful submit releases the draft; a refused one leaves it live and intact.
pub fn c17_submit_releases_the_draft<F: ConformanceFeature>() {
    let (mut store, id) = checked_out::<F>();
    assert!(store.is_live(id));
    assert_eq!(store.draft_count(), 1);

    store.submit(id).expect("an untouched checkout commits");

    assert!(!store.is_live(id));
    assert!(store.draft(id).is_none());
    assert!(store.draft_mut(id).is_none());
    assert_eq!(store.draft_count(), 0);
    assert_eq!(store.submit(id), Err(SubmitError::AlreadySubmitted));

    // a REFUSED submit hands the draft straight back, under the same id
    let refused = store.checkout();
    let _ = F::set_primary(store.draft_mut(refused).expect("live"), F::PRIMARY_INVALID);
    assert!(matches!(
        store.submit(refused),
        Err(SubmitError::Validation(_))
    ));
    assert!(store.is_live(refused));
    assert!(store.draft(refused).is_some());
}

/// C18 — `close` frees the draft, is idempotent, stops the store rebasing it, and is the **only**
/// release path. A handle that is merely forgotten leaves an edit session the store goes on rebasing.
pub fn c18_release_is_explicit_and_idempotent<F: ConformanceFeature>() {
    let (mut store, id) = checked_out::<F>();
    assert_eq!(store.draft_count(), 1);

    store.close(id);
    assert!(!store.is_live(id));
    assert!(store.draft(id).is_none());
    assert_eq!(store.draft_count(), 0);
    assert_eq!(store.rebasing_draft_count(), 0);

    store.close(id);
    store.close(id);
    assert_eq!(store.draft_count(), 0, "close is idempotent");

    assert_eq!(
        store.apply_canonical(F::with_primary(&F::entity(), F::PRIMARY_THEIRS)),
        vec![],
        "a closed draft is not rebased"
    );

    // the price of D16: an id is not an owner, and nothing else releases a draft
    {
        let _forgotten = store.checkout();
    }
    assert_eq!(store.draft_count(), 1, "an id is not an owner");
    assert_eq!(
        store.rebasing_draft_count(),
        1,
        "and the store goes on rebasing a draft nobody can reach"
    );
}

/// C19, store half — the store rebases *every* field on *every* canonical change, so a field whose
/// own canonical never moved is routinely rebased onto its own ancestor. It must not conflict.
pub fn c19_the_store_does_not_conflict_an_unmoved_field<F: ConformanceFeature>() {
    let entity = F::entity();
    let (mut store, id) = checked_out::<F>();

    // I edit the primary field.
    F::set_primary(store.draft_mut(id).expect("live"), F::PRIMARY_MINE).expect("valid");

    // The server changes only the secondary. Nothing else moved.
    assert_eq!(
        store.apply_canonical(F::with_secondary(&entity, F::SECONDARY_THEIRS)),
        vec![id]
    );

    let draft = store.draft(id).expect("live");
    assert_eq!(
        draft.conflicts(),
        vec![],
        "the server never touched the primary field; conflicting it would offer a `take theirs` \
         button holding the user's own ancestor"
    );
    assert!(F::primary(draft).is_dirty(), "my edit survives untouched");
    assert_eq!(text(F::primary(draft)).as_deref(), Some(F::PRIMARY_MINE));
    assert_eq!(
        text(F::secondary(draft)).as_deref(),
        Some(F::SECONDARY_THEIRS),
        "the clean secondary adopted theirs (C02)"
    );
}

/// C22 — "a draft exists" and "a draft rebases" are different questions, and the store answers both.
pub fn c22_draft_count_and_rebasing_draft_count_are_different_questions<F: ConformanceFeature>() {
    let mut store: Store<F::Draft> = Store::new(None);
    let create = store.checkout();
    assert_eq!(store.draft_count(), 1, "a create-flow draft exists");
    assert_eq!(
        store.rebasing_draft_count(),
        0,
        "and is never rebased (C12)"
    );

    store.apply_canonical(F::entity());
    let edit = store.checkout();
    assert_eq!(store.draft_count(), 2);
    assert_eq!(store.rebasing_draft_count(), 1);

    assert_eq!(store.delete_canonical(), vec![edit]);
    assert_eq!(store.draft_count(), 2, "an orphan exists (C11)");
    assert_eq!(store.rebasing_draft_count(), 0, "and is never rebased");

    store.close(create);
    store.close(edit);
    assert_eq!(store.draft_count(), 0);
    assert_eq!(store.rebasing_draft_count(), 0);
}

// =================================================================================================
// C20, C21 — stash and restore
// =================================================================================================

/// C20 — the stash carries each field's last input attempt and its ancestor, and restoring reproduces
/// value, ancestor, validity (including `Invalid { raw }`), dirtiness and the whole-draft bits.
pub fn c20_a_draft_stashes_and_restores<F: ConformanceFeature>() {
    let entity = F::entity();
    let mut draft = F::Draft::from_canonical(Some(&entity), 7);
    F::set_primary(&mut draft, F::PRIMARY_MINE).expect("valid");
    F::set_secondary(&mut draft, F::SECONDARY_INVALID).expect_err("must be rejected");

    let restored = F::Draft::from_stash(&draft.stash());

    // a dirty, valid field: value and ancestor both intact
    assert_eq!(
        text(F::primary(&restored)).as_deref(),
        Some(F::PRIMARY_MINE)
    );
    assert_eq!(
        base_text(F::primary(&restored)).as_deref(),
        Some(F::PRIMARY_BASE)
    );
    assert!(F::primary(&restored).is_dirty());

    // an invalid field: the user's rejected text is still the user's rejected text (C06 does not
    // stop being true because the process died)
    assert!(F::secondary(&restored).value().is_none());
    assert!(F::secondary(&restored).invalid_error().is_some());
    assert!(F::secondary(&restored).is_dirty());

    assert_eq!(restored.base_version(), 7);
    assert_eq!(restored.status(), DraftStatus::Live);

    let dirty = restored.dirty_fields();
    assert_eq!(
        dirty.len(),
        2,
        "exactly the two fields we touched, got {dirty:?}"
    );
    assert!(dirty.contains(&F::primary_id()));
    assert!(dirty.contains(&F::secondary_id()));
}

/// C20 — `sync` is not stashed. `theirs` from before the death names a value the server may no longer
/// hold, so the conflict re-derives against *fresh* canonical rather than being restored from memory.
pub fn c20_sync_is_not_stashed_and_re_derives<F: ConformanceFeature>() {
    let entity = F::entity();
    let mut draft = F::Draft::from_canonical(Some(&entity), 0);
    F::set_primary(&mut draft, F::PRIMARY_MINE).expect("valid");
    draft.rebase(&F::with_primary(&entity, F::PRIMARY_THEIRS), 1);
    assert_eq!(draft.conflicts(), vec![F::primary_id()]);

    // the stash carries no conflict at all...
    let mut restored = F::Draft::from_stash(&draft.stash());
    assert!(restored.conflicts().is_empty());
    assert!(F::primary(&restored).theirs().is_none());
    assert_eq!(
        text(F::primary(&restored)).as_deref(),
        Some(F::PRIMARY_MINE)
    );
    assert_eq!(
        base_text(F::primary(&restored)).as_deref(),
        Some(F::PRIMARY_BASE)
    );

    // ...and the rebase against whatever canonical says NOW re-derives it, naming a value the dead
    // process never saw. This is why a fourth text exists: with three, a stashed `theirs` and a
    // re-derived one would agree, and the test would pass for the wrong reason.
    restored.rebase(&F::with_primary(&entity, F::PRIMARY_OTHER), 5);
    assert_eq!(restored.conflicts(), vec![F::primary_id()]);
    assert_eq!(
        F::primary(&restored).theirs().cloned().map(Value::into_raw),
        Some(F::PRIMARY_OTHER.to_string()),
        "a restored conflict must name the CURRENT canonical, not the one we died holding"
    );
}

/// C21 — adopting a restored draft conflicts exactly those fields whose canonical moved while it was
/// away, and leaves the others dirty and `InSync` (C19 doing the work).
pub fn c21_restore_conflicts_only_the_fields_whose_canonical_moved<F: ConformanceFeature>() {
    let entity = F::entity();

    // Before the death: both fields edited, over a store at `entity()`.
    let stash = {
        let mut store: Store<F::Draft> = Store::new(Some(entity.clone()));
        let id = store.checkout();
        let draft = store.draft_mut(id).expect("live");
        F::set_primary(draft, F::PRIMARY_MINE).expect("valid");
        F::set_secondary(draft, F::SECONDARY_THEIRS).expect("valid");
        draft.stash()
    };

    // After: a fresh store, seeded from a server that moved the PRIMARY and nothing else.
    let mut store: Store<F::Draft> = Store::new(Some(F::with_primary(&entity, F::PRIMARY_THEIRS)));
    let id = store.restore(&stash);
    let draft = store.draft(id).expect("live");

    assert_eq!(
        draft.conflicts(),
        vec![F::primary_id()],
        "only the primary moved on the server while we were away"
    );
    assert_eq!(
        F::primary(draft).theirs().cloned().map(Value::into_raw),
        Some(F::PRIMARY_THEIRS.to_string())
    );
    assert_eq!(text(F::primary(draft)).as_deref(), Some(F::PRIMARY_MINE));

    // the secondary was dirty and untouched by the server: it comes back dirty, not conflicted
    assert!(F::secondary(draft).is_dirty());
    assert!(!F::secondary(draft).is_conflicted());
    assert_eq!(
        text(F::secondary(draft)).as_deref(),
        Some(F::SECONDARY_THEIRS)
    );

    let base_version = draft.base_version();
    assert_eq!(store.rebasing_draft_count(), 1);
    assert_eq!(base_version, store.version());
}

/// C21 — a resolution taken before the restore survives it, because its effect lives in the ancestor
/// and the ancestor is stashed. Nothing is re-litigated.
pub fn c21_a_resolved_conflict_stays_resolved_across_restore<F: ConformanceFeature>() {
    let entity = F::entity();
    let mut draft = F::Draft::from_canonical(Some(&entity), 0);
    F::set_primary(&mut draft, F::PRIMARY_MINE).expect("valid");
    draft.rebase(&F::with_primary(&entity, F::PRIMARY_THEIRS), 1);
    draft.resolve_keep_mine(F::primary_id());
    assert_eq!(
        base_text(F::primary(&draft)).as_deref(),
        Some(F::PRIMARY_THEIRS),
        "keep-mine adopts theirs as the ancestor"
    );

    // the process dies. The server still says what we already accepted.
    let mut store: Store<F::Draft> = Store::new(Some(F::with_primary(&entity, F::PRIMARY_THEIRS)));
    let id = store.restore(&draft.stash());
    let restored = store.draft(id).expect("live");

    assert!(
        restored.conflicts().is_empty(),
        "the user already resolved this; C19's early-out is what keeps it resolved"
    );
    assert_eq!(text(F::primary(restored)).as_deref(), Some(F::PRIMARY_MINE));
    assert!(F::primary(restored).is_dirty());
}

/// C21 — the entity was deleted while we were dead. The restored draft orphans (C11); it does not
/// quietly commit and resurrect the entity.
pub fn c21_restore_into_a_deleted_canonical_orphans_the_draft<F: ConformanceFeature>() {
    let entity = F::entity();
    let mut draft = F::Draft::from_canonical(Some(&entity), 3);
    F::set_primary(&mut draft, F::PRIMARY_MINE).expect("valid");

    let mut store: Store<F::Draft> = Store::new(None); // the server 404s
    let id = store.restore(&draft.stash());

    let restored = store.draft(id).expect("live");
    assert_eq!(restored.status(), DraftStatus::Orphaned);
    assert_eq!(restored.base_version(), 3, "an orphan's stamp stops moving");

    assert_eq!(
        store.rebasing_draft_count(),
        0,
        "an orphan is not registered"
    );
    assert_eq!(store.draft_count(), 1, "...but it very much exists (C22)");
    assert_eq!(store.submit(id), Err(SubmitError::Orphaned));
}

/// C21 — a create-flow draft has no ancestor, so it is never moved by canonical, restored or not.
pub fn c21_a_restored_create_flow_draft_is_never_moved<F: ConformanceFeature>() {
    let mut draft = F::Draft::from_canonical(None, 0);
    F::set_primary(&mut draft, F::PRIMARY_MINE).expect("valid");

    // someone else created the entity while we were dead
    let mut store: Store<F::Draft> = Store::new(Some(F::entity()));
    let id = store.restore(&draft.stash());

    {
        let restored = store.draft(id).expect("live");
        assert_eq!(text(F::primary(restored)).as_deref(), Some(F::PRIMARY_MINE));
        assert!(F::primary(restored).base().is_none());
        assert!(restored.conflicts().is_empty());
        assert_eq!(restored.status(), DraftStatus::Live);
    }
    assert_eq!(
        store.rebasing_draft_count(),
        0,
        "create-flow never registers"
    );

    // ...and a canonical change still does not move it
    assert_eq!(
        store.apply_canonical(F::with_primary(&F::entity(), F::PRIMARY_THEIRS)),
        vec![]
    );
    let restored = store.draft(id).expect("live");
    assert_eq!(text(F::primary(restored)).as_deref(), Some(F::PRIMARY_MINE));
}
