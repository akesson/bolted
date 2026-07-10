//! Naming helpers shared by every emitter. Moved out of `bolted-macros::expand` in step 10 (D25),
//! unchanged: `bolted-ffi-gen` must spell `UsernameError` exactly as `bolted-macros` spelled it, and
//! two copies of `upper_camel` would eventually disagree about `ascii_alnum_underscore`.

use quote::format_ident;
use syn::{Attribute, Ident, Meta};

/// `username` → `Username`. Used for field ids, check ids, and error variants.
///
/// Returns `ident` unchanged if the conversion would produce an empty string (an all-underscore
/// name): `format_ident!` panics on a non-identifier, and `CLAUDE.md` bans panics in library code.
/// The compiler then rejects the emitted variant, which is the right place for it to fail.
pub fn upper_camel(ident: &Ident) -> Ident {
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
pub fn suffixed(base: &Ident, suffix: &str) -> Ident {
    format_ident!("{}{}", base, suffix, span = base.span())
}

/// Is `attrs` carrying a `#[derive(.., Copy, ..)]`?
///
/// D8: value objects must not be `Copy`, because generated checkout/rebase clones every field
/// uniformly and `clippy::clone_on_copy` rejects `.clone()` on a `Copy` field under `-D warnings`.
/// Rust has no negative bound to express this, so [`crate::ValueDecl::parse`] refuses at rung 2.
pub fn derives_copy(attrs: &[Attribute]) -> bool {
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

/// Take the attributes named `name` out of `attrs`, returning them. A declaration consumes its own
/// helper attributes (`#[sanitize]`, `#[validate]`, `#[check]`, `#[rule]`); everything unrecognised is
/// passed through to the emitted item, so `#[derive(Eq)]` and doc comments survive.
pub fn take_attrs(attrs: &mut Vec<Attribute>, name: &str) -> Vec<Attribute> {
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

/// Does `attr` name a Bolted declaration attribute called `name`?
///
/// `#[bolted_macros::value]`, `#[bolted::value]` and a bare `#[value]` all count; `#[serde::value]`
/// does not. `bolted-ffi-gen` scans a feature crate's source the way BoltFFI scans ours, so it cannot
/// resolve `use` aliases and must recognise the attribute by spelling. Requiring the qualifier to
/// start with `bolted` is what keeps "by spelling" from meaning "by last segment, whoever wrote it".
pub fn is_bolted_attr(attr: &Attribute, name: &str) -> bool {
    let segments = &attr.path().segments;
    let Some(last) = segments.last() else {
        return false;
    };
    if last.ident != name {
        return false;
    }
    match segments.len() {
        1 => true,
        2 => segments[0].ident.to_string().starts_with("bolted"),
        _ => false,
    }
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

    /// The generator finds declarations by spelling, because it reads source text and cannot resolve
    /// a `use` alias. Which makes "whose `value` is it?" a question it has to answer.
    #[test]
    fn a_bolted_attr_is_recognised_by_spelling_but_not_by_last_segment_alone() {
        let item: syn::ItemStruct = syn::parse2(quote! {
            #[bolted_macros::value]
            #[bolted::value]
            #[value]
            #[serde::value]
            #[a::b::value]
            #[derive(Eq)]
            struct X(String);
        })
        .expect("parses");
        let matched: Vec<bool> = item
            .attrs
            .iter()
            .map(|a| is_bolted_attr(a, "value"))
            .collect();
        assert_eq!(matched, [true, true, true, false, false, false]);
    }
}
