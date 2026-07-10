//! `gen-profile` — `spike-profile`, declared instead of written.
//!
//! `spike-profile` is 724 lines of Rust whose doc comments say, over and over, *"exactly what
//! `#[bolted::value]` would generate"*, *"the shape `#[bolted::entity]` emits"*, *"a macro emits this
//! per field, mechanically"*. This crate is the cash-out. It declares the same feature — the same
//! composite value object, the same tier-2 rule, the same async uniqueness check — and passes the
//! same conformance suite, unmodified.
//!
//! **`spike-profile` is not deleted, and must not be.** It is the golden reference the generated code
//! is read against, and a step that edits its own reference proves nothing. The two coexist; the
//! step-09 report says which differences are intended.
//!
//! What did not survive the round trip, and why:
//!
//! - `try_set_availability(start, end)` becomes `try_set_availability((start, end))`. A macro sees a
//!   value's `Raw` as one type; it does not know that a 2-tuple is two arguments a human would rather
//!   pass separately.
//! - There are no `begin_username_check` / `complete_username_check` / `username_check_state`
//!   conveniences. The surface is [`bolted_core::Checked`], keyed by [`ProfileCheck`] (D18) — which is
//!   what `spike-profile`'s three inherent methods now delegate to anyway.
//!
//! Everything else — every error key, every constraint, every field id — is reproduced exactly. That
//! is what the `key = "…"` overrides on `custom(..)` are for: without them `Email`'s `invalid_email`
//! would silently become `email`, and three localisation files would go stale with nothing to notice.
#![forbid(unsafe_code)]

pub mod value_types;

use bolted_core::{Draft, ErrorData};
pub use value_types::{Date, DateRange, DateRangeError, ascii_alnum_underscore, email};

// =================================================================================================
// Tier 1 — the three newtypes. `DateRange` is composite and stays hand-written (D20).
// =================================================================================================

/// Trim; 3..=20 chars; ASCII alphanumeric + `_`.
#[bolted_macros::value]
#[derive(Eq)]
#[sanitize(trim)]
#[validate(
    len_chars(min = 3, max = 20),
    custom(ascii_alnum_underscore, variant = InvalidChars, key = "invalid_chars")
)]
pub struct Username(String);

/// Trim; 1..=30 chars.
#[bolted_macros::value]
#[derive(Eq)]
#[sanitize(trim)]
#[validate(len_chars(min = 1, max = 30))]
pub struct PersonName(String);

/// Trim + lowercase; one `@`, non-empty local and domain.
#[bolted_macros::value]
#[derive(Eq)]
#[sanitize(trim, lowercase)]
#[validate(custom(email, variant = Invalid, key = "invalid_email"))]
pub struct Email(String);

impl Email {
    /// The domain part. Always present for a valid `Email`, which is why this is safe to call.
    pub fn domain(&self) -> &str {
        self.as_str().split_once('@').map(|(_, d)| d).unwrap_or("")
    }
}

// =================================================================================================
// The entity
// =================================================================================================

/// The always-valid canonical state, and — via the macro — `ProfileField`, `ProfileCheck`,
/// `ProfileStash`, `ProfileDraft`, `ProfileStore`, and the four trait impls.
#[bolted_macros::entity(rules)]
#[derive(Eq)]
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

// =================================================================================================
// Tier 2 — the relational rules
// =================================================================================================

#[bolted_macros::rules(entity = Profile)]
impl ProfileDraft {
    /// A `corp_`-prefixed username requires the `corp.example` email domain.
    ///
    /// Evaluated only over valid values: an invalid or unset field is already flagged by tier 1, and
    /// a rule that fired on it would be noise. The `pins(email)` is what puts the error under the
    /// email box rather than the username box — and it is checked, because `ProfileField` has no
    /// variant for a field that does not exist.
    #[rule(pins(email))]
    fn corporate_email(&self) -> Result<(), ErrorData> {
        if let (Some(u), Some(em)) = (self.username.value(), self.email.value())
            && u.as_str().starts_with("corp_")
            && em.domain() != "corp.example"
        {
            return Err(ErrorData {
                key: "corporate_email_domain",
                params: vec![
                    ("expected", "corp.example".to_string()),
                    ("actual", em.domain().to_string()),
                ],
            });
        }
        Ok(())
    }
}

/// Kill criterion 1 of step 08, still holding for generated code: the store is `Send`, so the FFI
/// wrapper can put it behind the one `Mutex` step 02 demanded. A macro that reached for an `Rc` would
/// fail the build right here.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<ProfileDraft>();
    assert_send::<Profile>();
    assert_send::<ProfileStore>();
};

/// `dirty_fields()` is declaration order, and that is observable: a shell focusing the first invalid
/// field walks this list. `spike-profile` has always emitted it this way; nothing but review said so.
const _: fn() = || {
    fn assert_draft<D: Draft>() {}
    assert_draft::<ProfileDraft>();
};
