//! **The conformance suite.** Each `cNN_*` test is the executable form of one normative statement
//! in `docs/CONFORMANCE.md`; ARCHITECTURE §7 lists them. C01–C05 are property-based, the rest
//! example-based.
//!
//! This is the set that step 10 turns into per-language contract tests, and step 08 makes generic
//! over a feature. Until then it runs against `spike-profile`, the hand-written "as-if-generated"
//! reference implementation.
//!
//! `conformance_manifest_has_a_test_for_every_id` keeps the document and this file from drifting —
//! the suite's own rung-3 check (VISION's verification ladder).

use bolted_core::*;
use proptest::prelude::*;
use spike_profile::*;

// --------------------------------------------------------------------------------------------
// helpers
// --------------------------------------------------------------------------------------------

fn base_profile() -> Profile {
    Profile {
        username: Username::try_new("alice".to_string()).expect("valid username"),
        name: PersonName::try_new("Alice".to_string()).expect("valid name"),
        email: Email::try_new("alice@corp.example".to_string()).expect("valid email"),
        availability: DateRange::try_new((Date::new(2026, 1, 1), Date::new(2026, 12, 31)))
            .expect("valid range"),
    }
}

fn date_strat() -> impl Strategy<Value = Date> {
    (1970u16..2100, 1u8..=12, 1u8..=28).prop_map(|(y, m, d)| Date::new(y, m, d))
}

fn rule_present(report: &ValidationReport<ProfileField>, rule: &str) -> bool {
    report.rule_errors.iter().any(|v| v.rule == rule)
}

/// Drive the uniqueness check to a passing verdict for the username currently in the draft.
/// Since C16, a *dirty* username with an unrun check blocks commit — so every test that submits
/// an edited username must do this, exactly as a real shell must.
fn pass_check(d: &mut ProfileDraft) {
    let t = d.begin_username_check();
    assert!(d.complete_username_check(t, Ok(())));
}

fn username(s: &str) -> Username {
    Username::try_new(s.to_string()).expect("valid username")
}

/// `base_profile()` with a different canonical username, for rebase.
fn with_username(name: &str) -> Profile {
    let mut p = base_profile();
    p.username = username(name);
    p
}

// --------------------------------------------------------------------------------------------
// C01–C05: property-based
// --------------------------------------------------------------------------------------------

proptest! {
    // C01 — Value::try_new(v.into_raw()) == Ok(v) for every valid v (roundtrip), all four types.
    #[test]
    fn c01_roundtrip_username(raw in ".*") {
        if let Ok(v) = Username::try_new(raw) {
            prop_assert_eq!(Username::try_new(v.clone().into_raw()), Ok(v));
        }
    }

    #[test]
    fn c01_roundtrip_person_name(raw in ".*") {
        if let Ok(v) = PersonName::try_new(raw) {
            prop_assert_eq!(PersonName::try_new(v.clone().into_raw()), Ok(v));
        }
    }

    #[test]
    fn c01_roundtrip_email(raw in ".*") {
        if let Ok(v) = Email::try_new(raw) {
            prop_assert_eq!(Email::try_new(v.clone().into_raw()), Ok(v));
        }
    }

    #[test]
    fn c01_roundtrip_date_range((a, b) in (date_strat(), date_strat())) {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let v = DateRange::try_new((start, end)).expect("ordered range is valid");
        prop_assert_eq!(DateRange::try_new(v.clone().into_raw()), Ok(v));
    }

    // C02 — a non-dirty field always equals canonical after rebase (InSync).
    #[test]
    fn c02_untouched_follows_canonical(a in "[a-z]{3,20}", b in "[a-z]{3,20}") {
        let av = Username::try_new(a).expect("valid");
        let bv = Username::try_new(b).expect("valid");
        let mut f = Field::from_base(av);
        prop_assert!(!f.is_dirty());
        f.rebase(bv.clone());
        prop_assert_eq!(f.value(), Some(&bv));
        prop_assert!(matches!(f.sync(), SyncState::InSync));
        prop_assert!(!f.is_dirty());
    }

    // C03 — a dirty field whose canonical MOVED is never silently overwritten by rebase (yours
    // preserved, Conflicted), and the recorded ancestor does not move.
    //
    // `theirs != base` is the precondition this property was missing until step 07. Without it the
    // statement is false — an unmoved canonical must not conflict anything (C19) — but proptest
    // draws the three strings independently and never samples `theirs == base`, so the suite
    // asserted the bug instead of catching it.
    #[test]
    fn c03_dirty_preserved_on_conflict(
        base in "[a-z]{3,20}", mine in "[a-z]{3,20}", theirs in "[a-z]{3,20}"
    ) {
        prop_assume!(mine != base);
        prop_assume!(theirs != mine);
        prop_assume!(theirs != base);
        let basev = Username::try_new(base).expect("valid");
        let minev = Username::try_new(mine).expect("valid");
        let theirsv = Username::try_new(theirs).expect("valid");

        let mut f = Field::from_base(basev.clone());
        f.try_set(minev.clone().into_raw()).expect("valid");
        prop_assert!(f.is_dirty());
        f.rebase(theirsv.clone());

        prop_assert_eq!(f.value(), Some(&minev));   // yours preserved
        prop_assert_eq!(f.base(), Some(&basev));    // the common ancestor is still the ancestor
        prop_assert_eq!(f.theirs(), Some(&theirsv));
        prop_assert!(f.is_conflicted());
    }

    // C04 — convergent rebase (yours == theirs) lands clean and InSync.
    #[test]
    fn c04_convergent_rebase_clean(base in "[a-z]{3,20}", edit in "[a-z]{3,20}") {
        prop_assume!(edit != base);
        let basev = Username::try_new(base).expect("valid");
        let editv = Username::try_new(edit).expect("valid");

        let mut f = Field::from_base(basev);
        f.try_set(editv.clone().into_raw()).expect("valid");
        prop_assert!(f.is_dirty());
        f.rebase(editv.clone()); // theirs == yours

        prop_assert!(!f.is_dirty());
        prop_assert!(matches!(f.sync(), SyncState::InSync));
        prop_assert_eq!(f.value(), Some(&editv));
    }

    // C05 — setting a field back to its base value clears dirty (revert-for-free).
    #[test]
    fn c05_revert_clears_dirty(base in "[a-z]{3,20}", edit in "[a-z]{3,20}") {
        prop_assume!(edit != base);
        let basev = Username::try_new(base).expect("valid");
        let editv = Username::try_new(edit).expect("valid");

        let mut f = Field::from_base(basev.clone());
        prop_assert!(!f.is_dirty());
        f.try_set(editv.into_raw()).expect("valid");
        prop_assert!(f.is_dirty());
        f.try_set(basev.clone().into_raw()).expect("valid");
        prop_assert!(!f.is_dirty());
        prop_assert_eq!(f.value(), Some(&basev));
    }
}

