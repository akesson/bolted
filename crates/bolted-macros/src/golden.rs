//! Golden tests: what the macros emit, formatted, checked in, and diffed.
//!
//! **Why snapshots and not only behaviour tests.** `gen-profile` and `gen-note` already prove the
//! output *behaves*: they run the whole of `bolted-conformance` against it. What they cannot show is
//! the output's *shape*, and shape is the thing ARCHITECTURE §5 makes a rule about. A macro that
//! grew a `match` over `Validity`, or re-derived single-flight sequencing, or decided a commit's
//! gates for itself, would pass every conformance test and violate the doctrine. Here it shows up as
//! a diff a human reads.
//!
//! Run `BLESS=1 cargo test -p bolted-macros` to rewrite the snapshots after an intended change. Read
//! the diff before you commit it — that is the entire point of the file.
//!
//! No `cargo-expand`, no `macrotest`, no second toolchain: `prettyplease` formats the tokens the
//! expander returns, and the expander is a plain function because [`crate::expand::run`] is the only
//! thing that touches `proc_macro::TokenStream`.
//!
//! Panics here are deliberate, as they are in `bolted-conformance`: this module's product is a
//! failing test process.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::path::PathBuf;

/// Format `tokens` as a source file, compare against `tests/golden/<name>.rs`, and fail with a diff.
fn golden(name: &str, tokens: TokenStream2) {
    let file = syn::parse2::<syn::File>(tokens.clone()).unwrap_or_else(|e| {
        panic!("the macro emitted tokens that are not a valid Rust file: {e}\n\n{tokens}")
    });
    let got = prettyplease::unparse(&file);

    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "golden", name]
        .iter()
        .collect::<PathBuf>()
        .with_extension("rs");

    if std::env::var_os("BLESS").is_some() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("create tests/golden");
        }
        std::fs::write(&path, &got).expect("write the snapshot");
        return;
    }

    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing snapshot {}: {e}\nrun with BLESS=1 to create it",
            path.display()
        )
    });

    if want != got {
        panic!(
            "{} drifted from its snapshot.\n\n--- want ---\n{want}\n--- got ---\n{got}\n\
             re-run with BLESS=1 once you have read the diff and agree with it",
            path.display()
        );
    }
}

fn expand_value(input: TokenStream2) -> TokenStream2 {
    crate::value::expand(quote!(), input).expect("the declaration is well-formed")
}

fn expand_entity(attr: TokenStream2, input: TokenStream2) -> TokenStream2 {
    crate::entity::expand(attr, input).expect("the declaration is well-formed")
}

fn expand_rules(attr: TokenStream2, input: TokenStream2) -> TokenStream2 {
    crate::rules::expand(attr, input).expect("the declaration is well-formed")
}

// =================================================================================================
// snapshots
// =================================================================================================

/// The gnarly value: two sanitizers' worth of nothing, a length bound, and a custom predicate whose
/// error key is overridden so `fixture-profile`'s shells keep the l10n key they ship.
#[test]
fn value_username() {
    golden(
        "value_username",
        expand_value(quote! {
            #[sanitize(trim)]
            #[validate(
                len_chars(min = 3, max = 20),
                custom(ascii_alnum_underscore, variant = InvalidChars, key = "invalid_chars")
            )]
            pub struct Username(String);
        }),
    );
}

/// Two sanitizers, one custom validator, no length bound — and therefore no `TooShort`/`TooLong`.
#[test]
fn value_email() {
    golden(
        "value_email",
        expand_value(quote! {
            #[sanitize(trim, lowercase)]
            #[validate(custom(email, variant = Invalid, key = "invalid_email"))]
            pub struct Email(String);
        }),
    );
}

/// The whole entity: a checked field, three plain ones, and `rules`.
#[test]
fn entity_profile() {
    golden(
        "entity_profile",
        expand_entity(
            quote!(rules),
            quote! {
                pub struct Profile {
                    #[check(
                        rule = "username_unique",
                        pending_key = "username_check_pending",
                        required_key = "username_check_required",
                        failed_key = "username_taken"
                    )]
                    pub username: Username,
                    pub name: PersonName,
                    pub email: Email,
                    pub availability: DateRange,
                }
            },
        ),
    );
}

/// The other shape entirely: no check, no rules. The guard collapses to the identity, `…Check` is
/// not emitted, `Checked` is not implemented, and the rule set is empty — all of which a reader can
/// confirm in the snapshot rather than take on trust.
#[test]
fn entity_note() {
    golden(
        "entity_note",
        expand_entity(
            quote!(),
            quote! {
                pub struct Note {
                    pub title: Title,
                    pub body: Body,
                }
            },
        ),
    );
}

#[test]
fn rules_profile() {
    golden(
        "rules_profile",
        expand_rules(
            quote!(entity = Profile),
            quote! {
                impl ProfileDraft {
                    #[rule(pins(email))]
                    fn corporate_email(&self) -> Result<(), ErrorData> {
                        Ok(())
                    }
                }
            },
        ),
    );
}

