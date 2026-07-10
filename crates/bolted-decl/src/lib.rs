//! `bolted-decl` — the Bolted declaration, parsed once (D25).
//!
//! Two emitters consume this crate, and they must agree exactly:
//!
//! - [`bolted-macros`](../bolted_macros/index.html) emits the feature — the newtype, the draft, the
//!   store, the trait impls.
//! - `bolted-ffi-gen` emits the FFI layer — the `#[data]` DTOs, the `#[export]` classes, and the
//!   `From<UsernameError> for UsernameErrorFfi` bridge between them.
//!
//! A proc-macro crate can export nothing but its macros, so this split is forced. It is also the
//! point: **two parsers would be two contracts.** `mise run check` regenerates the committed FFI
//! source and byte-compares it (D22); with two parsers that check would be comparing a generator
//! against itself, and a declaration that meant one thing to the macro and another to the generator
//! would pass.
//!
//! [`ValueDecl::error_variants`] is where this earns its keep. `len_chars(min = 0, ..)` raises no
//! `TooShort`, because no input can be shorter than nothing. The macro knows. The FFI generator has
//! to know the same thing, in the same place, or `UsernameErrorFfi` gains a variant that the `From`
//! impl can never construct — and the compiler would accept it.
#![forbid(unsafe_code)]

pub mod entity;
pub mod feature;
pub mod naming;
pub mod rules;
pub mod value;

pub use entity::{Check, EntityDecl, EntityField};
pub use feature::Feature;
pub use rules::{Rule, RulesDecl};
pub use value::{ErrorVariant, ParamTy, Sanitizer, Validator, ValueDecl};
