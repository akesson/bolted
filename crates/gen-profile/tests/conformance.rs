//! `gen-profile` proves that a **generated** feature is a Bolted feature — rule, async check,
//! composite value and all.
//!
//! This file is `spike-profile/tests/conformance.rs` with three changes, and no others:
//!
//! 1. `spike_profile` → `gen_profile`.
//! 2. `try_set_availability(start, end)` → `try_set_availability((start, end))`.
//! 3. `fill_valid` drives the check through `bolted_core::Checked` instead of the inherent
//!    `begin_username_check` conveniences, which the macro does not emit.
//!
//! Not a constant moved, not an assertion weakened, not a suite line dropped. If the generated code
//! needed a concession the hand-written code did not, that would be kill criterion 1 of step 09.

use bolted_conformance::{AsyncCheckFeature, ConformanceFeature, RuleFeature, ValueFixture};
use bolted_core::{Checked, Draft, Field, Value};
use gen_profile::{
    Date, DateRange, Email, PersonName, Profile, ProfileCheck, ProfileDraft, ProfileField,
    ProfileStash, Username,
};
use proptest::prelude::*;
use proptest::strategy::{BoxedStrategy, Strategy};

// =================================================================================================
// Value fixtures — one per value type. Three are generated; `DateRange` is hand-written (D20), and
// the suite cannot tell which is which.
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
    /// so C16 refuses it until the uniqueness check has actually run.
    fn fill_valid(draft: &mut ProfileDraft) {
        draft.try_set_username("carol".to_string()).expect("valid");
        draft.try_set_name("Carol".to_string()).expect("valid");
        draft
            .try_set_email("carol@corp.example".to_string())
            .expect("valid");
        draft
            .try_set_availability((Date::new(2026, 5, 1), Date::new(2026, 5, 2)))
            .expect("valid");
        let check = ProfileCheck::UsernameUnique;
        let token = draft.begin_check(check);
        assert!(draft.complete_check(check, token, Ok(())));
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

    fn check_id() -> ProfileCheck {
        ProfileCheck::UsernameUnique
    }

    fn with_checked(entity: &Profile, raw: &str) -> Profile {
        Profile {
            username: username(raw),
            ..entity.clone()
        }
    }

    fn checked(draft: &ProfileDraft) -> &Field<Username> {
        &draft.username
    }
    fn set_checked(draft: &mut ProfileDraft, raw: &str) -> Result<(), <Username as Value>::Error> {
        draft.try_set_username(raw.to_string())
    }
}

bolted_conformance::feature_suite!(profile, ProfileFixture);
bolted_conformance::rule_suite!(profile_rule, ProfileFixture);
bolted_conformance::async_check_suite!(profile_check, ProfileFixture);

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

const _: fn() = || {
    fn assert_draft<D: Draft>() {}
    assert_draft::<ProfileDraft>();
};