// =================================================================================================
// refusals — what the macros must NOT compile
//
// Each of these is a rung-2 guarantee. `trybuild` would prove it against a real compiler at the cost
// of a dev-dependency and a second toolchain invocation; the expander returning `Err` is the same
// claim, one layer earlier, and it is what actually produces the `compile_error!`.
// =================================================================================================

#[track_caller]
fn refuses(result: syn::Result<TokenStream2>, expected: &str) {
    match result {
        Ok(tokens) => panic!("expected a refusal mentioning {expected:?}, got:\n{tokens}"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains(expected),
                "the error should mention {expected:?}, but said: {msg}"
            );
        }
    }
}

/// D8, enforced at rung 2. Value objects must not be `Copy`, because generated checkout/rebase
/// clones every field uniformly and `clippy::clone_on_copy` rejects that under `-D warnings`. Rust
/// cannot express a negative bound; this is the enforcement.
#[test]
fn a_value_object_may_not_be_copy() {
    refuses(
        crate::value::expand(
            quote!(),
            quote! {
                #[derive(Copy)]
                #[sanitize(trim)]
                pub struct Slug(String);
            },
        ),
        "must not be `Copy`",
    );
}

/// D20's boundary, stated where a user meets it.
#[test]
fn a_composite_value_is_refused_with_a_pointer_to_the_decision() {
    refuses(
        crate::value::expand(
            quote!(),
            quote! {
                pub struct DateRange { start: Date, end: Date }
            },
        ),
        "Composite value objects are not supported",
    );
}

/// Two validators raising the same error variant emit duplicate enum variants and an unreachable
/// match arm. The compiler catches it either way — but at the use site, pointing into generated code
/// the user never wrote. Refusing here is the difference between a diagnosis and a symptom.
#[test]
fn two_validators_may_not_raise_the_same_error_variant() {
    refuses(
        crate::value::expand(
            quote!(),
            quote! {
                #[validate(len_chars(min = 1, max = 5), len_chars(min = 2, max = 9))]
                pub struct X(String);
            },
        ),
        "merge them into one `len_chars",
    );
    refuses(
        crate::value::expand(
            quote!(),
            quote! {
                #[validate(custom(a::check), custom(b::check))]
                pub struct X(String);
            },
        ),
        "give one of them `variant =",
    );
}

/// The three `#[check(..)]` keys are stable l10n strings that three shells already ship. A macro
/// that defaulted them from the field name would silently move a translation key on a rename.
#[test]
fn a_check_must_name_all_three_of_its_stable_keys() {
    refuses(
        crate::entity::expand(
            quote!(),
            quote! {
                pub struct Profile {
                    #[check(rule = "username_unique")]
                    pub username: Username,
                }
            },
        ),
        "a macro must not invent them",
    );
}

#[test]
fn a_rule_must_pin_its_error_to_a_field() {
    refuses(
        crate::rules::expand(
            quote!(entity = Profile),
            quote! {
                impl ProfileDraft {
                    #[rule(pins())]
                    fn corporate_email(&self) -> Result<(), ErrorData> { Ok(()) }
                }
            },
        ),
        "pin its error to at least one field",
    );
}

#[test]
fn an_empty_rules_block_says_what_to_do_about_it() {
    refuses(
        crate::rules::expand(
            quote!(entity = Profile),
            quote!(
                impl ProfileDraft {}
            ),
        ),
        "drop the block",
    );
}

#[test]
fn an_entity_with_no_fields_has_no_draft() {
    refuses(
        crate::entity::expand(
            quote!(),
            quote!(
                pub struct Empty {}
            ),
        ),
        "no draft to edit",
    );
}

// =================================================================================================
// the three properties the emitted code is not allowed to lose
//
// Snapshots would catch these too, but only if a reviewer noticed. These fail by name.
// =================================================================================================

/// C12. A `StoreDraft::is_based` that consults a *single* field passes 21 of the 22 conformance
/// invariants — verified by mutation on both features in step 08 — and lets a stale edit silently
/// overwrite the server. Now that `is_based` is *generated*, this is where that is caught.
#[test]
fn is_based_ors_over_every_field() {
    let out = expand_entity(
        quote!(),
        quote! {
            pub struct Profile {
                pub username: Username,
                pub name: PersonName,
                pub email: Email,
                pub availability: DateRange,
            }
        },
    )
    .to_string();

    let is_based = out
        .split("fn is_based")
        .nth(1)
        .expect("StoreDraft::is_based is emitted");
    for field in ["username", "name", "email", "availability"] {
        assert!(
            is_based.contains(&format!("self . {field} . base ()")),
            "`is_based` must consult `{field}` — a single-field answer is invisible to 21 of the \
             22 invariants (C12, step 08)"
        );
    }
}