// --------------------------------------------------------------------------------------------
// C06–C18: example-based
// --------------------------------------------------------------------------------------------

// C06 — a failed try_set blocks submit; no stale valid value slips through.
#[test]
fn c06_failed_set_blocks_submit() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.checkout();
    {
        let d = store.draft_mut(id).expect("live");
        // last valid value is "alice"; now enter invalid text
        assert!(d.try_set_username("ab".to_string()).is_err()); // too short
        assert!(matches!(d.username.validity(), Validity::Invalid { .. }));
    }
    match store.submit(id) {
        Err(SubmitError::Validation(report)) => {
            assert!(
                report
                    .field_errors
                    .iter()
                    .any(|(f, _)| *f == ProfileField::Username)
            );
        }
        other => panic!("expected validation error, got {other:?}"),
    }
    // canonical unchanged: the stale "alice" was NOT silently submitted
    assert_eq!(
        store.canonical().map(|p| p.username.as_str()),
        Some("alice")
    );
    // and the refusal did not destroy the edit session
    assert!(store.is_live(id));
}

// C07 — commit succeeds iff all fields Valid, none Conflicted, no rule violations, status Live;
// and the committed entity equals the field values. Each refusal is typed (no synthetic rules).
#[test]
fn c07_commit_equivalence_and_entity_equals_fields() {
    // fully valid, checked, no conflicts, Live -> Ok, entity equals the field values
    let mut d = ProfileDraft::from_canonical(None, 0);
    d.try_set_username("bob".to_string()).expect("valid");
    d.try_set_name("Bob".to_string()).expect("valid");
    d.try_set_email("bob@corp.example".to_string())
        .expect("valid");
    d.try_set_availability(Date::new(2026, 3, 1), Date::new(2026, 3, 10))
        .expect("valid");
    pass_check(&mut d);

    let expected = Profile {
        username: username("bob"),
        name: PersonName::try_new("Bob".to_string()).expect("valid"),
        email: Email::try_new("bob@corp.example".to_string()).expect("valid"),
        availability: DateRange::try_new((Date::new(2026, 3, 1), Date::new(2026, 3, 10)))
            .expect("valid"),
    };
    match d.commit() {
        Ok(p) => assert_eq!(p, expected),
        Err((_, e)) => panic!("expected Ok, got {e:?}"),
    }

    // an invalid/unset field -> Validation
    let mut d2 = ProfileDraft::from_canonical(None, 0);
    d2.try_set_username("bob".to_string()).expect("valid"); // others left Unset
    pass_check(&mut d2);
    assert!(matches!(d2.commit(), Err((_, CommitError::Validation(_)))));

    // an unresolved conflict -> Conflicted, NOT a synthetic rule violation
    let base = base_profile();
    let mut d3 = ProfileDraft::from_canonical(Some(&base), 0);
    d3.try_set_username("carol".to_string()).expect("valid"); // dirty vs "alice"
    d3.rebase(&with_username("dave"), 1); // yours "carol" != theirs "dave" -> Conflicted
    assert!(d3.conflicts().contains(&ProfileField::Username));
    match d3.commit() {
        Err((_, CommitError::Conflicted { fields })) => {
            assert_eq!(fields, vec![ProfileField::Username]);
        }
        other => panic!("expected Conflicted, got {:?}", other.err().map(|(_, e)| e)),
    }

    // an orphaned draft -> Orphaned
    let mut d4 = ProfileDraft::from_canonical(Some(&base), 0);
    d4.orphan();
    assert!(matches!(d4.commit(), Err((_, CommitError::Orphaned))));
}

