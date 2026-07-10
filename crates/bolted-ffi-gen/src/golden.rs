//! Properties of the emitted FFI layer.
//!
//! There are no snapshot *files* here, and there need not be: `crates/gen-note-ffi/src/generated.rs`
//! and `crates/gen-profile-ffi/src/generated.rs` **are** the golden snapshots. They are committed,
//! formatted by `prettyplease`, compiled, clippied under `-D warnings`, and byte-compared against the
//! declaration on every `mise run check` (D22). A snapshot a compiler reads is worth several that only
//! a diff does.
//!
//! What is left for this file is what a snapshot cannot state: properties that must hold for *every*
//! feature, including ones nobody has written.

use crate::generate;

/// Two text fields, no rule, no check, no composite. `gen-note`, essentially.
const PLAIN: &str = r#"
    #[bolted_macros::value]
    #[sanitize(trim)]
    #[validate(len_chars(min = 1, max = 40))]
    pub struct Title(String);

    #[bolted_macros::value]
    #[validate(len_chars(min = 0, max = 200))]
    pub struct Body(String);

    #[bolted_macros::entity]
    pub struct Note {
        pub title: Title,
        pub body: Body,
    }
"#;

/// Everything at once: a check, a rule, and an undeclared (composite) value type.
const GNARLY: &str = r#"
    #[bolted_macros::value]
    #[sanitize(trim)]
    #[validate(len_chars(min = 3, max = 20))]
    pub struct Username(String);

    #[bolted_macros::value]
    #[validate(custom(email, variant = Invalid, key = "invalid_email"))]
    pub struct Email(String);

    #[bolted_macros::entity(rules)]
    pub struct Profile {
        #[check(
            rule = "username_unique",
            pending_key = "p",
            required_key = "r",
            failed_key = "username_taken"
        )]
        pub username: Username,
        pub email: Email,
        pub availability: DateRange,
    }

    #[bolted_macros::rules(entity = Profile)]
    impl ProfileDraft {
        #[rule(pins(email))]
        fn corporate_email(&self) -> Result<(), ErrorData> { Ok(()) }
    }
"#;

fn plain() -> String {
    generate(PLAIN, "gen_note").expect("generates")
}

fn gnarly() -> String {
    generate(GNARLY, "gen_profile").expect("generates")
}

/// The judgements a generated FFI layer may not make. `bolted-core` owns every one of them.
///
/// | needle | the judgement | where it lives |
/// |---|---|---|
/// | `Validity::` | is this field's input an error? | `Field::required_error` (D13) |
/// | `CommitError::` | may this draft commit? | `commit_gates` (C07) |
/// | `CheckState::` | does an unrun check block? | `SingleFlight::violation` (C13 + C16) |
/// | `SyncState::` | is this field conflicted? | `Field::sync` |
/// | `.is_ok()` | is this report clean? | `ValidationReport::is_ok` |
///
/// Step 09 wrote the same rule for `bolted-macros`, matching against a `TokenStream`'s `to_string`,
/// where `quote` prints `Validity ::` with a space. This file matches against `prettyplease` output,
/// which prints paths tight. Copying step 09's needles here made every assertion **vacuous, and
/// green**. Hence [`the_forbidden_needles_can_actually_fire`], below.
const FORBIDDEN: [&str; 5] = [
    "Validity::",
    "CommitError::",
    "CheckState::",
    "SyncState::",
    ".is_ok()",
];

#[test]
fn the_emitted_code_makes_no_judgement_of_its_own() {
    for (name, src) in [("plain", plain()), ("gnarly", gnarly())] {
        for forbidden in FORBIDDEN {
            assert!(
                !src.contains(forbidden),
                "the generated FFI layer for `{name}` mentions `{forbidden}`. A judgement has moved \
                 out of bolted-core into codegen, where no reviewer reads it and no type-checker \
                 constrains it. Put it back."
            );
        }
    }
}

/// A forbidding test that cannot fire forbids nothing.
///
/// Runs a fragment that makes all five judgements through the same formatter the generator uses, and
/// insists every needle matches. If `prettyplease` ever changes how it spaces a path, this fails —
/// rather than `the_emitted_code_makes_no_judgement_of_its_own` quietly passing forever.
#[test]
fn the_forbidden_needles_can_actually_fire() {
    let judgemental: syn::File = syn::parse_str(
        r#"
        fn guilty(f: &Field<V>, r: &Report, c: &CheckState<()>, s: &SyncState<V>) -> bool {
            let _ = matches!(f.validity(), Validity::Unset);
            let _ = matches!(s, SyncState::InSync);
            let _ = matches!(c, CheckState::Idle);
            let _ = CommitError::Orphaned;
            r.is_ok()
        }
        "#,
    )
    .expect("parses");
    let formatted = prettyplease::unparse(&judgemental);

    for forbidden in FORBIDDEN {
        assert!(
            formatted.contains(forbidden),
            "`{forbidden}` does not match code that plainly contains it, once `prettyplease` has \
             formatted it. The needle is wrong, and `the_emitted_code_makes_no_judgement_of_its_own` \
             is asserting nothing.\n\n{formatted}"
        );
    }
}

