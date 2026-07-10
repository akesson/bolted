//! `gen-note` proves that a **generated** feature is a Bolted feature.
//!
//! This file is `spike-note/tests/conformance.rs` with `spike_note` swapped for `gen_note`. Nothing
//! else changed — not a constant, not an assertion, not a suite line. That is the claim step 09
//! exists to make, and the diff between the two files is its whole evidence.
//!
//! Still no `rule_suite!` and no `async_check_suite!`: a generated feature with no rule and no check
//! owes C08, C10, C13, C16 and C20's verdict clause exactly as little as a hand-written one does.

use bolted_conformance::{ConformanceFeature, ValueFixture};
use bolted_core::{Field, Value};
use gen_note::{Body, Note, NoteDraft, NoteField, NoteStash, Title};
use proptest::prelude::*;
use proptest::strategy::BoxedStrategy;

pub struct TitleFixture;
impl ValueFixture for TitleFixture {
    type Value = Title;
    fn any_raw() -> BoxedStrategy<String> {
        prop_oneof![".*", "[A-Za-z ]{1,40}", "   [A-Za-z]{1,20}   "].boxed()
    }
    fn valid_raw() -> BoxedStrategy<String> {
        "[A-Za-z]{1,40}".boxed()
    }
    fn invalid_raw() -> String {
        "  ".to_string() // trims to empty
    }
}

pub struct BodyFixture;
impl ValueFixture for BodyFixture {
    type Value = Body;
    fn any_raw() -> BoxedStrategy<String> {
        prop_oneof![".*", "[A-Za-z ]{1,200}", "  [A-Za-z]{1,50}  "].boxed()
    }
    fn valid_raw() -> BoxedStrategy<String> {
        "[A-Za-z]{1,200}".boxed()
    }
    fn invalid_raw() -> String {
        "".to_string()
    }
}

bolted_conformance::field_suite!(title, TitleFixture);
bolted_conformance::field_suite!(body, BodyFixture);

pub struct NoteFixture;

fn title(raw: &str) -> Title {
    Title::try_new(raw.to_string()).expect("a fixture constant must be valid")
}
fn body(raw: &str) -> Body {
    Body::try_new(raw.to_string()).expect("a fixture constant must be valid")
}

impl ConformanceFeature for NoteFixture {
    type Entity = Note;
    type Draft = NoteDraft;
    type Primary = Title;
    type Secondary = Body;

    const PRIMARY_BASE: &'static str = "Shopping";
    const PRIMARY_MINE: &'static str = "My Title";
    const PRIMARY_THEIRS: &'static str = "Their Title";
    const PRIMARY_OTHER: &'static str = "Other Title";
    const PRIMARY_INVALID: &'static str = "  ";
    const SECONDARY_BASE: &'static str = "milk and eggs";
    const SECONDARY_THEIRS: &'static str = "bread";
    const SECONDARY_INVALID: &'static str = "";

    fn entity() -> Note {
        Note {
            title: title(Self::PRIMARY_BASE),
            body: body(Self::SECONDARY_BASE),
        }
    }

    fn with_primary(entity: &Note, raw: &str) -> Note {
        Note {
            title: title(raw),
            ..entity.clone()
        }
    }

    fn with_secondary(entity: &Note, raw: &str) -> Note {
        Note {
            body: body(raw),
            ..entity.clone()
        }
    }

    fn primary_id() -> NoteField {
        NoteField::Title
    }
    fn secondary_id() -> NoteField {
        NoteField::Body
    }

    fn primary(draft: &NoteDraft) -> &Field<Title> {
        &draft.title
    }
    fn secondary(draft: &NoteDraft) -> &Field<Body> {
        &draft.body
    }

    fn set_primary(draft: &mut NoteDraft, raw: &str) -> Result<(), <Title as Value>::Error> {
        draft.try_set_title(raw.to_string())
    }
    fn set_secondary(draft: &mut NoteDraft, raw: &str) -> Result<(), <Body as Value>::Error> {
        draft.try_set_body(raw.to_string())
    }

    fn fill_valid(draft: &mut NoteDraft) {
        draft.try_set_title("Fresh".to_string()).expect("valid");
        draft.try_set_body("something".to_string()).expect("valid");
    }

    fn forget_secondary_ancestor(stash: &mut NoteStash) {
        stash.body.base = None;
    }
}

bolted_conformance::feature_suite!(note, NoteFixture);

#[test]
fn the_fixture_constants_describe_the_entity_it_returns() {
    let e = NoteFixture::entity();
    assert_eq!(e.title.as_str(), NoteFixture::PRIMARY_BASE);
    assert_eq!(e.body.as_str(), NoteFixture::SECONDARY_BASE);

    let primaries = [
        NoteFixture::PRIMARY_BASE,
        NoteFixture::PRIMARY_MINE,
        NoteFixture::PRIMARY_THEIRS,
        NoteFixture::PRIMARY_OTHER,
    ]
    .map(title);
    for (i, a) in primaries.iter().enumerate() {
        for b in &primaries[i + 1..] {
            assert_ne!(a, b, "the primary texts must be four distinct values");
        }
    }

    assert!(Title::try_new(NoteFixture::PRIMARY_INVALID.to_string()).is_err());
    assert!(Body::try_new(NoteFixture::SECONDARY_INVALID.to_string()).is_err());
}

/// The generated feature agrees with the hand-written one on every constraint it declares — the
/// numbers came out of the DSL, not out of a shell (`CLAUDE.md`: no constraint literals in shells).
///
/// This is the only assertion in the file that `spike-note` does not also make, and it is here
/// because `constraints()` is the one thing `#[bolted::value]` emits that no conformance invariant
/// covers: it is shell-affordance metadata, exported and never re-checked.
#[test]
fn the_declared_constraints_survive_the_macro() {
    use bolted_core::Constraint::{LenChars, Required};

    assert_eq!(
        NoteField::Title.constraints(),
        vec![Required, LenChars { min: 1, max: 40 }]
    );
    assert_eq!(
        NoteField::Body.constraints(),
        vec![Required, LenChars { min: 1, max: 200 }]
    );
}