// C08 — rebase re-runs tier-2 validation (the corporate_email rule flips when the rebased
// username changes, even though it pins to Email).
#[test]
fn c08_rebase_reruns_tier2_rule() {
    let base = base_profile(); // username "alice", email "alice@corp.example"
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);

    // edit email to a dirty non-corp domain; rule still OK because username is "alice"
    d.try_set_email("bob@other.com".to_string()).expect("valid");
    assert!(!rule_present(&d.validate(), "corporate_email"));

    // canonical username becomes corp_* -> rebase adopts it (username was untouched)
    d.rebase(&with_username("corp_bob"), 1);

    // now the rule fires: re-evaluated with the rebased username
    let report = d.validate();
    assert!(rule_present(&report, "corporate_email"));
    let v = report
        .rule_errors
        .iter()
        .find(|v| v.rule == "corporate_email")
        .expect("present");
    assert_eq!(v.pins, vec![ProfileField::Email]); // pinned to Email

    // And `email` — dirty, but its canonical value never moved — is NOT conflicted. Until step 07
    // it was: this very test drove `email.rebase("alice@corp.example")` against its own ancestor
    // and got `Conflicted { theirs: "alice@corp.example" }`. The suite never looked (C19).
    assert_eq!(d.conflicts(), vec![]);
}

// C09 — resolve_keep_mine: value=yours, base=theirs, dirty, InSync. resolve_take_theirs:
// value=theirs, clean, InSync.
#[test]
fn c09_resolution_semantics() {
    // keep-mine
    let mut f = Field::from_base(username("alice"));
    let mine = username("mine1");
    let theirs = username("their1");
    f.try_set(mine.clone().into_raw()).expect("valid");
    f.rebase(theirs.clone());
    assert!(f.is_conflicted());
    f.resolve_keep_mine();
    assert_eq!(f.value(), Some(&mine));
    assert_eq!(f.base(), Some(&theirs));
    assert!(f.is_dirty());
    assert!(matches!(f.sync(), SyncState::InSync));

    // take-theirs
    let mut g = Field::from_base(username("alice"));
    let mine2 = username("mine2");
    let theirs2 = username("their2");
    g.try_set(mine2.into_raw()).expect("valid");
    g.rebase(theirs2.clone());
    g.resolve_take_theirs();
    assert_eq!(g.value(), Some(&theirs2));
    assert!(!g.is_dirty());
    assert!(matches!(g.sync(), SyncState::InSync));
}

// C10 — stale async completions (old sequence) are ignored; the latest begin wins.
#[test]
fn c10_stale_async_ignored() {
    // unit level
    let mut sf: SingleFlight<i32> = SingleFlight::new();
    let a = sf.begin();
    let b = sf.begin(); // supersedes a
    assert!(!sf.complete(a, 1)); // stale
    assert!(sf.complete(b, 2)); // latest wins
    assert_eq!(sf.state(), &CheckState::Done { verdict: 2 });

    // draft level: a stale FAILING verdict must not resurrect after a fresh PASSING one
    let mut d = ProfileDraft::from_canonical(None, 0);
    let first = d.begin_username_check();
    let second = d.begin_username_check();
    assert!(!d.complete_username_check(first, Err(ErrorData::new("taken"))));
    assert!(d.complete_username_check(second, Ok(())));
    assert!(!rule_present(&d.validate(), "username_unique"));
}

// C11 — canonical deletion orphans the draft; submit on orphaned is a typed error.
#[test]
fn c11_delete_orphans_and_submit_is_typed() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.checkout();
    assert_eq!(store.delete_canonical(), vec![id]); // the effect, returned as data
    assert_eq!(
        store.draft(id).expect("live").status(),
        DraftStatus::Orphaned
    );
    assert_eq!(store.submit(id), Err(SubmitError::Orphaned));
    assert!(store.canonical().is_none());
    assert!(store.is_live(id)); // the refusal handed the draft back
}

