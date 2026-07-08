//! The conformance-suite seed: ARCHITECTURE §7 invariants I1–I12, one named test each
//! (`i01_*` … `i12_*`) so the mapping to the design is auditable. I1–I5 are property-based; the
//! rest are example-based. This suite is what later becomes the per-language contract-test set.

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

// --------------------------------------------------------------------------------------------
// I1–I5: property-based
// --------------------------------------------------------------------------------------------

proptest! {
    // I1 — Value::try_new(v.into_raw()) == Ok(v) for every valid v (roundtrip), all four types.
    #[test]
    fn i01_roundtrip_username(raw in ".*") {
        if let Ok(v) = Username::try_new(raw) {
            prop_assert_eq!(Username::try_new(v.clone().into_raw()), Ok(v));
        }
    }

    #[test]
    fn i01_roundtrip_person_name(raw in ".*") {
        if let Ok(v) = PersonName::try_new(raw) {
            prop_assert_eq!(PersonName::try_new(v.clone().into_raw()), Ok(v));
        }
    }

    #[test]
    fn i01_roundtrip_email(raw in ".*") {
        if let Ok(v) = Email::try_new(raw) {
            prop_assert_eq!(Email::try_new(v.clone().into_raw()), Ok(v));
        }
    }

    #[test]
    fn i01_roundtrip_date_range((a, b) in (date_strat(), date_strat())) {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let v = DateRange::try_new((start, end)).expect("ordered range is valid");
        prop_assert_eq!(DateRange::try_new(v.into_raw()), Ok(v));
    }

    // I2 — a non-dirty field always equals canonical after rebase (InSync).
    #[test]
    fn i02_untouched_follows_canonical(a in "[a-z]{3,20}", b in "[a-z]{3,20}") {
        let av = Username::try_new(a).expect("valid");
        let bv = Username::try_new(b).expect("valid");
        let mut f = Field::from_base(av);
        prop_assert!(!f.is_dirty());
        f.rebase(bv.clone());
        prop_assert_eq!(f.value(), Some(&bv));
        prop_assert!(matches!(f.sync(), SyncState::InSync));
        prop_assert!(!f.is_dirty());
    }

    // I3 — a dirty field is never silently overwritten by rebase (yours preserved, Conflicted).
    #[test]
    fn i03_dirty_preserved_on_conflict(
        base in "[a-z]{3,20}", mine in "[a-z]{3,20}", theirs in "[a-z]{3,20}"
    ) {
        prop_assume!(mine != base);
        prop_assume!(theirs != mine);
        let basev = Username::try_new(base).expect("valid");
        let minev = Username::try_new(mine).expect("valid");
        let theirsv = Username::try_new(theirs).expect("valid");

        let mut f = Field::from_base(basev);
        f.try_set(minev.clone().into_raw()).expect("valid");
        prop_assert!(f.is_dirty());
        f.rebase(theirsv.clone());

        prop_assert_eq!(f.value(), Some(&minev)); // yours preserved
        match f.sync() {
            SyncState::Conflicted { theirs: t, .. } => prop_assert_eq!(t, &theirsv),
            other => prop_assert!(false, "expected Conflicted, got {:?}", other),
        }
    }

    // I4 — convergent rebase (yours == theirs) lands clean and InSync.
    #[test]
    fn i04_convergent_rebase_clean(base in "[a-z]{3,20}", edit in "[a-z]{3,20}") {
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

    // I5 — setting a field back to its base value clears dirty (revert-for-free).
    #[test]
    fn i05_revert_clears_dirty(base in "[a-z]{3,20}", edit in "[a-z]{3,20}") {
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
// I6–I12: example-based
// --------------------------------------------------------------------------------------------

// I6 — a failed try_set blocks submit; no stale valid value slips through.
#[test]
fn i06_failed_set_blocks_submit() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let handle = store.checkout();
    {
        let mut d = handle.borrow_mut();
        // last valid value is "alice"; now enter invalid text
        assert!(d.try_set_username("ab".to_string()).is_err()); // too short
        assert!(matches!(d.username.validity(), Validity::Invalid { .. }));
    }
    match store.submit(handle) {
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
}

// I7 — commit succeeds iff all fields Valid, none Conflicted, no rule violations; and the
// committed entity equals the field values.
#[test]
fn i07_commit_equivalence_and_entity_equals_fields() {
    // fully valid, no conflicts, Live -> Ok, entity equals the field values
    let mut d = ProfileDraft::from_canonical(None, 0);
    d.try_set_username("bob".to_string()).expect("valid");
    d.try_set_name("Bob".to_string()).expect("valid");
    d.try_set_email("bob@corp.example".to_string())
        .expect("valid");
    d.try_set_availability(Date::new(2026, 3, 1), Date::new(2026, 3, 10))
        .expect("valid");

    let expected = Profile {
        username: Username::try_new("bob".to_string()).expect("valid"),
        name: PersonName::try_new("Bob".to_string()).expect("valid"),
        email: Email::try_new("bob@corp.example".to_string()).expect("valid"),
        availability: DateRange::try_new((Date::new(2026, 3, 1), Date::new(2026, 3, 10)))
            .expect("valid"),
    };
    assert_eq!(d.commit(), Ok(expected));

    // an invalid/unset field -> Err
    let mut d2 = ProfileDraft::from_canonical(None, 0);
    d2.try_set_username("bob".to_string()).expect("valid"); // others left Unset
    assert!(d2.commit().is_err());

    // an unresolved conflict -> Err (none Conflicted)
    let base = base_profile();
    let mut d3 = ProfileDraft::from_canonical(Some(&base), 0);
    d3.try_set_username("carol".to_string()).expect("valid"); // dirty vs "alice"
    let mut other = base.clone();
    other.username = Username::try_new("dave".to_string()).expect("valid");
    d3.rebase(&other); // yours "carol" != theirs "dave" -> Conflicted
    assert!(d3.conflicts().contains(&ProfileField::Username));
    assert!(d3.commit().is_err());
}

// I8 — rebase re-runs tier-2 validation (the corporate_email rule flips when the rebased
// username changes, even though it pins to Email).
#[test]
fn i08_rebase_reruns_tier2_rule() {
    let base = base_profile(); // username "alice", email "alice@corp.example"
    let mut d = ProfileDraft::from_canonical(Some(&base), 0);

    // edit email to a dirty non-corp domain; rule still OK because username is "alice"
    d.try_set_email("bob@other.com".to_string()).expect("valid");
    assert!(!rule_present(&d.validate(), "corporate_email"));

    // canonical username becomes corp_* -> rebase adopts it (username was untouched)
    let mut updated = base.clone();
    updated.username = Username::try_new("corp_bob".to_string()).expect("valid");
    d.rebase(&updated);

    // now the rule fires: re-evaluated with the rebased username
    let report = d.validate();
    assert!(rule_present(&report, "corporate_email"));
    let v = report
        .rule_errors
        .iter()
        .find(|v| v.rule == "corporate_email")
        .expect("present");
    assert_eq!(v.pins, vec![ProfileField::Email]); // pinned to Email
}

// I9 — resolve_keep_mine: value=yours, base=theirs, dirty, InSync. resolve_take_theirs:
// value=theirs, clean, InSync.
#[test]
fn i09_resolution_semantics() {
    // keep-mine
    let mut f = Field::from_base(Username::try_new("alice".to_string()).expect("valid"));
    let mine = Username::try_new("mine1".to_string()).expect("valid");
    let theirs = Username::try_new("their1".to_string()).expect("valid");
    f.try_set(mine.clone().into_raw()).expect("valid");
    f.rebase(theirs.clone());
    assert!(matches!(f.sync(), SyncState::Conflicted { .. }));
    f.resolve_keep_mine();
    assert_eq!(f.value(), Some(&mine));
    assert_eq!(f.base(), Some(&theirs));
    assert!(f.is_dirty());
    assert!(matches!(f.sync(), SyncState::InSync));

    // take-theirs
    let mut g = Field::from_base(Username::try_new("alice".to_string()).expect("valid"));
    let mine2 = Username::try_new("mine2".to_string()).expect("valid");
    let theirs2 = Username::try_new("their2".to_string()).expect("valid");
    g.try_set(mine2.into_raw()).expect("valid");
    g.rebase(theirs2.clone());
    g.resolve_take_theirs();
    assert_eq!(g.value(), Some(&theirs2));
    assert!(!g.is_dirty());
    assert!(matches!(g.sync(), SyncState::InSync));
}

// I10 — stale async completions (old sequence) are ignored; the latest begin wins.
#[test]
fn i10_stale_async_ignored() {
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

// I11 — canonical deletion orphans the draft; submit on orphaned is a typed error.
#[test]
fn i11_delete_orphans_and_submit_is_typed() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    let handle = store.checkout();
    store.delete_canonical();
    assert_eq!(handle.borrow().status(), DraftStatus::Orphaned);
    match store.submit(handle) {
        Err(SubmitError::Orphaned) => {}
        other => panic!("expected Orphaned, got {other:?}"),
    }
    assert!(store.canonical().is_none());
}

// I12 — create-flow drafts (no base) never rebase and commit normally.
#[test]
fn i12_create_flow_never_rebases_and_commits() {
    let mut store: ProfileStore = Store::new(None);
    let handle = store.checkout(); // create-flow, not registered for rebase

    // someone else creates canonical; the create draft must NOT rebase
    store.apply_canonical(base_profile());
    {
        let d = handle.borrow();
        assert!(matches!(d.username.validity(), Validity::Unset));
        assert!(matches!(d.name.validity(), Validity::Unset));
        assert!(matches!(d.email.validity(), Validity::Unset));
        assert!(matches!(d.availability.validity(), Validity::Unset));
        assert!(d.dirty_fields().is_empty());
    }

    // fill and commit normally
    {
        let mut d = handle.borrow_mut();
        d.try_set_username("carol".to_string()).expect("valid");
        d.try_set_name("Carol".to_string()).expect("valid");
        d.try_set_email("carol@corp.example".to_string())
            .expect("valid");
        d.try_set_availability(Date::new(2026, 5, 1), Date::new(2026, 5, 2))
            .expect("valid");
    }
    store.submit(handle).expect("create-flow submit ok");
    assert_eq!(
        store.canonical().map(|p| p.username.as_str()),
        Some("carol")
    );
}
