//! The `#[bolted::value]` declaration: tier 1, the parse-don't-validate boundary (D20).

use crate::naming::{derives_copy, take_attrs, upper_camel};
use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use syn::spanned::Spanned;
use syn::{Fields, Ident, ItemStruct, LitInt, LitStr, Path, Type, Visibility, parse2};

/// `trim` / `lowercase`. Applied in declaration order, before any validator sees the raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sanitizer {
    Trim,
    Lowercase,
}

/// One declared validator. Each contributes zero or more error variants and exactly one
/// `Constraint`.
#[derive(Debug, Clone)]
pub enum Validator {
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

/// A named `u32` payload on an error variant. The only payload shape a validator can produce, which
/// is why this is an enum of one and not a `syn::Type`: `bolted-ffi-gen` must project every payload
/// into a `#[data]` field, and it may not be handed a type it cannot cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamTy {
    U32,
}

/// One variant of a value's generated error enum: its ident, its stable `ErrorData` key, and its
/// named params.
///
/// **This type is why `bolted-decl` exists.** `bolted-macros` emits `UsernameError` from it and
/// `bolted-ffi-gen` emits `UsernameErrorFfi` and the `From` bridge between them from it. Derived in
/// one place, the two cannot disagree about whether `len_chars(min = 0, ..)` has a `TooShort`.
#[derive(Debug, Clone)]
pub struct ErrorVariant {
    pub ident: Ident,
    pub key: String,
    pub params: Vec<(&'static str, ParamTy)>,
}

/// A parsed `#[bolted::value]` newtype.
pub struct ValueDecl {
    /// The struct with `#[sanitize]` / `#[validate]` stripped; every other attribute survives.
    pub item: ItemStruct,
    pub name: Ident,
    pub vis: Visibility,
    /// The newtype's single field type — the `Value::Raw`.
    pub raw: Type,
    pub sanitizers: Vec<Sanitizer>,
    pub validators: Vec<Validator>,
}

impl ValueDecl {
    pub fn parse(item: TokenStream2) -> syn::Result<Self> {
        Self::from_item(parse2(item)?)
    }

    pub fn from_item(mut item: ItemStruct) -> syn::Result<Self> {
        // D8. A `Copy` value object makes the uniform `.clone()` in every generated checkout/rebase a
        // hard clippy error. Rust cannot say `!Copy` in a bound, so the refusal lives here — at rung
        // 2, where a build fails, rather than at rung 3 where a lint has to be run.
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
        let decl = ValueDecl {
            name: item.ident.clone(),
            vis: item.vis.clone(),
            raw,
            sanitizers,
            validators,
            item,
        };
        decl.reject_duplicate_variants()?;
        Ok(decl)
    }

    /// `Username` → `UsernameError`.
    pub fn error_ident(&self) -> Ident {
        format_ident!("{}Error", self.name, span = self.name.span())
    }

    /// Is the raw form a `String`? D24 deduplicates the FFI field-state DTOs on exactly this.
    pub fn is_text(&self) -> bool {
        is_string(&self.raw)
    }

    /// Every variant of the generated error enum, in declaration order.
    pub fn error_variants(&self) -> Vec<ErrorVariant> {
        let mut out = Vec::new();
        for v in &self.validators {
            match v {
                // `min == 0` cannot fail, so no `TooShort` variant exists and none would be
                // reachable. Both emitters learn this here, once.
                Validator::LenChars { min, .. } => {
                    if *min > 0 {
                        out.push(ErrorVariant {
                            ident: format_ident!("TooShort"),
                            key: "too_short".to_owned(),
                            params: vec![("min", ParamTy::U32), ("actual", ParamTy::U32)],
                        });
                    }
                    out.push(ErrorVariant {
                        ident: format_ident!("TooLong"),
                        key: "too_long".to_owned(),
                        params: vec![("max", ParamTy::U32), ("actual", ParamTy::U32)],
                    });
                }
                Validator::Custom { variant, key, .. } => out.push(ErrorVariant {
                    ident: variant.clone(),
                    key: key.clone(),
                    params: Vec::new(),
                }),
            }
        }
        out
    }

    /// Two validators raising the same variant emit a duplicate enum variant *and* an unreachable
    /// match arm — a compile error at the use site, pointing into code the user never wrote. Refuse
    /// it here, where the message can name the declaration and say what to do.
    ///
    /// Found by asking what a second `len_chars` on one value does. Nothing forbade it, and the
    /// answer was two `TooShort`s.
    fn reject_duplicate_variants(&self) -> syn::Result<()> {
        let names: Vec<Ident> = self.error_variants().into_iter().map(|v| v.ident).collect();
        for (i, first) in names.iter().enumerate() {
            if let Some(dup) = names[i + 1..].iter().find(|n| *n == first) {
                let hint = if first == "TooShort" || first == "TooLong" {
                    "merge them into one `len_chars(min = .., max = ..)`"
                } else {
                    "give one of them `variant = SomeOtherName`"
                };
                return Err(syn::Error::new(
                    dup.span(),
                    format!("two validators both raise `{first}`: {hint}"),
                ));
            }
        }
        Ok(())
    }
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

fn parse_sanitizers(attrs: &mut Vec<syn::Attribute>) -> syn::Result<Vec<Sanitizer>> {
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

fn parse_validators(attrs: &mut Vec<syn::Attribute>) -> syn::Result<Vec<Validator>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn decl(tokens: TokenStream2) -> ValueDecl {
        ValueDecl::parse(tokens).expect("parses")
    }

    /// The `min = 0` special case is the one place the two emitters could silently disagree.
    #[test]
    fn a_zero_minimum_raises_no_too_short() {
        let d = decl(quote! {
            #[validate(len_chars(min = 0, max = 40))]
            struct Body(String);
        });
        let names: Vec<String> = d
            .error_variants()
            .iter()
            .map(|v| v.ident.to_string())
            .collect();
        assert_eq!(names, ["TooLong"]);

        let d = decl(quote! {
            #[validate(len_chars(min = 1, max = 40))]
            struct Title(String);
        });
        let names: Vec<String> = d
            .error_variants()
            .iter()
            .map(|v| v.ident.to_string())
            .collect();
        assert_eq!(names, ["TooShort", "TooLong"]);
    }

    #[test]
    fn custom_overrides_keep_the_shipped_l10n_keys() {
        let d = decl(quote! {
            #[validate(custom(email, variant = Invalid, key = "invalid_email"))]
            struct Email(String);
        });
        let v = &d.error_variants()[0];
        assert_eq!(v.ident.to_string(), "Invalid");
        assert_eq!(v.key, "invalid_email");
        assert!(v.params.is_empty());
    }

    #[test]
    fn a_copy_value_is_refused_at_rung_2() {
        let err = ValueDecl::parse(quote! {
            #[derive(Copy)]
            struct X(String);
        })
        .map(|_| ())
        .expect_err("D8 refuses Copy");
        assert!(err.to_string().contains("must not be `Copy`"));
    }

    #[test]
    fn two_validators_may_not_raise_the_same_error_variant() {
        let err = ValueDecl::parse(quote! {
            #[validate(len_chars(min = 1, max = 10), len_chars(min = 2, max = 5))]
            struct X(String);
        })
        .map(|_| ())
        .expect_err("duplicate TooShort");
        assert!(err.to_string().contains("both raise `TooShort`"));
    }

    #[test]
    fn the_raw_type_decides_whether_a_value_is_text() {
        assert!(
            decl(quote!(
                struct A(String);
            ))
            .is_text()
        );
        assert!(
            !decl(quote!(
                struct B(u32);
            ))
            .is_text()
        );
    }
}