// C12 — create-flow drafts (no base) never rebase and commit normally.
#[test]
fn c12_create_flow_never_rebases_and_commits() {
    let mut store: ProfileStore = Store::new(None);
    let id = store.checkout(); // create-flow, not registered for rebase

    // someone else creates canonical; the create draft must NOT rebase — and `apply_canonical`
    // says so in its return value, which is the only thing an FFI shell would emit a snapshot for.
    assert_eq!(store.apply_canonical(base_profile()), vec![]);
    {
        let d = store.draft(id).expect("live");
        assert!(matches!(d.username.validity(), Validity::Unset));
        assert!(matches!(d.name.validity(), Validity::Unset));
        assert!(matches!(d.email.validity(), Validity::Unset));
        assert!(matches!(d.availability.validity(), Validity::Unset));
        assert!(d.dirty_fields().is_empty());
    }

    // fill and commit normally
    {
        let d = store.draft_mut(id).expect("live");
        d.try_set_username("carol".to_string()).expect("valid");
        d.try_set_name("Carol".to_string()).expect("valid");
        d.try_set_email("carol@corp.example".to_string())
            .expect("valid");
        d.try_set_availability(Date::new(2026, 5, 1), Date::new(2026, 5, 2))
            .expect("valid");
        pass_check(d);
    }
    store.submit(id).expect("create-flow submit ok");
    assert_eq!(
        store.canonical().map(|p| p.username.as_str()),
        Some("carol")
    );
}

// C13 — a change to the checked field's VALUE (edit or rebase) resets the async verdict to
// unchecked; a mutation that leaves the value unchanged keeps it (ARCHITECTURE §2/§8). The check
// is pinned to `username`; a verdict endorses a value, so a changed value un-endorses it.
#[test]
fn c13_async_verdict_resets_on_value_change() {
    let base = base_profile(); // username "alice"

    let passed = |d: &ProfileDraft| {
        matches!(
            d.username_check_state(),
            CheckState::Done { verdict: Ok(()) }
        )
    };

    // (a) passed, then edit to a DIFFERENT value -> reset to Idle.
    let mut a = ProfileDraft::from_canonical(Some(&base), 0);
    pass_check(&mut a);
    a.try_set_username("alice2".to_string()).expect("valid");
    assert!(matches!(a.username_check_state(), CheckState::Idle));

    // (b) passed, then edit to the SAME value -> verdict stands (value-based, like dirty).
    let mut b = ProfileDraft::from_canonical(Some(&base), 0);
    pass_check(&mut b);
    b.try_set_username("alice".to_string()).expect("valid");
    assert!(passed(&b));

    // (c) clean field, rebase adopts a new canonical username -> value moves -> reset.
    let mut c = ProfileDraft::from_canonical(Some(&base), 0);
    pass_check(&mut c);
    c.rebase(&with_username("newalice"), 1);
    assert!(matches!(c.username_check_state(), CheckState::Idle));

    // (d) dirty field, rebase CONFLICTS (yours preserved) -> value unchanged -> verdict stands.
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);
    d.try_set_username("mine".to_string()).expect("valid");
    pass_check(&mut d); // the verdict endorses "mine"
    d.rebase(&with_username("theirs"), 1);
    assert!(d.username.is_conflicted());
    assert_eq!(d.username.value().map(|u| u.as_str()), Some("mine")); // preserved
    assert!(passed(&d));

    // (e) resolve a username conflict: take-theirs moves the value -> reset; keep-mine does not.
    let mut e_take = ProfileDraft::from_canonical(Some(&base), 0);
    e_take.try_set_username("mine".to_string()).expect("valid");
    pass_check(&mut e_take);
    e_take.rebase(&with_username("theirs"), 1);
    e_take.resolve_take_theirs(ProfileField::Username);
    assert_eq!(e_take.username.value().map(|u| u.as_str()), Some("theirs"));
    assert!(matches!(e_take.username_check_state(), CheckState::Idle));

    let mut e_keep = ProfileDraft::from_canonical(Some(&base), 0);
    e_keep.try_set_username("mine".to_string()).expect("valid");
    pass_check(&mut e_keep);
    e_keep.rebase(&with_username("theirs"), 1);
    e_keep.resolve_keep_mine(ProfileField::Username);
    assert_eq!(e_keep.username.value().map(|u| u.as_str()), Some("mine"));
    assert!(passed(&e_keep));
}

// C14 — editing a conflicted field to a value equal to `theirs` resolves the conflict: clean,
// InSync, base adopted. The mirror image of C04, where the canonical change arrives second.
#[test]
fn c14_editing_to_theirs_auto_converges() {
    let base = base_profile();
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);
    d.try_set_username("mine1".to_string()).expect("valid");
    d.rebase(&with_username("their1"), 1);
    assert_eq!(d.conflicts(), vec![ProfileField::Username]);

    // type their value, character by character as a UI would (the short prefixes are invalid)
    for prefix in ["t", "th", "the", "thei", "their", "their1"] {
        let _ = d.try_set_username(prefix.to_string());
    }

    assert!(d.conflicts().is_empty());
    assert!(!d.username.is_dirty());
    assert!(matches!(d.username.sync(), SyncState::InSync));
    assert_eq!(d.username.base().map(|u| u.as_str()), Some("their1"));

    // and the value moved, so C13 reset the verdict — the new value was never checked
    assert!(matches!(d.username_check_state(), CheckState::Idle));
}