/// Declaration order is observable: a shell that focuses the first invalid field walks `dirty_fields`,
/// and a user reads their form top to bottom.
#[test]
fn dirty_fields_and_conflicts_emit_in_declaration_order() {
    let out = expand_entity(
        quote!(),
        quote! {
            pub struct Profile {
                pub username: Username,
                pub name: PersonName,
                pub email: Email,
            }
        },
    )
    .to_string();

    for method in ["fn dirty_fields", "fn conflicts"] {
        let body = out.split(method).nth(1).expect("emitted");
        let order: Vec<usize> = ["Username", "Name", "Email"]
            .iter()
            .map(|v| {
                body.find(&format!("ProfileField :: {v}"))
                    .unwrap_or_else(|| panic!("{method} must mention {v}"))
            })
            .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "{method} must push fields in declaration order, got offsets {order:?}"
        );
    }
}

/// C13's reset must be reachable from *every* mutation path, so every mutation path must route
/// through the one guard. A macro that emitted `self.username_check.reset()` per call site would
/// forget the next call site somebody adds.
#[test]
fn every_mutation_path_routes_through_the_single_guard() {
    let out = expand_entity(
        quote!(),
        quote! {
            pub struct Profile {
                #[check(rule = "u", pending_key = "p", required_key = "r", failed_key = "f")]
                pub username: Username,
                pub name: PersonName,
            }
        },
    )
    .to_string();

    // Exactly one `reset()` exists, and it is inside `bolted_guard`.
    assert_eq!(
        out.matches(". reset ()").count(),
        1,
        "the verdict reset must exist exactly once, inside the guard"
    );
    let guard = out
        .split("fn bolted_guard")
        .nth(1)
        .expect("the guard is emitted");
    assert!(guard.contains(". reset ()"), "the reset lives in the guard");

    // Every mutation that CAN move a checked field's value routes through it: the checked field's own
    // setter, both resolvers (which take a field id at runtime), and `rebase` (which moves every
    // field). Named once more to define it. Nowhere else, because nowhere else can a checked value
    // move.
    const CHECKED_SETTERS: usize = 1;
    assert_eq!(
        out.matches("bolted_guard").count(),
        1 + CHECKED_SETTERS + 2 + 1,
        "definition, {CHECKED_SETTERS} checked setter, 2 resolvers, rebase — and no unguarded path"
    );
}

/// The converse, and it is about latency, not correctness: `try_set_name` **must not** be guarded.
///
/// The guard clones every checked field's value and compares it afterwards. Routing an unchecked
/// field's setter through it would clone the `Username` on every keystroke of the *name* box — on the
/// exact path step 07's kill criterion 4 measures, and the one the "core validates every keystroke"
/// bet rests on. `fixture-profile` guards only `try_set_username`; the first version of this macro
/// guarded all four, and the report would have claimed the hot path was untouched.
#[test]
fn an_unchecked_fields_setter_does_not_pay_for_the_guard() {
    // Three fields, so that neither setter under test is the last one: `body_of` slices up to the
    // next `pub fn`, and the final setter would otherwise swallow the resolvers and `rebase` — whose
    // guards are correct and would make this test pass for the wrong reason.
    let out = expand_entity(
        quote!(),
        quote! {
            pub struct Profile {
                #[check(rule = "u", pending_key = "p", required_key = "r", failed_key = "f")]
                pub username: Username,
                pub name: PersonName,
                pub email: Email,
            }
        },
    )
    .to_string();

    let body_of = |setter: &str| -> String {
        let body = out
            .split(&format!("pub fn {setter}"))
            .nth(1)
            .and_then(|s| s.split("pub fn ").next())
            .unwrap_or_else(|| panic!("{setter} is emitted"))
            .to_string();
        assert!(
            !body.contains("resolve_keep_mine"),
            "the slice for {setter} must stop before the resolvers, or it proves nothing"
        );
        body
    };

    assert!(
        !body_of("try_set_name").contains("bolted_guard"),
        "an unchecked field's setter must not clone every checked value on every keystroke"
    );
    assert!(
        body_of("try_set_username").contains("bolted_guard"),
        "the checked field's own setter must be guarded (C13)"
    );
}

/// The doctrine, as a test. Behavior belongs to `bolted-core`; the macro stamps names. If one of
/// these ever appears in emitted code, the judgement it encodes has moved to the least verifiable
/// place on the ladder (ARCHITECTURE §5) — and no conformance test would notice.
#[test]
fn the_emitted_code_makes_no_judgement_of_its_own() {
    let out = expand_entity(
        quote!(rules),
        quote! {
            pub struct Profile {
                #[check(rule = "u", pending_key = "p", required_key = "r", failed_key = "f")]
                pub username: Username,
                pub name: PersonName,
            }
        },
    )
    .to_string();

    let forbidden = [
        ("Validity ::", "tier-1 validity is `Field::required_error`"),
        ("CheckState ::", "C13 + C16 are `SingleFlight::violation`"),
        (
            "CommitError :: Conflicted",
            "C07's gates are `commit_gates`",
        ),
        ("CommitError :: Orphaned", "C07's gates are `commit_gates`"),
        ("is_ok ()", "C07's gates are `commit_gates`"),
    ];
    for (needle, why) in forbidden {
        assert!(
            !out.contains(needle),
            "emitted code contains {needle:?} — {why} (ARCHITECTURE §5)"
        );
    }
}
