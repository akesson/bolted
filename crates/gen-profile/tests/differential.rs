//! The generated feature against the hand-written one, driven through the same script.
//!
//! ROADMAP's Phase 3 says *"the hand-written spike code becomes the golden reference the generated
//! code is diffed against"*. A textual diff cannot do that job — `gen-profile` is 130 lines and
//! `fixture-profile` is 724, and the differences that matter are not the ones a diff shows. So the two
//! features are driven through identical inputs and their **observable answers** are compared:
//! parsed values, error keys, constraint metadata, dirty sets, conflict sets, validation reports,
//! and commit outcomes.
//!
//! The types are foreign to each other — `gen_profile::ProfileField` and `fixture_profile::ProfileField`
//! are different enums — so everything is projected to data before comparison. That projection is
//! also the point: two independent implementations of one contract agree on what a shell would see.
//!
//! Panics are the product here, as in any test.

use bolted_core::{Checked, Draft, ErrorData, StoreDraft, Value};

// =================================================================================================
// projections — the shape a shell would observe, from either side
// =================================================================================================

/// A parse outcome as data: the canonical raw on success, the keyed error on failure.
fn parse<V: Value<Raw = String>>(raw: &str) -> Result<String, ErrorData> {
    V::try_new(raw.to_string())
        .map(Value::into_raw)
        .map_err(Into::into)
}

fn ids<Id: std::fmt::Debug>(list: Vec<Id>) -> Vec<String> {
    list.iter().map(|f| format!("{f:?}")).collect()
}

/// `(field_errors, rule_errors)`, with field ids reduced to their names.
type Report = (
    Vec<(String, ErrorData)>,
    Vec<(String, Vec<String>, ErrorData)>,
);

fn report<D: Draft>(draft: &D) -> Report {
    let r = draft.validate();
    (
        r.field_errors
            .into_iter()
            .map(|(f, e)| (format!("{f:?}"), e))
            .collect(),
        r.rule_errors
            .into_iter()
            .map(|v| (v.rule.to_string(), ids(v.pins), v.error))
            .collect(),
    )
}

/// Everything a shell can see about a draft, as comparable data.
fn observe<D: Draft>(draft: &D) -> (Vec<String>, Vec<String>, Report, u64, String) {
    (
        ids(draft.dirty_fields()),
        ids(draft.conflicts()),
        report(draft),
        draft.base_version(),
        format!("{:?}", draft.status()),
    )
}

// =================================================================================================
// tier 1 — the value types
// =================================================================================================

/// The raws below include forms each type *sanitizes*, forms it rejects, and forms it accepts. A
/// generated `try_new` that trimmed but forgot to lowercase, or that swapped `<` for `<=`, shows up
/// here as a differing `Ok` or a differing key.
const USERNAMES: &[&str] = &[
    "",
    "ab",
    "abc",
    "  alice  ",
    "corp_bob",
    "Not Valid!",
    "a-b",
];
const NAMES: &[&str] = &["", "   ", "A", "  Alice  ", "Alice Anderson"];
const EMAILS: &[&str] = &[
    "",
    "not-an-email",
    "@nodomain",
    "nolocal@",
    "a@b",
    "  ALICE@CORP.EXAMPLE  ",
];

#[test]
fn the_generated_value_types_parse_exactly_as_the_hand_written_ones_do() {
    for raw in USERNAMES {
        assert_eq!(
            parse::<gen_profile::Username>(raw),
            parse::<fixture_profile::Username>(raw),
            "Username::try_new({raw:?})"
        );
    }
    for raw in NAMES {
        assert_eq!(
            parse::<gen_profile::PersonName>(raw),
            parse::<fixture_profile::PersonName>(raw),
            "PersonName::try_new({raw:?})"
        );
    }
    for raw in EMAILS {
        assert_eq!(
            parse::<gen_profile::Email>(raw),
            parse::<fixture_profile::Email>(raw),
            "Email::try_new({raw:?})"
        );
    }
}

