//! `bolted-macros` — the derives. **Output is thin delegation to `bolted-core`.**
//!
//! ARCHITECTURE §5 states the doctrine these macros exist to obey:
//!
//! > *Generic framework types carry all logic (rung 1, written once). Derive/attr macros do only
//! > mechanical name-stamping, delegating immediately to the generics. Thin macros are a
//! > verification-ladder requirement: macro output is the least-verifiable code, so it must stay
//! > trivial.*
//!
//! Concretely, that means these macros emit **no `match` over a `Validity`, no `if` that decides
//! whether a commit is refused, and no re-derivation of single-flight sequencing**. Writing them is
//! what pushed three such pieces of behavior out of the would-be generated code and down into the
//! core, where rustc checks them once: [`bolted_core::Field::required_error`],
//! [`bolted_core::commit_gates`], and `SingleFlight::violation`. A macro that reaches for one of
//! those shapes again is a macro that has drifted; the golden snapshots in `src/golden.rs` exist to
//! make that visible in a diff.
//!
//! ## What is here
//!
//! | Macro | Emits |
//! |---|---|
//! | [`macro@value`] | a newtype + `impl Value` + a keyed error enum + `From<Error> for ErrorData` + `constraints()` |
//! | [`macro@entity`] | the entity, `…Field`, `…Check`, `…Stash`, `…Draft`, `…Store`, and `Draft`/`StoreDraft`/`Stashable`/`Checked` |
//! | [`macro@rules`] | the tier-2 rule set the entity's `validate()` calls |
//!
//! ## What is deliberately absent
//!
//! `#[bolted::feature_model]` (D21). It composes onto BoltFFI's `#[data]`/`#[export]`, and
//! `bolted-ffi` is the only crate that may import boltffi; the `Feature` trait it would stamp has
//! never been written, in any of five spikes. See `docs/steps/step-09-report.md`.
//!
//! ## Shape of the code
//!
//! Each macro is a one-line `#[proc_macro_attribute]` shell over an ordinary
//! `fn(TokenStream2, TokenStream2) -> syn::Result<TokenStream2>`. The shells are the only place
//! `proc_macro::TokenStream` appears, which is what makes the real functions unit-testable — a proc
//! macro crate cannot be called from its own integration tests, but it can test itself.
//!
//! Errors are `syn::Error`, rendered as `compile_error!` at the use site: `CLAUDE.md`'s ban on
//! `unwrap`/`expect`/`panic!` in library code holds here, and a malformed declaration must fail the
//! *build*, at rung 2, not the test run.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod entity;
mod expand;
#[cfg(test)]
mod golden;
mod rules;
mod value;

/// Declare a constrained value type (tier 1).
///
/// ```ignore
/// #[bolted_macros::value]
/// #[sanitize(trim)]
/// #[validate(len_chars(min = 3, max = 20), custom(ascii_alnum_underscore, variant = InvalidChars, key = "invalid_chars"))]
/// pub struct Username(String);
/// ```
///
/// The raw type is the newtype's field type; sanitizers run before validators, in declaration order.
///
/// Emits `Username`, `Username::as_str`, `enum UsernameError`, `impl Value for Username`, and
/// `impl From<UsernameError> for ErrorData` — the last being the single most repetitive block in a
/// hand-written feature, and pure name-stamping (variant → snake_case key, named fields → params).
///
/// **`Copy` is rejected** (D8): generated checkout/rebase code clones every field uniformly, and
/// `clippy::clone_on_copy` makes that a hard error under `-D warnings`. Rust cannot express a
/// negative bound, so the macro refuses a `#[derive(Copy)]` value at rung 2 rather than leaving it
/// to a `bolted-check` lint at rung 3.
///
/// Composite value objects (a tuple raw, several named parts) are **not** supported: see D20.
#[proc_macro_attribute]
pub fn value(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand::run(attr, item, value::expand)
}

/// Declare an entity, and with it the draft that edits one.
///
/// ```ignore
/// #[bolted_macros::entity(rules)]
/// pub struct Profile {
///     #[check(rule = "username_unique", pending_key = "…_pending", required_key = "…_required")]
///     pub username: Username,
///     pub name: PersonName,
/// }
/// ```
///
/// `rules` says that a `#[bolted_macros::rules]` impl block exists for this entity. Omit it and the
/// entity emits an empty rule set; the compiler catches both mistakes — a missing impl if you
/// promise rules you do not write, a conflicting impl if you write rules you did not promise.
///
/// Three properties of the output are load-bearing enough to be tested by name:
///
/// - **`is_based()` ORs over every field.** A draft that retains an ancestor in *any* field is
///   entity-backed (C12). Consulting one field passes 21 of the 22 invariants, silently, and lets a
///   stale edit overwrite the server — see the step-08 report.
/// - **`dirty_fields()` and `conflicts()` emit in declaration order**, which is observable.
/// - **Every mutation that can move a checked field's value goes through one generated guard**, so
///   no path can skip C13's verdict reset. Per-call-site guards are how you get a
///   `resolve_take_theirs` that leaves a stale `Done(Ok)` standing.
#[proc_macro_attribute]
pub fn entity(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand::run(attr, item, entity::expand)
}

/// Declare the tier-2 rules of an entity: relational checks over several fields at once.
///
/// ```ignore
/// #[bolted_macros::rules(entity = Profile)]
/// impl ProfileDraft {
///     #[rule(pins(email))]
///     fn corporate_email(&self) -> Result<(), ErrorData> { … }
/// }
/// ```
///
/// A rule returns bare [`bolted_core::ErrorData`]; the macro wraps it in a `RuleViolation` carrying
/// the rule's name and its pinned field ids. Rules see only *valid* values — a field that is invalid
/// or unset is already reported by tier 1, and a rule that fired on it would be noise.
///
/// `pins(email)` becomes `ProfileField::Email`, so **pinning a nonexistent field is a compile
/// error**: the field-id enum has no such variant. `profile.rs` has claimed that property in a
/// comment since step 01 and nothing ever tested it.
#[proc_macro_attribute]
pub fn rules(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand::run(attr, item, rules::expand)
}
