//! The generated wrapper, run.
//!
//! `mise run check` proves the FFI layer *is* what the declaration says. This proves the FFI layer
//! *works* — without an emulator, a simulator, or a packed artifact, because `#[export]` leaves the
//! Rust types perfectly ordinary.
//!
//! The Swift and Kotlin suites are still the real evidence (they exercise the generated *bindings*).
//! These tests exist because D23 is new behaviour, and new behaviour with no test is a claim.

use bolted_ffi::{CheckStateFfi, CheckVerdictFfi, DraftClosedFfi, TextFieldSync, TextValidity};
use gen_profile_ffi::custom::{AvailabilityRaw, PlainDate};
use gen_profile_ffi::generated::*;

fn date(year: u16, month: u8, day: u8) -> PlainDate {
    PlainDate { year, month, day }
}

fn values(username: &str) -> ProfileValues {
    ProfileValues {
        username: username.to_owned(),
        name: "Ada".to_owned(),
        email: "ada@corp.example".to_owned(),
        availability: AvailabilityRaw {
            start: date(2026, 1, 1),
            end: date(2026, 12, 31),
        },
    }
}

fn seeded() -> ProfileStoreFfi {
    let store = ProfileStoreFfi::new();
    store.apply_canonical(values("ada")).expect("valid seed");
    store
}

/// Answers from a script, and records every value it was asked about.
struct Scripted {
    verdict: CheckVerdictFfi,
    seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Scripted {
    fn new(verdict: CheckVerdictFfi) -> (Box<Self>, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Box::new(Scripted {
                verdict,
                seen: seen.clone(),
            }),
            seen,
        )
    }
}

impl UsernameChecker for Scripted {
    fn check(&self, value: String) -> CheckVerdictFfi {
        self.seen.lock().expect("not poisoned").push(value);
        self.verdict
    }
}

// =================================================================================================
// D23 — the typed refusal
// =================================================================================================

/// C17 releases the draft; the foreign handle survives it. Before this step every mutator then took
/// the `draft_mut(id) → None` branch and returned `Ok(())`, silently doing nothing.
#[test]
fn every_mutator_refuses_a_submitted_draft_instead_of_lying_about_it() {
    let store = seeded();
    let draft = store.checkout();
    draft.try_set_name("Grace".to_owned()).expect("live draft");
    draft.submit().expect("valid");

    assert!(
        !draft.is_live(),
        "C17: a successful submit releases the draft"
    );

    assert_eq!(
        draft.try_set_username("x".to_owned()),
        Err(UsernameErrorFfi::DraftClosed)
    );
    assert_eq!(
        draft.try_set_name("x".to_owned()),
        Err(PersonNameErrorFfi::DraftClosed)
    );
    assert_eq!(
        draft.try_set_email("x@y.z".to_owned()),
        Err(EmailErrorFfi::DraftClosed)
    );
    assert_eq!(
        draft.try_set_availability(AvailabilityRaw {
            start: date(2026, 1, 1),
            end: date(2026, 2, 1)
        }),
        Err(gen_profile_ffi::custom::AvailabilityErrorFfi::DraftClosed),
        "the composite's hand-written error type must carry DraftClosed too"
    );
    assert_eq!(
        draft.resolve_keep_mine(ProfileFieldId::Username),
        Err(DraftClosedFfi::DraftClosed)
    );
    assert_eq!(
        draft.resolve_take_theirs(ProfileFieldId::Email),
        Err(DraftClosedFfi::DraftClosed)
    );
}

/// ...and the observers stay total. A shell calls `validate()` on every keystroke and `is_live()` to
/// ask the question the mutators now answer with an error.
#[test]
fn the_observers_of_a_submitted_draft_do_not_throw() {
    let store = seeded();
    let draft = store.checkout();
    draft
        .submit()
        .expect("a clean checkout of a valid canonical commits");

    assert!(!draft.is_live());
    let report = draft.validate();
    assert!(report.field_errors.is_empty() && report.rule_errors.is_empty());
    let snapshot = draft.snapshot();
    assert_eq!(snapshot.username.validity, TextValidity::Unset);
    let _ = draft.stash();
}

/// `run_*_check` distinguishes "no checker installed" from "the draft is gone". `spike-profile-ffi`
/// returned `false` for both.
#[test]
fn running_a_check_without_a_checker_is_not_the_same_as_running_it_on_a_corpse() {
    let store = seeded();
    let draft = store.checkout();
    assert_eq!(
        draft.run_username_check(),
        Ok(false),
        "no checker installed"
    );

    draft.set_username_checker(Scripted::new(CheckVerdictFfi::Pass).0);
    draft.submit().expect("valid");
    assert_eq!(draft.run_username_check(), Err(DraftClosedFfi::DraftClosed));
}

// =================================================================================================
// The generated check driver
// =================================================================================================