// C15 — a rebase records the store version the draft is now based on. Before the freeze this stamp
// was written once at checkout, so a draft snapshot's `version` was stale after any rebase and the
// version-guarded reconcile it existed for could never fire.
#[test]
fn c15_rebase_advances_base_version() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.checkout();
    assert_eq!(store.version(), 0);
    assert_eq!(store.draft(id).expect("live").base_version(), 0);

    store.apply_canonical(with_username("bravo"));
    assert_eq!(store.version(), 1);
    assert_eq!(store.draft(id).expect("live").base_version(), 1);

    store.apply_canonical(with_username("charlie"));
    assert_eq!(store.version(), 2);
    assert_eq!(store.draft(id).expect("live").base_version(), 2);

    // an orphaned draft is based on no canonical at all: its stamp stops moving
    store.delete_canonical();
    assert_eq!(store.version(), 3);
    assert_eq!(store.draft(id).expect("live").base_version(), 2);
}

// C16 — an unrun async check blocks commit only while its pinned field is dirty. A clean field
// still holds the canonical value, which was verified when it was committed.
#[test]
fn c16_unrun_check_blocks_only_a_dirty_field() {
    let base = base_profile();

    // clean username, check never run -> commits
    let mut clean = ProfileDraft::from_canonical(Some(&base), 0);
    clean
        .try_set_name("Renamed".to_string())
        .expect("valid name");
    assert!(matches!(clean.username_check_state(), CheckState::Idle));
    assert!(clean.validate().is_ok());
    assert!(clean.commit().is_ok());

    // dirty username, check never run -> refused, pinned to Username
    let mut dirty = ProfileDraft::from_canonical(Some(&base), 0);
    dirty.try_set_username("alice2".to_string()).expect("valid");
    let report = dirty.validate();
    let v = report
        .rule_errors
        .iter()
        .find(|v| v.rule == "username_unique")
        .expect("an unrun check on a dirty field must block");
    assert_eq!(v.error.key, "username_check_required");
    assert_eq!(v.pins, vec![ProfileField::Username]);
    assert!(matches!(
        dirty.commit(),
        Err((_, CommitError::Validation(_)))
    ));

    // ...and a passing check unblocks it
    let mut checked = ProfileDraft::from_canonical(Some(&base), 0);
    checked
        .try_set_username("alice2".to_string())
        .expect("valid");
    pass_check(&mut checked);
    assert!(checked.validate().is_ok());
    assert!(checked.commit().is_ok());

    // reverting to the canonical value makes it clean again, so no check is demanded (C05 + C16)
    let mut reverted = ProfileDraft::from_canonical(Some(&base), 0);
    reverted
        .try_set_username("alice2".to_string())
        .expect("valid");
    reverted
        .try_set_username("alice".to_string())
        .expect("valid");
    assert!(!reverted.username.is_dirty());
    assert!(reverted.validate().is_ok());
}

// C17 — a successful submit releases the draft: the id stops being live. A refused submit does not.
#[test]
fn c17_submit_releases_the_draft() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.checkout();
    assert!(store.is_live(id));
    assert_eq!(store.draft_count(), 1);

    store.submit(id).expect("a clean draft commits");

    assert!(!store.is_live(id));
    assert!(store.draft(id).is_none());
    assert!(store.draft_mut(id).is_none());
    assert_eq!(store.draft_count(), 0);
    assert_eq!(store.submit(id), Err(SubmitError::AlreadySubmitted));

    // by contrast, a REFUSED submit hands the draft straight back (F3): the edit session survives,
    // under the same id — a shell holding it does not have to re-checkout.
    let id2 = store.checkout();
    store
        .draft_mut(id2)
        .expect("live")
        .try_set_name("  ".to_string())
        .expect_err("blank name is invalid");
    assert!(matches!(store.submit(id2), Err(SubmitError::Validation(_))));
    assert!(store.is_live(id2));
    assert!(store.draft(id2).is_some());
}

// C18 — close() frees the draft, is idempotent, and the store stops rebasing it. Since step 08 it is
// also the ONLY release path: a `DraftId` is not an owner, so dropping it releases nothing. Android's
// ART never freed a draft either (step 05, H1); the contract now reads the same on every platform
// instead of being forgiving in Rust alone.
#[test]
fn c18_close_frees_the_draft_and_is_idempotent() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.checkout();
    assert_eq!(store.draft_count(), 1);

    store.close(id);
    assert!(!store.is_live(id));
    assert!(store.draft(id).is_none());
    assert_eq!(store.draft_count(), 0);
    assert_eq!(store.rebasing_draft_count(), 0);

    // idempotent — and closing an id that is already gone is not an error
    store.close(id);
    store.close(id);
    assert_eq!(store.draft_count(), 0);

    // a closed draft is not rebased, and a canonical change over it is harmless
    assert_eq!(store.apply_canonical(with_username("bravo")), vec![]);
    assert_eq!(store.draft_count(), 0);

    // The other half of the contract, and the price of D16: nothing else releases a draft. An id is
    // `Copy`; forgetting it leaks an edit session the store keeps rebasing forever. This assertion
    // *is* the leak, stated on purpose — it is what a Kotlin `onCleared()` exists to prevent, and
    // why `bolted-ffi` still owes a `Cleaner` backstop (ARCHITECTURE §9).
    //
    // (Writing `drop(forgotten)` here earns `dropping_copy_types`: "calls to std::mem::drop with a
    // value that implements Copy does nothing". The lint is the proof. Scope exit says it quietly.)
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