/// A too-long username is 21 characters, not 20. The DSL's bound is inclusive on both ends, and this
/// is the only test that says so against an independent implementation.
#[test]
fn the_length_bounds_are_inclusive_on_both_ends() {
    for n in [2usize, 3, 20, 21] {
        let raw = "a".repeat(n);
        assert_eq!(
            parse::<gen_profile::Username>(&raw),
            parse::<fixture_profile::Username>(&raw),
            "Username of length {n}"
        );
    }
    for n in [0usize, 1, 30, 31] {
        let raw = "a".repeat(n);
        assert_eq!(
            parse::<gen_profile::PersonName>(&raw),
            parse::<fixture_profile::PersonName>(&raw),
            "PersonName of length {n}"
        );
    }
}

/// Constraint metadata is exported to shells and never re-checked by any invariant, so it is exactly
/// the sort of thing a generator quietly gets wrong. `CLAUDE.md` forbids constraint literals in shell
/// code precisely because this list is the single source of truth.
#[test]
fn the_generated_constraints_match_the_hand_written_ones() {
    use fixture_profile::ProfileField as S;
    use gen_profile::ProfileField as G;

    let pairs: [(Vec<_>, Vec<_>); 4] = [
        (G::Username.constraints(), S::Username.constraints()),
        (G::Name.constraints(), S::Name.constraints()),
        (G::Email.constraints(), S::Email.constraints()),
        (G::Availability.constraints(), S::Availability.constraints()),
    ];
    for (generated, hand_written) in pairs {
        assert_eq!(generated, hand_written);
    }
}

// =================================================================================================
// the feature — one script, two implementations
// =================================================================================================

fn gen_entity() -> gen_profile::Profile {
    gen_profile::Profile {
        username: value("alice"),
        name: value("Alice"),
        email: value("alice@corp.example"),
        availability: Value::try_new((
            gen_profile::Date::new(2026, 1, 1),
            gen_profile::Date::new(2026, 12, 31),
        ))
        .expect("valid"),
    }
}

fn spike_entity() -> fixture_profile::Profile {
    fixture_profile::Profile {
        username: value("alice"),
        name: value("Alice"),
        email: value("alice@corp.example"),
        availability: Value::try_new((
            fixture_profile::Date::new(2026, 1, 1),
            fixture_profile::Date::new(2026, 12, 31),
        ))
        .expect("valid"),
    }
}

fn value<V: Value<Raw = String>>(raw: &str) -> V {
    V::try_new(raw.to_string()).expect("a test constant must be valid")
}

/// Drive both drafts through the same edits and assert they agree at **every** step, not only at the
/// end. An implementation that reaches the right final state through a wrong intermediate one — a
/// verdict that resets a beat late, a conflict that appears and then heals — is still wrong, and a
/// shell rendering each step would show it.
#[test]
fn the_two_implementations_agree_at_every_step_of_an_edit_session() {
    let (ge, se) = (gen_entity(), spike_entity());
    let mut g = gen_profile::ProfileDraft::from_canonical(Some(&ge), 0);
    let mut s = fixture_profile::ProfileDraft::from_canonical(Some(&se), 0);

    let mut step = 0;
    let mut agree = |g: &gen_profile::ProfileDraft, s: &fixture_profile::ProfileDraft| {
        assert_eq!(observe(g), observe(s), "the two diverged at step {step}");
        step += 1;
    };
    agree(&g, &s);

    // 1. an invalid name: tier 1 reports it, and the previous valid value is not silently kept (C06)
    assert!(g.try_set_name("   ".into()).is_err());
    assert!(s.try_set_name("   ".into()).is_err());
    agree(&g, &s);

    // 2. repaired, and now dirty
    g.try_set_name("My Name".into()).expect("valid");
    s.try_set_name("My Name".into()).expect("valid");
    agree(&g, &s);

    // 3. a corp_ username against a non-corp email: the tier-2 rule fires, pinned to `email`
    g.try_set_username("corp_bob".into()).expect("valid");
    s.try_set_username("corp_bob".into()).expect("valid");
    g.try_set_email("bob@other.com".into()).expect("valid");
    s.try_set_email("bob@other.com".into()).expect("valid");
    agree(&g, &s);

    // 4. the unrun uniqueness check blocks the dirty username (C16)
    assert!(!g.validate().is_ok());
    assert!(!s.validate().is_ok());

    // 5. pass the check on both surfaces: `Checked` (generated) and the inherent delegate (spike)
    let check = gen_profile::ProfileCheck::UsernameUnique;
    let gt = g.begin_check(check);
    assert!(g.complete_check(check, gt, Ok(())));
    let st = s.begin_username_check();
    assert!(s.complete_username_check(st, Ok(())));
    agree(&g, &s);

    // 6. the server moves `email` only. The rule's verdict changes; `name` must NOT conflict, because
    //    its own canonical never moved (C19/D14) — the defect step 07 found in the frozen core.
    let ge2 = gen_profile::Profile {
        email: value("bob@corp.example"),
        ..ge.clone()
    };
    let se2 = fixture_profile::Profile {
        email: value("bob@corp.example"),
        ..se.clone()
    };
    g.rebase(&ge2, 1);
    s.rebase(&se2, 1);
    agree(&g, &s);

    // 7. the server moves the username under a dirty, checked field: conflict, verdict stands (C13d)
    let ge3 = gen_profile::Profile {
        username: value("carol"),
        ..ge2.clone()
    };
    let se3 = fixture_profile::Profile {
        username: value("carol"),
        ..se2.clone()
    };
    g.rebase(&ge3, 2);
    s.rebase(&se3, 2);
    agree(&g, &s);

    // 8. take theirs: the value moves, so the verdict resets and the check is demanded again (C09/C13)
    g.resolve_take_theirs(gen_profile::ProfileField::Username);
    s.resolve_take_theirs(fixture_profile::ProfileField::Username);
    agree(&g, &s);

    // 9. both refuse to commit, for the same typed reason and with the same report
    let g_err = g.commit().map(|_| ()).map_err(|(_, e)| format!("{e:?}"));
    let s_err = s.commit().map(|_| ()).map_err(|(_, e)| format!("{e:?}"));
    assert_eq!(g_err, s_err);
    assert!(
        g_err.is_err(),
        "the session ends refused, or step 9 proves nothing"
    );
}