#[test]
fn a_failed_check_raises_the_declared_key_and_blocks_the_submit() {
    let store = seeded();
    let draft = store.checkout();
    draft
        .try_set_username("taken".to_owned())
        .expect("valid username");
    draft.set_username_checker(Scripted::new(CheckVerdictFfi::Fail).0);

    assert_eq!(draft.run_username_check(), Ok(true));

    // `failed_key = "username_taken"` is declared in gen-profile, not invented here or in Swift.
    match draft.snapshot().username_check {
        CheckStateFfi::Failed { error } => assert_eq!(error.key, "username_taken"),
        other => panic!("expected a failed verdict, got {other:?}"),
    }

    let report = draft.validate();
    assert!(
        report
            .rule_errors
            .iter()
            .any(|v| v.rule == "username_unique"),
        "C13: a failed verdict is a rule violation pinned to the checked field"
    );
    assert!(
        draft.submit().is_err(),
        "C07: validation refuses the commit"
    );
}

#[test]
fn the_checker_is_asked_about_the_value_it_will_be_bound_to() {
    let store = seeded();
    let draft = store.checkout();
    draft
        .try_set_username("  Grace  ".to_owned())
        .expect("sanitized to `Grace`");
    let (checker, seen) = Scripted::new(CheckVerdictFfi::Pass);
    draft.set_username_checker(checker);
    assert_eq!(draft.run_username_check(), Ok(true));

    // The sanitizer ran first (D9's echo rule lives above this), so the checker sees the *parsed*
    // value, not the raw keystrokes.
    assert_eq!(seen.lock().expect("not poisoned").as_slice(), ["Grace"]);
    assert_eq!(draft.snapshot().username_check, CheckStateFfi::Passed);
}

/// C13, generated: moving the checked field's value discards the verdict it was bound to.
#[test]
fn a_verdict_does_not_survive_the_value_that_earned_it() {
    let store = seeded();
    let draft = store.checkout();
    draft.try_set_username("grace".to_owned()).expect("valid");
    draft.set_username_checker(Scripted::new(CheckVerdictFfi::Pass).0);
    assert_eq!(draft.run_username_check(), Ok(true));
    assert_eq!(draft.snapshot().username_check, CheckStateFfi::Passed);

    draft.try_set_username("hopper".to_owned()).expect("valid");
    assert_eq!(
        draft.snapshot().username_check,
        CheckStateFfi::Unchecked,
        "the guard must fire on the checked field's own setter"
    );

    // ...and a setter for an unchecked field must NOT reset it (step 09, headline 3).
    draft.try_set_username("grace".to_owned()).expect("valid");
    assert_eq!(draft.run_username_check(), Ok(true));
    draft
        .try_set_name("Grace Hopper".to_owned())
        .expect("valid");
    assert_eq!(
        draft.snapshot().username_check,
        CheckStateFfi::Passed,
        "typing in the name box must not invalidate the username's verdict"
    );
}

// =================================================================================================
// The store discipline (D16), generated
// =================================================================================================

/// Step 02 called this the wrapper's hardest-won invariant: **never call a foreign callback while
/// holding the store lock.** A Swift checker that touches the store reentrantly would deadlock.
///
/// No `unsafe`, no raw pointers: `ProfileStoreFfi` shares the very `Mutex` the check driver takes, so
/// a checker holding the store probes exactly the hazard. If phase B held the lock, this test hangs.
#[test]
fn a_reentrant_checker_does_not_deadlock() {
    use std::sync::Arc;

    struct Nosy(Arc<ProfileStoreFfi>);
    impl UsernameChecker for Nosy {
        fn check(&self, _value: String) -> CheckVerdictFfi {
            // Both take the store lock the driver must have dropped by now.
            let _ = self.0.live_draft_count();
            let _ = self.0.canonical();
            CheckVerdictFfi::Pass
        }
    }

    let store = Arc::new(seeded());
    let draft = store.checkout();
    draft.try_set_username("grace".to_owned()).expect("valid");
    draft.set_username_checker(Box::new(Nosy(Arc::clone(&store))));

    assert_eq!(draft.run_username_check(), Ok(true));
    assert_eq!(draft.snapshot().username_check, CheckStateFfi::Passed);
}

/// C22, generated: "a draft exists" and "a draft rebases" are different questions.
#[test]
fn the_two_draft_counts_answer_different_questions() {
    let store = ProfileStoreFfi::new();
    let create_flow = store.checkout(); // no canonical yet
    assert_eq!(store.live_draft_count(), 1);
    assert_eq!(
        store.rebasing_draft_count(),
        0,
        "C12: a create-flow draft is not rebased"
    );

    store.apply_canonical(values("ada")).expect("valid");
    let editing = store.checkout();
    assert_eq!(store.live_draft_count(), 2);
    assert_eq!(store.rebasing_draft_count(), 1);

    drop(editing);
    assert_eq!(store.live_draft_count(), 1, "C18: Drop closes the draft");
    drop(create_flow);
    assert_eq!(store.live_draft_count(), 0);
}

