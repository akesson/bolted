//! The `proc_macro::TokenStream` boundary, crossed exactly once.
//!
//! Everything above this line is `proc_macro2` and `syn`, which is what lets the macros be unit
//! tested: `proc_macro::TokenStream` can only be constructed inside a real proc-macro invocation.
//!
//! The naming helpers this module used to own moved to `bolted_decl::naming` in step 10 — the FFI
//! generator has to spell `UsernameError` exactly as this crate spells it (D25).

use proc_macro2::TokenStream as TokenStream2;

/// The shell every `#[proc_macro_attribute]` shares: convert in, run, convert out, and render a
/// `syn::Error` as `compile_error!` at the caller's span.
pub(crate) fn run(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
    f: impl Fn(TokenStream2, TokenStream2) -> syn::Result<TokenStream2>,
) -> proc_macro::TokenStream {
    match f(attr.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
