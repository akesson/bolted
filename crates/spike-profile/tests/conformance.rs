//! `spike-profile` proves it is a Bolted feature.
//!
//! Everything below is *fixture*: the suite itself lives in `bolted-conformance`, generic over these
//! traits, and `docs/CONFORMANCE.md` states each `CNN` normatively. Until step 08 the 22 invariants
//! were written out longhand here, against `ProfileDraft` by name.
//!
//! Roles, not names. The suite edits a **primary** text field and moves a **secondary** one on the
//! server; it never learns that they are called `name` and `email`. The **checked** field is
//! `username`, and the async-check invariants (C10, C13, C16) come with `AsyncCheckFeature` — a
//! feature without a uniqueness check does not owe them. Likewise C08 arrives with `RuleFeature`.

use bolted_conformance::{AsyncCheckFeature, ConformanceFeature, RuleFeature, ValueFixture};
use bolted_core::{CheckState, CheckToken, Draft, ErrorData, Field, Value};
use proptest::prelude::*;
use proptest::strategy::{BoxedStrategy, Strategy};
use spike_profile::{
    Date, DateRange, Email, PersonName, Profile, ProfileDraft, ProfileField, ProfileStash, Username,
};

// =================================================================================================
// Value fixtures — one per value type. `any_raw` deliberately includes forms the type SANITIZES:
// C01's roundtrip is only interesting when `into_raw` has to return the canonical form rather than
// the form that was typed.
//
// A fixture is a marker type, not the value type itself: `impl ValueFixture for Username` cannot be
// written here, because both the trait and `Username` are foreign to this test crate (orphan rule).
// =================================================================================================

pub struct UsernameFixture;
impl ValueFixture for UsernameFixture {
    type Value = Username;
    fn any_raw() -> BoxedStrategy<String> {
        prop_oneof![".*", "[a-z0-9_]{3,20}", "  [a-z]{3,20}  "].boxed() // trimmed
    }
    fn valid_raw() -> BoxedStrategy<String> {
        "[a-z]{3,20}".boxed()
    }
    fn invalid_raw() -> String {
        "ab".to_string() // too short
    }
}

pub struct PersonNameFixture;
impl ValueFixture for PersonNameFixture {
    type Value = PersonName;
    fn any_raw() -> BoxedStrategy<String> {
        prop_oneof![".*", "[A-Za-z ]{1,30}", "   [A-Za-z]{1,20}   "].boxed()
    }
    fn valid_raw() -> BoxedStrategy<String> {
        "[A-Za-z]{1,30}".boxed()
    }
    fn invalid_raw() -> String {
        "   ".to_string() // trims to empty
    }
}

pub struct EmailFixture;
impl ValueFixture for EmailFixture {
    type Value = Email;
    fn any_raw() -> BoxedStrategy<String> {
        prop_oneof![
            ".*",
            "[a-z]{1,8}@[a-z]{1,8}\\.example",
            "  [A-Z]{1,8}@[A-Z]{1,8}\\.EXAMPLE  " // trimmed AND lowercased
        ]
        .boxed()
    }
    fn valid_raw() -> BoxedStrategy<String> {
        "[a-z]{1,8}@[a-z]{1,8}\\.example".boxed()
    }
    fn invalid_raw() -> String {
        "not-an-email".to_string() // no '@'
    }
}

pub struct DateRangeFixture;
impl ValueFixture for DateRangeFixture {
    type Value = DateRange;
    fn any_raw() -> BoxedStrategy<(Date, Date)> {
        (date(), date()).boxed() // half of these are start > end
    }
    fn valid_raw() -> BoxedStrategy<(Date, Date)> {
        (date(), date())
            .prop_map(|(a, b)| if a <= b { (a, b) } else { (b, a) })
            .boxed()
    }
    fn invalid_raw() -> (Date, Date) {
        (Date::new(2026, 12, 31), Date::new(2026, 1, 1)) // start after end
    }
}