// C22 — the store answers two different questions about drafts, and they have different answers.
// Until step 08 there was one `live_draft_count()` on each side of the FFI: it meant "would be
// rebased" in the core and "exists" in the wrapper. They disagreed by one on every create-flow
// draft, for five steps. Step 07 shipped a Swift test to *document* the divergence, because with two
// hand-written store loops nothing could fix it. Deleting one of them did.
#[test]
fn c22_draft_count_and_rebasing_draft_count_are_different_questions() {
    // a create-flow draft exists, and is not rebased (C12)
    let mut store: ProfileStore = Store::new(None);
    let create = store.checkout();
    assert_eq!(store.draft_count(), 1);
    assert_eq!(store.rebasing_draft_count(), 0);

    // an entity-backed checkout is both
    store.apply_canonical(base_profile());
    let edit = store.checkout();
    assert_eq!(store.draft_count(), 2);
    assert_eq!(store.rebasing_draft_count(), 1);

    // an orphan exists, and is not rebased (C11)
    assert_eq!(store.delete_canonical(), vec![edit]);
    assert_eq!(store.draft_count(), 2);
    assert_eq!(store.rebasing_draft_count(), 0);

    // close removes it from both
    store.close(create);
    store.close(edit);
    assert_eq!(store.draft_count(), 0);
    assert_eq!(store.rebasing_draft_count(), 0);
}

// C19 — rebase is a THREE-way merge. The store rebases every field of a draft on every canonical
// change, so a field whose own canonical value never moved is routinely rebased onto its own
// ancestor. That must not conflict it.
#[test]
fn c19_a_field_whose_canonical_did_not_move_is_not_conflicted() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.checkout();

    // I edit `name`.
    store
        .draft_mut(id)
        .expect("live")
        .try_set_name("My Name".to_string())
        .expect("valid");

    // The server changes only `username`. Nothing else moved.
    store.apply_canonical(with_username("bravo"));

    let d = store.draft(id).expect("live");
    assert_eq!(
        d.conflicts(),
        vec![],
        "the server never touched `name`; conflicting it offers a `take theirs` button whose \
         value is the user's own ancestor"
    );
    assert!(d.name.is_dirty()); // my edit survives untouched
    assert_eq!(d.name.value().map(|v| v.as_str()), Some("My Name"));
    assert_eq!(d.username.value().map(|v| v.as_str()), Some("bravo")); // clean -> adopted (C02)
}

// C19, second half: canonical moving BACK to the ancestor clears an existing conflict — the other
// side stopped disagreeing. And a repeated rebase onto the same canonical changes nothing.
#[test]
fn c19_canonical_returning_to_the_ancestor_clears_the_conflict_and_rebase_is_idempotent() {
    let base = base_profile();
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);
    d.try_set_username("mine1".to_string()).expect("valid");

    d.rebase(&with_username("their1"), 1);
    assert_eq!(d.conflicts(), vec![ProfileField::Username]);

    // idempotent: rebasing onto the same canonical again is a no-op
    let theirs_before = d.username.theirs().cloned();
    d.rebase(&with_username("their1"), 2);
    assert_eq!(d.conflicts(), vec![ProfileField::Username]);
    assert_eq!(d.username.theirs().cloned(), theirs_before);

    // the server reverts to the ancestor: no one else is changing this field any more
    d.rebase(&base, 3);
    assert!(d.conflicts().is_empty());
    assert_eq!(d.username.value().map(|v| v.as_str()), Some("mine1"));
    assert!(d.username.is_dirty());
    assert_eq!(d.username.base().map(|v| v.as_str()), Some("alice"));
}

// --------------------------------------------------------------------------------------------
// C20–C21: stash / restore across process death
// --------------------------------------------------------------------------------------------