/// C11 + C12, on both, from the same starting point: an orphan is terminal and a create-flow draft is
/// never moved. `is_based` is *generated* now, so this is a differential check on the property whose
/// single-field version passed 21 of 22 invariants in step 08.
#[test]
fn orphaning_and_create_flow_agree() {
    let (ge, se) = (gen_entity(), spike_entity());

    let mut g = gen_profile::ProfileDraft::from_canonical(Some(&ge), 0);
    let mut s = fixture_profile::ProfileDraft::from_canonical(Some(&se), 0);
    assert!(g.is_based() && s.is_based());
    g.orphan();
    s.orphan();
    g.rebase(&ge, 9); // terminal: a rebase after an orphan must not move it
    s.rebase(&se, 9);
    assert_eq!(observe(&g), observe(&s));

    let g = gen_profile::ProfileDraft::from_canonical(None, 0);
    let s = fixture_profile::ProfileDraft::from_canonical(None, 0);
    assert!(!g.is_based() && !s.is_based());
    assert_eq!(observe(&g), observe(&s));
}

/// C20/C21: the stash is raw data, and a restored draft is identical on both sides — including that
/// the async verdict did **not** survive it.
#[test]
fn the_stash_round_trips_identically() {
    use bolted_core::Stashable;

    let (ge, se) = (gen_entity(), spike_entity());
    let mut g = gen_profile::ProfileDraft::from_canonical(Some(&ge), 3);
    let mut s = fixture_profile::ProfileDraft::from_canonical(Some(&se), 3);

    g.try_set_username("alice2".into()).expect("valid");
    s.try_set_username("alice2".into()).expect("valid");
    assert!(g.try_set_email("nope".into()).is_err()); // an Invalid { raw } must survive the stash
    assert!(s.try_set_email("nope".into()).is_err());

    let check = gen_profile::ProfileCheck::UsernameUnique;
    let gt = g.begin_check(check);
    assert!(g.complete_check(check, gt, Ok(())));
    let st = s.begin_username_check();
    assert!(s.complete_username_check(st, Ok(())));

    let (g, s) = (
        gen_profile::ProfileDraft::from_stash(&g.stash()),
        fixture_profile::ProfileDraft::from_stash(&s.stash()),
    );
    assert_eq!(observe(&g), observe(&s));
    // and the verdict is gone on both, so C16 demands a fresh check on the dirty username
    assert!(
        report(&g).1.iter().any(|(r, ..)| r == "username_unique"),
        "a restored draft's checked field must be unchecked"
    );
}