fn date() -> impl Strategy<Value = Date> {
    (1970u16..2100, 1u8..=12, 1u8..=28).prop_map(|(y, m, d)| Date::new(y, m, d))
}

// C01–C06, C09, C14, C19, C20 — once per value type. Four types × ten invariants; before the
// extraction, `Username` carried all of them alone.
bolted_conformance::field_suite!(username, UsernameFixture);
bolted_conformance::field_suite!(person_name, PersonNameFixture);
bolted_conformance::field_suite!(email, EmailFixture);
bolted_conformance::field_suite!(date_range, DateRangeFixture);

// =================================================================================================
// The feature fixture
// =================================================================================================

pub struct ProfileFixture;

fn username(raw: &str) -> Username {
    Username::try_new(raw.to_string()).expect("a fixture constant must be valid")
}
fn person_name(raw: &str) -> PersonName {
    PersonName::try_new(raw.to_string()).expect("a fixture constant must be valid")
}
fn email(raw: &str) -> Email {
    Email::try_new(raw.to_string()).expect("a fixture constant must be valid")
}

impl ConformanceFeature for ProfileFixture {
    type Entity = Profile;
    type Draft = ProfileDraft;
    type Primary = PersonName;
    type Secondary = Email;

    const PRIMARY_BASE: &'static str = "Alice";
    const PRIMARY_MINE: &'static str = "My Name";
    const PRIMARY_THEIRS: &'static str = "Their Name";
    const PRIMARY_OTHER: &'static str = "Other Name";
    const PRIMARY_INVALID: &'static str = "   ";
    const SECONDARY_BASE: &'static str = "alice@corp.example";
    const SECONDARY_THEIRS: &'static str = "mine@other.com";
    const SECONDARY_INVALID: &'static str = "not-an-email";

    fn entity() -> Profile {
        Profile {
            username: username(<Self as AsyncCheckFeature>::CHECKED_BASE),
            name: person_name(Self::PRIMARY_BASE),
            email: email(Self::SECONDARY_BASE),
            availability: DateRange::try_new((Date::new(2026, 1, 1), Date::new(2026, 12, 31)))
                .expect("valid range"),
        }
    }

    fn with_primary(entity: &Profile, raw: &str) -> Profile {
        Profile {
            name: person_name(raw),
            ..entity.clone()
        }
    }

    fn with_secondary(entity: &Profile, raw: &str) -> Profile {
        Profile {
            email: email(raw),
            ..entity.clone()
        }
    }

    fn primary_id() -> ProfileField {
        ProfileField::Name
    }
    fn secondary_id() -> ProfileField {
        ProfileField::Email
    }

    fn primary(draft: &ProfileDraft) -> &Field<PersonName> {
        &draft.name
    }
    fn secondary(draft: &ProfileDraft) -> &Field<Email> {
        &draft.email
    }

    fn set_primary(
        draft: &mut ProfileDraft,
        raw: &str,
    ) -> Result<(), <PersonName as Value>::Error> {
        draft.try_set_name(raw.to_string())
    }
    fn set_secondary(draft: &mut ProfileDraft, raw: &str) -> Result<(), <Email as Value>::Error> {
        draft.try_set_email(raw.to_string())
    }

    /// "Committable" means more than "every field valid": a create-flow draft's `username` is dirty,
    /// so C16 refuses it until the uniqueness check has actually run. A real create form must do
    /// exactly this, which is why the suite makes the fixture say so out loud.
    fn fill_valid(draft: &mut ProfileDraft) {
        draft.try_set_username("carol".to_string()).expect("valid");
        draft.try_set_name("Carol".to_string()).expect("valid");
        draft
            .try_set_email("carol@corp.example".to_string())
            .expect("valid");
        draft
            .try_set_availability(Date::new(2026, 5, 1), Date::new(2026, 5, 2))
            .expect("valid");
        let token = draft.begin_username_check();
        assert!(draft.complete_username_check(token, Ok(())));
    }

    fn forget_secondary_ancestor(stash: &mut ProfileStash) {
        stash.email.base = None;
    }
}