// C20 — every field's value, ancestor, validity and dirtiness survive the round trip, including an
// `Invalid { raw }` the user never fixed (C06 does not stop being true because the process died).
#[test]
fn c20_stash_round_trips_values_ancestors_and_validity() {
    let base = base_profile();
    let mut d = ProfileDraft::from_canonical(Some(&base), 7);
    d.try_set_name("My Name".to_string()).expect("valid");
    d.try_set_email("not-an-email".to_string())
        .expect_err("no @: invalid");

    let restored = ProfileDraft::from_stash(&d.stash());

    // a dirty, valid field: value and ancestor both intact
    assert_eq!(restored.name.value().map(|v| v.as_str()), Some("My Name"));
    assert_eq!(restored.name.base().map(|v| v.as_str()), Some("Alice"));
    assert!(restored.name.is_dirty());

    // an invalid field: the user's rejected text is still the user's rejected text
    match restored.email.validity() {
        Validity::Invalid { raw, .. } => assert_eq!(raw, "not-an-email"),
        other => panic!("expected the raw attempt to survive, got {other:?}"),
    }
    assert!(restored.email.is_dirty());

    // an untouched field stays clean, and the whole-draft bits carry over
    assert_eq!(restored.username.value().map(|v| v.as_str()), Some("alice"));
    assert!(!restored.username.is_dirty());
    assert_eq!(restored.base_version(), 7);
    assert_eq!(restored.status(), DraftStatus::Live);
    assert_eq!(
        restored.dirty_fields(),
        vec![ProfileField::Name, ProfileField::Email]
    );
}

// C20 — an async verdict does NOT survive: it endorses a value against a server state that may have
// moved while we were dead. C13 + C16 then make the restored draft safe with no new invariant.
#[test]
fn c20_an_async_verdict_does_not_survive_the_stash() {
    let base = base_profile();
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);
    d.try_set_username("alice2".to_string()).expect("valid");
    pass_check(&mut d);
    assert!(matches!(
        d.username_check_state(),
        CheckState::Done { verdict: Ok(()) }
    ));
    assert!(d.validate().is_ok());

    let restored = ProfileDraft::from_stash(&d.stash());

    assert!(matches!(restored.username_check_state(), CheckState::Idle));
    assert!(restored.username.is_dirty());
    let report = restored.validate();
    let v = report
        .rule_errors
        .iter()
        .find(|v| v.rule == "username_unique")
        .expect("C16 must demand a fresh check for a restored dirty username");
    assert_eq!(v.error.key, "username_check_required");
}

// C20 — `sync` is not stashed. `theirs` from before the death is a value the server may no longer
// hold, so the conflict is re-derived against FRESH canonical, not restored from a stale memory.
#[test]
fn c20_sync_is_not_stashed_and_re_derives_against_fresh_canonical() {
    let base = base_profile();
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);
    d.try_set_username("mine1".to_string()).expect("valid");
    d.rebase(&with_username("their1"), 1);
    assert_eq!(d.conflicts(), vec![ProfileField::Username]);

    // the stash carries no conflict...
    let mut restored = ProfileDraft::from_stash(&d.stash());
    assert!(restored.conflicts().is_empty());
    assert_eq!(restored.username.value().map(|v| v.as_str()), Some("mine1"));
    assert_eq!(restored.username.base().map(|v| v.as_str()), Some("alice"));

    // ...and the rebase against whatever canonical says NOW re-derives it, with a fresh `theirs`
    restored.rebase(&with_username("their2"), 5);
    assert_eq!(restored.conflicts(), vec![ProfileField::Username]);
    assert_eq!(
        restored.username.theirs().map(|v| v.as_str()),
        Some("their2"),
        "a restored conflict must name the CURRENT canonical, not the one we died holding"
    );
}

// C21 — restore is a rebase: `Store::adopt` conflicts exactly those fields whose canonical moved
// while the process was dead, and leaves the rest alone (which is C19 doing the work).
#[test]
fn c21_restore_conflicts_only_the_fields_whose_canonical_moved() {
    // Before the death: two dirty fields on a store at the base profile.
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.checkout();
    let stash = {
        let d = store.draft_mut(id).expect("live");
        d.try_set_name("My Name".to_string()).expect("valid");
        d.try_set_email("mine@other.com".to_string())
            .expect("valid");
        d.stash()
    };
    drop(store); // the process dies; the core-side draft dies with it

    // After: a fresh store, seeded from the server — which moved `email` and nothing else.
    let mut server = base_profile();
    server.email = Email::try_new("server@corp.example".to_string()).expect("valid");
    let mut store: ProfileStore = Store::new(Some(server));

    let id = store.restore(&stash);
    let d = store.draft(id).expect("live");

    assert_eq!(
        d.conflicts(),
        vec![ProfileField::Email],
        "only `email` moved on the server while we were away"
    );
    assert_eq!(
        d.email.theirs().map(|v| v.as_str()),
        Some("server@corp.example")
    );
    assert_eq!(d.email.value().map(|v| v.as_str()), Some("mine@other.com"));

    // `name` was dirty and untouched by the server: it comes back dirty, not conflicted.
    assert!(d.name.is_dirty());
    assert!(!d.name.is_conflicted());
    assert_eq!(d.name.value().map(|v| v.as_str()), Some("My Name"));

    // and the draft is registered for live rebase again, stamped with the new store version
    let base_version = d.base_version();
    assert_eq!(store.rebasing_draft_count(), 1);
    assert_eq!(base_version, store.version());
}

