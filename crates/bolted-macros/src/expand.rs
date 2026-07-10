//! The `proc_macro::TokenStream` boundary, crossed exactly once, plus the naming helpers every
//! macro needs.
//!
//! Everything above this line is `proc_macro2` and `syn`, which is what lets the macros be unit
//! tested: `proc_macro::TokenStream` can only be constructed inside a real proc-macro invocation.

use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use syn::{Attribute, Ident, Meta};

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

/// `username` → `Username`. Used for field ids, check ids, and error variants.
///
/// Returns `ident` unchanged if the conversion would produce an empty string (an all-underscore
/// name): `format_ident!` panics on a non-identifier, and `CLAUDE.md` bans panics in library code.
/// The compiler then rejects the emitted variant, which is the right place for it to fail.
pub(crate) fn upper_camel(ident: &Ident) -> Ident {
    let mut out = String::new();
    let mut cap = true;
    for c in ident.to_string().chars() {
        if c == '_' {
            cap = true;
        } else if cap {
            out.extend(c.to_uppercase());
            cap = false;
        } else {
            out.push(c);
        }
    }
    if out.is_empty() {
        return ident.clone();
    }
    format_ident!("{}", out, span = ident.span())
}

/// `Profile` + `"Draft"` → `ProfileDraft`.
pub(crate) fn suffixed(base: &Ident, suffix: &str) -> Ident {
    format_ident!("{}{}", base, suffix, span = base.span())
}

/// Is `attrs` carrying a `#[derive(.., Copy, ..)]`?
///
/// D8: value objects must not be `Copy`, because generated checkout/rebase clones every field
/// uniformly and `clippy::clone_on_copy` rejects `.clone()` on a `Copy` field under `-D warnings`.
/// Rust has no negative bound to express this, so [`crate::value`] refuses at rung 2.
pub(crate) fn derives_copy(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("derive") {
            return false;
        }
        let Meta::List(list) = &a.meta else {
            return false;
        };
        let Ok(paths) = list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        ) else {
            return false;
        };
        paths.iter().any(|p| p.is_ident("Copy"))
    })
}

/// Take the attributes named `name` out of `attrs`, returning them. The macro consumes its own
/// helper attributes (`#[sanitize]`, `#[validate]`, `#[check]`, `#[rule]`); everything it does not
/// recognise is passed through to the emitted item, so `#[derive(Eq)]` and doc comments survive.
pub(crate) fn take_attrs(attrs: &mut Vec<Attribute>, name: &str) -> Vec<Attribute> {
    let mut taken = Vec::new();
    let mut kept = Vec::new();
    for a in attrs.drain(..) {
        if a.path().is_ident(name) {
            taken.push(a);
        } else {
            kept.push(a);
        }
    }
    *attrs = kept;
    taken
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn upper_camel_handles_snake_case() {
        let cases = [
            ("username", "Username"),
            ("username_unique", "UsernameUnique"),
            ("a", "A"),
            ("ascii_alnum_underscore", "AsciiAlnumUnderscore"),
        ];
        for (input, want) in cases {
            assert_eq!(upper_camel(&format_ident!("{input}")).to_string(), want);
        }
    }

    /// D8, at the only place it can be caught: the declaration.
    #[test]
    fn copy_is_detected_wherever_it_hides_in_a_derive_list() {
        let yes: syn::ItemStruct = syn::parse2(quote! {
            #[derive(Debug, Clone, Copy, PartialEq)]
            struct X(String);
        })
        .expect("parses");
        assert!(derives_copy(&yes.attrs));

        let no: syn::ItemStruct = syn::parse2(quote! {
            #[derive(Debug, Clone, PartialEq)]
            #[doc = "Copy"]
            struct X(String);
        })
        .expect("parses");
        assert!(!derives_copy(&no.attrs));
    }
}