/// D23. Every verb that *mutates* a draft must be able to say the draft is gone; a silent `Ok(())`
/// after C17 released it is the lie this step set out to remove.
///
/// Observers stay total — `is_live()` is how a shell asks, and `validate()` runs on every keystroke.
#[test]
fn every_mutating_verb_can_refuse_a_dead_draft_and_no_observer_can() {
    let src = gnarly();

    for mutator in [
        "pub fn try_set_username",
        "pub fn try_set_email",
        "pub fn try_set_availability",
        "pub fn resolve_keep_mine",
        "pub fn resolve_take_theirs",
        "pub fn run_username_check",
    ] {
        let sig = signature(&src, mutator);
        assert!(
            sig.contains("Result"),
            "the mutator `{mutator}` returns `{sig}` — it cannot refuse a released draft, so it \
             silently does nothing (D23)"
        );
    }

    for observer in [
        "pub fn snapshot",
        "pub fn validate",
        "pub fn stash",
        "pub fn is_live",
    ] {
        let sig = signature(&src, observer);
        assert!(
            !sig.contains("Result"),
            "the observer `{observer}` returns `{sig}`, but observers are total"
        );
    }
}

/// From `pub fn foo` up to its opening brace.
fn signature<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not in the generated code at all"));
    let rest = &src[start..];
    let end = rest.find('{').unwrap_or(rest.len());
    rest[..end].trim_end()
}

/// D24. Two `Raw = String` values share one field-state family; nothing per-value is stamped.
#[test]
fn text_fields_share_one_field_state_family() {
    let src = gnarly();
    assert!(src.matches("TextFieldState").count() >= 2);
    for per_value in ["UsernameFieldState", "EmailValidity", "UsernameFieldSync"] {
        assert!(
            !src.contains(per_value),
            "`{per_value}` was stamped. `Validity<V>` mentions V; the wire shape mentions only \
             `V::Raw`, and the raw type is the axis that varies across the boundary (D19/D24)"
        );
    }
    // ...but *errors* stay per value: they have different variants, and a typed throw is a feature.
    assert!(src.contains("enum UsernameErrorFfi"));
    assert!(src.contains("enum EmailErrorFfi"));
}

/// D25's escape hatch. `DateRange` is not declared in the source, so the generator must not invent a
/// projection for it. It names one, and lets the compiler demand it.
#[test]
fn an_undeclared_value_type_is_never_guessed_at() {
    let src = gnarly();
    assert!(src.contains("use crate::custom::*;"));
    for demanded in [
        "crate::custom::availability_state",
        "crate::custom::availability_raw",
        "crate::custom::availability_stash",
        "crate::custom::availability_from_stash",
        "crate::custom::availability_error",
        "crate::custom::availability_closed",
    ] {
        assert!(
            src.contains(demanded),
            "the generator stopped demanding `{demanded}` — so a missing projection is no longer a \
             compile error, and a composite could cross as whatever the generator felt like"
        );
    }
    // It must not have derived a wire shape from the value's name or its parts.
    assert!(!src.contains("struct AvailabilityRaw"));
    assert!(!src.contains("struct DateRangeFfi"));
}

/// A generator with one input is shaped like that input — step 08's lesson, then step 09's, now this
/// one's. `gen-note-ffi` was generated first so these paths are real, not retrofitted.
#[test]
fn a_feature_without_checks_or_composites_pays_for_neither() {
    let src = plain();
    assert!(
        !src.contains("use crate::custom::*;"),
        "no composite: no custom module"
    );
    assert!(
        !src.contains("Checked"),
        "no check: `Checked` is not imported"
    );
    assert!(
        !src.contains("CheckStateFfi"),
        "no check: nothing check-shaped in the snapshot"
    );
    assert!(!src.contains("Checker"), "no check: no capability trait");
    assert!(
        !src.contains("CoreErrorData"),
        "no check: no failed_key to raise"
    );

    // And everything that does not depend on a check is still there.
    assert!(src.contains("pub fn try_set_title"));
    assert!(src.contains("pub fn submit"));
    assert!(src.contains("enum NoteFieldId"));
}