// C21 — a prior `resolve_keep_mine` survives, because its effect lives in the *ancestor* and the
// ancestor is stashed. The restored field stays dirty and in sync; nothing re-litigates.
#[test]
fn c21_a_resolved_conflict_stays_resolved_across_restore() {
    let base = base_profile();
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);
    d.try_set_name("My Name".to_string()).expect("valid");
    d.rebase(
        &{
            let mut p = base_profile();
            p.name = PersonName::try_new("Their Name".to_string()).expect("valid");
            p
        },
        1,
    );
    d.resolve_keep_mine(ProfileField::Name); // base := "Their Name", value stays "My Name"
    assert_eq!(d.name.base().map(|v| v.as_str()), Some("Their Name"));

    // The process dies. The server still says "Their Name" — the value we already accepted.
    let mut server = base_profile();
    server.name = PersonName::try_new("Their Name".to_string()).expect("valid");
    let mut store: ProfileStore = Store::new(Some(server));

    let id = store.restore(&d.stash());
    let d = store.draft(id).expect("live");
    assert!(
        d.conflicts().is_empty(),
        "the user already resolved this; C19's early-out is what keeps it resolved"
    );
    assert_eq!(d.name.value().map(|v| v.as_str()), Some("My Name"));
    assert!(d.name.is_dirty());
}

// C21 — the entity was deleted while we were dead. The restored draft orphans (C11); it does not
// quietly commit and resurrect the entity.
#[test]
fn c21_restore_into_a_deleted_canonical_orphans_the_draft() {
    let base = base_profile();
    let mut d = ProfileDraft::from_canonical(Some(&base), 3);
    d.try_set_name("My Name".to_string()).expect("valid");

    let mut store: ProfileStore = Store::new(None); // the server 404s
    let id = store.restore(&d.stash());

    let d = store.draft(id).expect("live");
    assert_eq!(d.status(), DraftStatus::Orphaned);
    assert_eq!(d.base_version(), 3); // an orphan's stamp stops moving (C15)

    assert_eq!(store.rebasing_draft_count(), 0); // an orphan is not registered for rebase
    assert_eq!(store.draft_count(), 1); // ...but it very much exists (C22)
    assert_eq!(store.submit(id), Err(SubmitError::Orphaned));
}

// C21 — a create-flow draft has no ancestor, so it is never moved by canonical, restored or not
// (C12). It commits normally.
#[test]
fn c21_a_restored_create_flow_draft_is_never_moved() {
    let mut d = ProfileDraft::from_canonical(None, 0);
    d.try_set_username("newbie".to_string()).expect("valid");

    // someone else created the entity while we were dead
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let id = store.restore(&d.stash());

    let d = store.draft(id).expect("live");
    assert_eq!(d.username.value().map(|v| v.as_str()), Some("newbie"));
    assert!(d.username.base().is_none());
    assert!(d.conflicts().is_empty());
    assert_eq!(d.status(), DraftStatus::Live);
    assert_eq!(
        store.rebasing_draft_count(),
        0,
        "create-flow never registers"
    );

    // ...and a canonical change still does not move it
    assert_eq!(store.apply_canonical(with_username("bravo")), vec![]);
    let d = store.draft(id).expect("live");
    assert_eq!(d.username.value().map(|v| v.as_str()), Some("newbie"));
}

// --------------------------------------------------------------------------------------------
// The suite's own drift check (VISION's rung 3): the document and this file must not disagree.
// --------------------------------------------------------------------------------------------

/// Every `CNN` in `docs/CONFORMANCE.md` has at least one `cNN_*` test here, and every `cNN_*` test
/// here is documented there. Without this, `CONFORMANCE.md` is prose that rots.
#[test]
fn conformance_manifest_has_a_test_for_every_id() {
    let doc_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/CONFORMANCE.md");
    let doc = std::fs::read_to_string(doc_path).expect("docs/CONFORMANCE.md must exist");
    let suite =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/conformance.rs"))
            .expect("this file must be readable");

    // Normative rows start `| C07 |`.
    let documented: Vec<String> = doc
        .lines()
        .filter_map(|l| l.strip_prefix("| C"))
        .filter_map(|rest| rest.split('|').next())
        .map(|id| format!("C{}", id.trim()))
        .filter(|id| id.len() == 3 && id[1..].chars().all(|c| c.is_ascii_digit()))
        .collect();

    // Tests are declared `fn c07_...`.
    let implemented: Vec<String> = suite
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("fn c"))
        .filter(|rest| rest.len() >= 3 && rest[..2].chars().all(|c| c.is_ascii_digit()))
        .map(|rest| format!("C{}", &rest[..2]))
        .collect();

    assert!(!documented.is_empty(), "parsed no IDs from CONFORMANCE.md");

    for id in &documented {
        assert!(
            implemented.contains(id),
            "{id} is normative in docs/CONFORMANCE.md but has no `{}_*` test",
            id.to_lowercase()
        );
    }
    for id in &implemented {
        assert!(
            documented.contains(id),
            "`{}_*` exists but {id} is not a normative row in docs/CONFORMANCE.md",
            id.to_lowercase()
        );
    }
}
