//! `#[bolted_macros::value]` — tier 1, the parse-don't-validate boundary (D20).
//!
//! Sanitize, then validate, then wrap. The macro's whole job is to turn a declaration into the three
//! things a hand-written value type repeats: the newtype, the keyed error enum, and the
//! `From<Error> for ErrorData` bridge. Nothing here decides *what is valid* — the length comparison
//! and the user's `custom` predicate do that, and both are ordinary code the compiler checks.

use crate::expand::{derives_copy, take_attrs, upper_camel};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Attribute, Fields, Ident, ItemStruct, LitInt, LitStr, Path, Type, parse2};

/// `trim` / `lowercase`. Applied in declaration order, before any validator sees the raw.
enum Sanitizer {
    Trim,
    Lowercase,
}

/// One declared validator. Each contributes zero or more error variants and exactly one
/// `Constraint`.
enum Validator {
    LenChars {
        min: u32,
        max: u32,
    },
    Custom {
        /// The predicate: `fn(&str) -> bool`.
        path: Path,
        /// The error variant it raises. Defaults to `UpperCamel(last segment)`.
        variant: Ident,
        /// The `ErrorData` key. Defaults to the last segment verbatim.
        key: String,
        /// The name in `Constraint::Custom(..)`, always the last segment.
        constraint: String,
    },
}

pub(crate) fn expand(_attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let mut item: ItemStruct = parse2(item)?;

    // D8. A `Copy` value object makes the uniform `.clone()` in every generated checkout/rebase a
    // hard clippy error. Rust cannot say `!Copy` in a bound, so the refusal lives here — at rung 2,
    // where a build fails, rather than at rung 3 where a lint has to be run.
    if derives_copy(&item.attrs) {
        return Err(syn::Error::new(
            item.span(),
            "a Bolted value object must not be `Copy` (ARCHITECTURE §8, D8): generated \
             checkout/rebase code clones every field uniformly, and `clippy::clone_on_copy` \
             rejects that under `-D warnings`",
        ));
    }

    let raw = newtype_raw(&item)?;
    let sanitizers = parse_sanitizers(&mut item.attrs)?;
    let validators = parse_validators(&mut item.attrs)?;

    if !sanitizers.is_empty() && !is_string(&raw) {
        return Err(syn::Error::new(
            raw.span(),
            "`#[sanitize(..)]` is only defined for a `String` raw",
        ));
    }

    let name = item.ident.clone();
    let error = format_ident!("{}Error", name, span = name.span());
    let vis = item.vis.clone();
    let attrs = &item.attrs;

    let sanitize = sanitizers.iter().map(|s| match s {
        Sanitizer::Trim => quote!(let __raw = __raw.trim().to_owned();),
        Sanitizer::Lowercase => quote!(let __raw = __raw.to_lowercase();),
    });

    let needs_len = validators
        .iter()
        .any(|v| matches!(v, Validator::LenChars { .. }));
    let len_binding = needs_len.then(|| quote!(let __len = __raw.chars().count() as u32;));

    let checks = validators.iter().map(|v| match v {
        // `min == 0` cannot fail, so no `TooShort` arm is emitted and none would be reachable.
        Validator::LenChars { min: 0, max } => quote! {
            if __len > #max { return Err(#error::TooLong { max: #max, actual: __len }); }
        },
        Validator::LenChars { min, max } => quote! {
            if __len < #min { return Err(#error::TooShort { min: #min, actual: __len }); }
            if __len > #max { return Err(#error::TooLong { max: #max, actual: __len }); }
        },
        // `&__raw` is `&String`; the predicate takes `&str` and deref coercion bridges them.
        Validator::Custom { path, variant, .. } => quote! {
            if !#path(&__raw) { return Err(#error::#variant); }
        },
    });

    let error_variants = error_variants(&validators);
    let error_arms = error_arms(&error, &validators);
    let constraints = validators.iter().map(|v| match v {
        Validator::LenChars { min, max } => {
            quote!(::bolted_core::Constraint::LenChars { min: #min, max: #max })
        }
        Validator::Custom { constraint, .. } => {
            quote!(::bolted_core::Constraint::Custom(#constraint))
        }
    });

    let as_str = is_string(&raw).then(|| {
        quote! {
            impl #name {
                pub fn as_str(&self) -> &str { &self.0 }
            }
        }
    });

    Ok(quote! {
        #(#attrs)*
        #[derive(Debug, Clone, PartialEq)]
        #vis struct #name(#raw);

        #as_str

        /// The structured, localisable rejection reason. Never a message string.
        #[derive(Debug, Clone, PartialEq, Eq)]
        #vis enum #error {
            #(#error_variants,)*
        }

        impl ::bolted_core::Value for #name {
            type Raw = #raw;
            type Error = #error;

            fn try_new(__raw: Self::Raw) -> ::core::result::Result<Self, Self::Error> {
                #(#sanitize)*
                #len_binding
                #(#checks)*
                Ok(#name(__raw))
            }

            fn into_raw(self) -> Self::Raw { self.0 }

            fn constraints() -> &'static [::bolted_core::Constraint] {
                &[#(#constraints),*]
            }
        }

        impl ::core::convert::From<#error> for ::bolted_core::ErrorData {
            fn from(__e: #error) -> Self {
                match __e {
                    #(#error_arms)*
                }
            }
        }
    })
}

/// The raw type is the newtype's single field. D20 scopes this macro to newtypes; a composite value
/// object (`DateRange`, whose raw is `(Date, Date)`) keeps its hand-written `Value` impl.
fn newtype_raw(item: &ItemStruct) -> syn::Result<Type> {
    match &item.fields {
        Fields::Unnamed(f) if f.unnamed.len() == 1 => Ok(f.unnamed[0].ty.clone()),
        _ => Err(syn::Error::new(
            item.span(),
            "`#[bolted::value]` declares a newtype: exactly one unnamed field, whose type is the \
             raw form. Composite value objects are not supported (D20) — write the `Value` impl",
        )),
    }
}

fn is_string(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.is_ident("String"))
}

