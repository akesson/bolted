//! Feature-behaviour tests beyond the numbered invariants: sanitization, the composite value
//! object, constraint-metadata export, and a full checkout→edit→conflict→resolve→submit lifecycle.

use bolted_core::*;
use spike_profile::*;

fn base_profile() -> Profile {
    Profile {
        username: Username::try_new("alice".to_string()).expect("valid username"),
        name: PersonName::try_new("Alice".to_string()).expect("valid name"),
        email: Email::try_new("alice@corp.example".to_string()).expect("valid email"),
        availability: DateRange::try_new((Date::new(2026, 1, 1), Date::new(2026, 12, 31)))
            .expect("valid range"),
    }
}

#[test]
fn sanitization_trims_and_lowercases() {
    let u = Username::try_new("  Alice_1  ".to_string()).expect("valid");
    assert_eq!(u.as_str(), "Alice_1");

    let n = PersonName::try_new("  Bob  ".to_string()).expect("valid");
    assert_eq!(n.as_str(), "Bob");

    let e = Email::try_new("  Foo@BAR.COM  ".to_string()).expect("valid");
    assert_eq!(e.as_str(), "foo@bar.com");
    assert_eq!(e.domain(), "bar.com");
}

#[test]
fn empty_after_trim_is_rejected() {
    assert!(matches!(
        PersonName::try_new("   ".to_string()),
        Err(PersonNameError::TooShort { min: 1, actual: 0 })
    ));
    assert!(matches!(
        Username::try_new("  ".to_string()),
        Err(UsernameError::TooShort { .. })
    ));
}

#[test]
fn date_range_composite_grouped_setter() {
    // ordered -> valid; the grouped setter records one composite value
    let mut d = ProfileDraft::from_canonical(None, 0);
    d.try_set_availability(Date::new(2026, 1, 1), Date::new(2026, 1, 2))
        .expect("ordered");
    let dr = d.availability.value().expect("valid");
    assert_eq!(dr.start(), Date::new(2026, 1, 1));
    assert_eq!(dr.end(), Date::new(2026, 1, 2));

    // reversed -> error, recorded as Invalid (blocks submit)
    let mut d2 = ProfileDraft::from_canonical(None, 0);
    let err = d2
        .try_set_availability(Date::new(2026, 1, 2), Date::new(2026, 1, 1))
        .expect_err("reversed is invalid");
    assert!(matches!(err, DateRangeError::StartAfterEnd { .. }));
    assert!(matches!(
        d2.availability.validity(),
        Validity::Invalid { .. }
    ));
}

#[test]
fn constraint_metadata_exported() {
    // value-intrinsic constraints
    assert_eq!(
        Username::constraints().to_vec(),
        vec![
            Constraint::LenChars { min: 3, max: 20 },
            Constraint::Custom("ascii_alnum_underscore"),
        ]
    );

    // field-level export prepends Required, then the value's own constraints
    let c = ProfileField::Username.constraints();
    assert_eq!(c[0], Constraint::Required);
    assert!(c.contains(&Constraint::LenChars { min: 3, max: 20 }));

    // every profile field is required
    for f in [
        ProfileField::Username,
        ProfileField::Name,
        ProfileField::Email,
        ProfileField::Availability,
    ] {
        assert_eq!(f.constraints()[0], Constraint::Required);
    }
}

#[test]
fn corporate_email_rule_blocks_and_passes() {
    // corp_ username with wrong domain -> rule violation pinned to Email
    let mut d = ProfileDraft::from_canonical(None, 0);
    d.try_set_username("corp_x".to_string()).expect("valid");
    d.try_set_name("X".to_string()).expect("valid");
    d.try_set_email("x@other.com".to_string()).expect("valid");
    d.try_set_availability(Date::new(2026, 1, 1), Date::new(2026, 1, 2))
        .expect("valid");
    let report = d.validate();
    assert!(
        report
            .rule_errors
            .iter()
            .any(|v| v.rule == "corporate_email")
    );

    // right domain -> no violation. The username is a fresh one nobody has checked, so C16 still
    // blocks the commit until the uniqueness check passes.
    d.try_set_email("x@corp.example".to_string())
        .expect("valid");
    assert!(rule_present(&d.validate(), "username_unique"));
    let token = d.begin_username_check();
    assert!(d.complete_username_check(token, Ok(())));
    assert!(d.validate().is_ok());
    assert!(d.commit().is_ok());
}

fn rule_present(report: &ValidationReport<ProfileField>, rule: &str) -> bool {
    report.rule_errors.iter().any(|v| v.rule == rule)
}

#[test]
fn pending_username_check_blocks_then_passes() {
    let mut d = ProfileDraft::from_canonical(None, 0);
    d.try_set_username("zoe".to_string()).expect("valid");
    d.try_set_name("Zoe".to_string()).expect("valid");
    d.try_set_email("zoe@corp.example".to_string())
        .expect("valid");
    d.try_set_availability(Date::new(2026, 1, 1), Date::new(2026, 1, 2))
        .expect("valid");

    // a pending check blocks validation
    let token = d.begin_username_check();
    assert!(
        d.validate()
            .rule_errors
            .iter()
            .any(|v| v.rule == "username_unique")
    );

    // completing it OK unblocks
    assert!(d.complete_username_check(token, Ok(())));
    assert!(d.validate().is_ok());

    // a failed check blocks again (as a rule error carrying the check's ErrorData)
    let token2 = d.begin_username_check();
    assert!(d.complete_username_check(token2, Err(ErrorData::new("username_taken"))));
    let report = d.validate();
    let v = report
        .rule_errors
        .iter()
        .find(|v| v.rule == "username_unique")
        .expect("blocked");
    assert_eq!(v.error.key, "username_taken");
}

#[test]
fn full_lifecycle_checkout_edit_conflict_resolve_submit() {
    let base = base_profile(); // username "alice"
    let mut store: ProfileStore = Store::new(Some(base.clone()));
    let mut handle = store.checkout();

    // edit username
    {
        let mut d = handle.borrow_mut().expect("live");
        d.try_set_username("alice2".to_string()).expect("valid");
        assert!(d.dirty_fields().contains(&ProfileField::Username));
    }

    // background canonical change to a different username -> conflict on that field only
    let mut background = base.clone();
    background.username = Username::try_new("admin".to_string()).expect("valid");
    store.apply_canonical(background);
    {
        let d = handle.borrow().expect("live");
        assert_eq!(d.conflicts(), vec![ProfileField::Username]);
    }

    // resolve keep-mine, check the kept username, then submit
    {
        let mut d = handle.borrow_mut().expect("live");
        d.resolve_keep_mine(ProfileField::Username);
        assert!(d.conflicts().is_empty());
        // keep-mine leaves the value where it was, so the verdict (had there been one) would
        // stand — but there never was one, and the field is dirty, so C16 demands a check.
        let token = d.begin_username_check();
        assert!(d.complete_username_check(token, Ok(())));
    }
    store.submit(&mut handle).expect("submit ok after resolve");
    assert!(!handle.is_live()); // C17: a successful submit tombstones the handle

    // canonical now carries our value
    assert_eq!(
        store.canonical().map(|p| p.username.as_str()),
        Some("alice2")
    );
    assert_eq!(store.version(), 2); // apply_canonical + submit
}

#[test]
fn base_version_recorded_at_checkout() {
    let mut store: ProfileStore = Store::new(Some(base_profile()));
    store.apply_canonical(base_profile()); // version -> 1
    let handle = store.checkout();
    assert_eq!(handle.borrow().expect("live").base_version(), 1);
}