/// Metadata, not state: a shell derives `maxLength` from this and never writes a `30`.
///
/// `Required` leads every list — D13's judgement, made by `#[bolted::entity]` and not restated here.
#[test]
fn the_declared_constraints_cross_the_boundary() {
    use bolted_ffi::ConstraintFfi;
    let store = ProfileStoreFfi::new();
    assert_eq!(
        store.constraints(ProfileFieldId::Username),
        vec![
            ConstraintFfi::Required,
            ConstraintFfi::LenChars { min: 3, max: 20 },
            ConstraintFfi::Custom {
                key: "ascii_alnum_underscore".to_owned()
            },
        ]
    );
    assert_eq!(
        store.constraints(ProfileFieldId::Name),
        vec![
            ConstraintFfi::Required,
            ConstraintFfi::LenChars { min: 1, max: 30 }
        ]
    );
}

// =================================================================================================
// The projection itself
//
// Everything above tests the *wrapper's* behaviour. These test what the snapshot SAYS — and they
// exist because a mutation pass found the wrapper suite blind to all of it. `bolted-conformance`
// covers the core; nothing covered the boundary between the core and the wire.
// =================================================================================================

/// M12 / M2. Dirty is per-field and in aggregate, and both cross.
#[test]
fn the_snapshot_reports_which_fields_are_dirty() {
    let store = seeded();
    let draft = store.checkout();
    assert!(!draft.snapshot().any_dirty);

    draft.try_set_name("Grace".to_owned()).expect("valid");
    let snapshot = draft.snapshot();
    assert!(snapshot.any_dirty);
    assert!(snapshot.name.dirty);
    assert!(
        !snapshot.username.dirty,
        "only the field that moved is dirty"
    );

    // D11: dirty is value-based, so typing a value back is not dirty.
    draft.try_set_name("Ada".to_owned()).expect("valid");
    assert!(!draft.snapshot().any_dirty);
    assert!(!draft.snapshot().name.dirty);
}

/// M1 / M3. A conflict names the field that conflicted, and the list is in declaration order.
#[test]
fn conflicts_cross_with_the_right_field_ids_in_declaration_order() {
    let store = seeded();
    let draft = store.checkout();
    draft.try_set_username("mine".to_owned()).expect("valid");
    draft
        .try_set_email("mine@corp.example".to_owned())
        .expect("valid");

    // The server moves both fields the draft has touched.
    let mut theirs = values("theirs");
    theirs.email = "theirs@corp.example".to_owned();
    store.apply_canonical(theirs).expect("valid");

    let snapshot = draft.snapshot();
    assert_eq!(
        snapshot.conflicts,
        vec![ProfileFieldId::Username, ProfileFieldId::Email],
        "declaration order: a shell walks this list to focus the first conflict"
    );
    assert!(matches!(
        snapshot.username.sync,
        TextFieldSync::Conflicted { .. }
    ));
    assert!(
        matches!(snapshot.name.sync, TextFieldSync::InSync),
        "name never moved"
    );
}

/// M9. The two resolvers are not the same function.
#[test]
fn keep_mine_and_take_theirs_reach_different_outcomes() {
    let store = seeded();
    let draft = store.checkout();
    draft.try_set_username("mine".to_owned()).expect("valid");
    draft
        .try_set_email("mine@corp.example".to_owned())
        .expect("valid");

    let mut theirs = values("theirs");
    theirs.email = "theirs@corp.example".to_owned();
    store.apply_canonical(theirs).expect("valid");

    draft
        .resolve_keep_mine(ProfileFieldId::Username)
        .expect("live");
    draft
        .resolve_take_theirs(ProfileFieldId::Email)
        .expect("live");

    let snapshot = draft.snapshot();
    assert!(snapshot.conflicts.is_empty());
    assert_eq!(
        snapshot.username.validity,
        TextValidity::Valid {
            value: "mine".to_owned()
        },
        "keep mine keeps mine"
    );
    assert_eq!(
        snapshot.email.validity,
        TextValidity::Valid {
            value: "theirs@corp.example".to_owned()
        },
        "take theirs takes theirs"
    );
}

/// M14, and §9's *"a real `Pending` across FFI"*.
///
/// With a synchronous checker, `begin` and `complete` are atomic inside one call, so a `snapshot()`
/// taken *after* `run_username_check` returns can never be `Pending`. It is observable from exactly
/// one place: the draft's stream, which the driver pushes to between the two halves. That is what the
/// split `begin`/`complete` buys (D10), and it is why a spinner is not a fiction.
#[test]
fn a_check_in_flight_is_observably_pending() {
    let store = seeded();
    let draft = store.checkout();
    draft.try_set_username("grace".to_owned()).expect("valid");

    let subscription = draft.snapshots();
    draft.set_username_checker(Scripted::new(CheckVerdictFfi::Pass).0);
    assert_eq!(draft.run_username_check(), Ok(true));

    let states: Vec<CheckStateFfi> = std::iter::from_fn(|| subscription.pop_event())
        .map(|snapshot: ProfileSnapshot| snapshot.username_check)
        .collect();

    assert_eq!(
        states,
        vec![CheckStateFfi::Pending, CheckStateFfi::Passed],
        "the driver must emit Pending before it calls out, or a shell has nothing to \
         hang a spinner on"
    );
    // ...and after the fact, only the verdict remains. §9 asked whether `Pending` ever reaches a
    // `snapshot()` caller. With a synchronous checker: no. It reaches a subscriber.
    assert_eq!(draft.snapshot().username_check, CheckStateFfi::Passed);
}