fn parse_sanitizers(attrs: &mut Vec<Attribute>) -> syn::Result<Vec<Sanitizer>> {
    let mut out = Vec::new();
    for attr in take_attrs(attrs, "sanitize") {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("trim") {
                out.push(Sanitizer::Trim);
            } else if meta.path.is_ident("lowercase") {
                out.push(Sanitizer::Lowercase);
            } else {
                return Err(meta.error("unknown sanitizer; expected `trim` or `lowercase`"));
            }
            Ok(())
        })?;
    }
    Ok(out)
}

fn parse_validators(attrs: &mut Vec<Attribute>) -> syn::Result<Vec<Validator>> {
    let mut out = Vec::new();
    for attr in take_attrs(attrs, "validate") {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("len_chars") {
                let (mut min, mut max) = (None, None);
                meta.parse_nested_meta(|m| {
                    let target = if m.path.is_ident("min") {
                        &mut min
                    } else if m.path.is_ident("max") {
                        &mut max
                    } else {
                        return Err(m.error("expected `min` or `max`"));
                    };
                    *target = Some(m.value()?.parse::<LitInt>()?.base10_parse::<u32>()?);
                    Ok(())
                })?;
                let (Some(min), Some(max)) = (min, max) else {
                    return Err(meta.error("`len_chars` needs both `min` and `max`"));
                };
                if min > max {
                    return Err(meta.error("`len_chars` has min > max: no input can satisfy it"));
                }
                out.push(Validator::LenChars { min, max });
            } else if meta.path.is_ident("custom") {
                out.push(parse_custom(&meta)?);
            } else {
                return Err(meta.error("unknown validator; expected `len_chars` or `custom`"));
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// `custom(predicate)` / `custom(predicate, variant = Invalid, key = "invalid_email")`.
///
/// The overrides exist so a generated feature can keep the l10n keys its shells already ship. Without
/// them `Email`'s `invalid_email` would silently become `email`, and three localisation files would
/// go stale with nothing to notice.
fn parse_custom(meta: &syn::meta::ParseNestedMeta) -> syn::Result<Validator> {
    let mut path: Option<Path> = None;
    let mut variant: Option<Ident> = None;
    let mut key: Option<String> = None;

    meta.parse_nested_meta(|m| {
        if m.path.is_ident("variant") {
            variant = Some(m.value()?.parse::<Ident>()?);
        } else if m.path.is_ident("key") {
            key = Some(m.value()?.parse::<LitStr>()?.value());
        } else if path.is_none() {
            path = Some(m.path.clone());
        } else {
            return Err(m.error("`custom` takes one predicate, then `variant`/`key` overrides"));
        }
        Ok(())
    })?;

    let Some(path) = path else {
        return Err(meta.error("`custom(..)` needs a predicate: a `fn(&str) -> bool`"));
    };
    let Some(last) = path.segments.last() else {
        return Err(meta.error("`custom(..)`'s predicate must name a function"));
    };
    let last = last.ident.clone();

    Ok(Validator::Custom {
        variant: variant.unwrap_or_else(|| upper_camel(&last)),
        key: key.unwrap_or_else(|| last.to_string()),
        constraint: last.to_string(),
        path,
    })
}

fn error_variants(validators: &[Validator]) -> Vec<TokenStream2> {
    let mut out = Vec::new();
    for v in validators {
        match v {
            Validator::LenChars { min, .. } => {
                if *min > 0 {
                    out.push(quote!(TooShort {
                        min: u32,
                        actual: u32
                    }));
                }
                out.push(quote!(TooLong {
                    max: u32,
                    actual: u32
                }));
            }
            Validator::Custom { variant, .. } => out.push(quote!(#variant)),
        }
    }
    out
}

/// The `From<Error> for ErrorData` arms. Pure name-stamping: a variant becomes a key, its named
/// fields become params. This block is the most repetitive thing in a hand-written value type, and
/// generating it is most of why `#[bolted::value]` pays for itself.
fn error_arms(error: &Ident, validators: &[Validator]) -> Vec<TokenStream2> {
    let mut out = Vec::new();
    for v in validators {
        match v {
            Validator::LenChars { min, .. } => {
                if *min > 0 {
                    out.push(quote! {
                        #error::TooShort { min, actual } => ::bolted_core::ErrorData {
                            key: "too_short",
                            params: vec![
                                ("min", ::std::string::ToString::to_string(&min)),
                                ("actual", ::std::string::ToString::to_string(&actual)),
                            ],
                        },
                    });
                }
                out.push(quote! {
                    #error::TooLong { max, actual } => ::bolted_core::ErrorData {
                        key: "too_long",
                        params: vec![
                            ("max", ::std::string::ToString::to_string(&max)),
                            ("actual", ::std::string::ToString::to_string(&actual)),
                        ],
                    },
                });
            }
            Validator::Custom { variant, key, .. } => {
                out.push(quote!(#error::#variant => ::bolted_core::ErrorData::new(#key),));
            }
        }
    }
    out
}