/// Declaration order is observable: `dirty_fields()` walks it, and a shell focusing the first invalid
/// field walks that. The FFI's field-id enum and snapshot must agree with the declaration.
#[test]
fn the_field_id_enum_and_snapshot_follow_declaration_order() {
    let src = gnarly();

    let enum_body = between(&src, "pub enum ProfileFieldId {", "}");
    assert_ordered(
        enum_body,
        &["Username", "Email", "Availability"],
        "field id",
    );

    let snap = between(&src, "pub struct ProfileSnapshot {", "}");
    assert_ordered(snap, &["username", "email", "availability"], "snapshot");
}

fn assert_ordered(haystack: &str, needles: &[&str], what: &str) {
    let at: Vec<usize> = needles
        .iter()
        .map(|n| {
            haystack
                .find(n)
                .unwrap_or_else(|| panic!("{what}: `{n}` missing from `{haystack}`"))
        })
        .collect();
    assert!(
        at.windows(2).all(|w| w[0] < w[1]),
        "{what} is not in declaration order: {haystack}"
    );
}

fn between<'a>(src: &'a str, open: &str, close: &str) -> &'a str {
    let start = src
        .find(open)
        .unwrap_or_else(|| panic!("`{open}` not found"))
        + open.len();
    let rest = &src[start..];
    &rest[..rest.find(close).unwrap_or(rest.len())]
}

/// The foreign checker is called with **no lock held**. Step 02 called this the wrapper's hardest-won
/// invariant, and a reentrant Swift checker punishes a violation with a deadlock.
///
/// Textual, and therefore weak — the Swift and Kotlin reentrancy probes are the real test. It is here
/// because it is free, and because a generator can lose this in one careless edit.
#[test]
fn the_foreign_checker_is_called_outside_every_lock() {
    let src = gnarly();
    let call = src.find("checker.check(value)").expect("the outcall");
    let before = &src[..call];
    let last_lock = before
        .rfind("let mut g = lock(&self.core);")
        .expect("a locked phase precedes the outcall");
    let tail = &before[last_lock..];
    let (opens, closes) = (tail.matches('{').count(), tail.matches('}').count());
    assert!(
        closes > opens,
        "the store lock is still held when the foreign checker is called: a reentrant checker \
         deadlocks, and step 02 paid for that lesson once already"
    );
}

/// A file that says it is generated is a file nobody hand-edits.
#[test]
fn the_generated_file_says_so_and_says_how_to_regenerate() {
    let src = plain();
    assert!(src.starts_with("// @generated by bolted-ffi-gen. DO NOT EDIT."));
    assert!(src.contains("mise run gen:ffi"));
}

/// Generation is a function. If it were not, the drift check would be a coin toss.
#[test]
fn generation_is_deterministic() {
    assert_eq!(gnarly(), gnarly());
}

/// The name-collision refusal (step 12, deliverable 6c). A generated top-level type whose name is a
/// per-language built-in is refused at generation time, naming the offended language — no silent
/// rename. It is a **tripwire**: the generator suffixes every declaration-derived name today
/// (`<Entity>Snapshot`, `<Value>ErrorFfi`), so no real declaration can produce a bare `Date`/`Error`;
/// the check is exercised directly rather than through an input that cannot reach it. That `gnarly()`
/// and `plain()` above call `generate()` — which now runs this check — is the proof the real emitted
/// layer clears the deny-list.
#[test]
fn a_generated_type_named_like_a_platform_builtin_is_refused() {
    let swift_clash: syn::File = syn::parse_str("pub struct Date { pub y: u16 }").expect("valid");
    let err = crate::reject_reserved_type_names(&swift_clash).expect_err("`Date` must be refused");
    let msg = err.to_string();
    assert!(
        msg.contains("Swift"),
        "the refusal names the offended language: {msg}"
    );
    assert!(msg.contains("Date"), "and the colliding name: {msg}");

    let kotlin_clash: syn::File = syn::parse_str("pub enum Exception { A }").expect("valid");
    assert!(
        crate::reject_reserved_type_names(&kotlin_clash).is_err(),
        "`Exception` collides with a Kotlin built-in"
    );

    // Ordinary suffixed names — what the generator actually emits — pass.
    let clean: syn::File =
        syn::parse_str("pub struct ProfileSnapshot { pub v: u64 }").expect("valid");
    assert!(crate::reject_reserved_type_names(&clean).is_ok());
}