impl RuleFeature for ProfileFixture {
    const RULE: &'static str = "corporate_email";

    fn rule_pins() -> Vec<ProfileField> {
        vec![ProfileField::Email]
    }

    /// Dirty the email to a non-corp domain — the rule is still satisfied, because the username is
    /// `alice`. Then hand back an entity whose *username* is `corp_bob`: rebasing onto it adopts the
    /// username (clean field, C02) and makes the rule fire on an email the rebase never touched.
    fn arrange_rule_flip(draft: &mut ProfileDraft) -> Profile {
        draft
            .try_set_email("bob@other.com".to_string())
            .expect("valid");
        <Self as AsyncCheckFeature>::with_checked(&Self::entity(), "corp_bob")
    }
}

impl AsyncCheckFeature for ProfileFixture {
    type Checked = Username;

    const CHECKED_BASE: &'static str = "alice";
    const CHECKED_MINE: &'static str = "alice2";
    const CHECKED_THEIRS: &'static str = "bravo";
    const CHECK_RULE: &'static str = "username_unique";
    const CHECK_REQUIRED_KEY: &'static str = "username_check_required";

    fn with_checked(entity: &Profile, raw: &str) -> Profile {
        Profile {
            username: username(raw),
            ..entity.clone()
        }
    }

    fn checked_id() -> ProfileField {
        ProfileField::Username
    }
    fn checked(draft: &ProfileDraft) -> &Field<Username> {
        &draft.username
    }
    fn set_checked(draft: &mut ProfileDraft, raw: &str) -> Result<(), <Username as Value>::Error> {
        draft.try_set_username(raw.to_string())
    }

    fn begin_check(draft: &mut ProfileDraft) -> CheckToken {
        draft.begin_username_check()
    }
    fn complete_check(
        draft: &mut ProfileDraft,
        token: CheckToken,
        verdict: Result<(), ErrorData>,
    ) -> bool {
        draft.complete_username_check(token, verdict)
    }
    fn check_state(draft: &ProfileDraft) -> &CheckState<Result<(), ErrorData>> {
        draft.username_check_state()
    }
}

bolted_conformance::feature_suite!(profile, ProfileFixture);
bolted_conformance::rule_suite!(profile_rule, ProfileFixture);
bolted_conformance::async_check_suite!(profile_check, ProfileFixture);

/// The fixture's own promises, which the suite relies on and cannot check from inside a generic:
/// `entity()` really does read `PRIMARY_BASE` / `SECONDARY_BASE` / `CHECKED_BASE`, and the four
/// primary texts really are four distinct values.
#[test]
fn the_fixture_constants_describe_the_entity_it_returns() {
    let e = ProfileFixture::entity();
    assert_eq!(e.name.as_str(), ProfileFixture::PRIMARY_BASE);
    assert_eq!(e.email.as_str(), ProfileFixture::SECONDARY_BASE);
    assert_eq!(
        e.username.as_str(),
        <ProfileFixture as AsyncCheckFeature>::CHECKED_BASE
    );

    let primaries = [
        ProfileFixture::PRIMARY_BASE,
        ProfileFixture::PRIMARY_MINE,
        ProfileFixture::PRIMARY_THEIRS,
        ProfileFixture::PRIMARY_OTHER,
    ]
    .map(person_name);
    for (i, a) in primaries.iter().enumerate() {
        for b in &primaries[i + 1..] {
            assert_ne!(a, b, "the primary texts must be four distinct values");
        }
    }

    assert!(PersonName::try_new(ProfileFixture::PRIMARY_INVALID.to_string()).is_err());
    assert!(Email::try_new(ProfileFixture::SECONDARY_INVALID.to_string()).is_err());
}

/// `ProfileDraft::conflicts()` is `Draft`'s, and the suite reads it; this keeps the import honest.
const _: fn() = || {
    fn assert_draft<D: Draft>() {}
    assert_draft::<ProfileDraft>();
};
