//! `#[bolted_macros::rules]` — tier 2, the relational rules of one entity.
//!
//! A rule is an ordinary private method returning `Result<(), ErrorData>`. The macro's entire
//! contribution is to wrap each `Err` in a `RuleViolation` carrying the rule's name and the field
//! ids it pins to, and to collect them in declaration order. It never decides what a rule *means*.
//!
//! `pins(email)` becomes `ProfileField::Email`, so a typo names a variant that does not exist and the
//! build fails. `spike-profile` has asserted that property in a comment since step 01, where nothing
//! could test it — pins were written out longhand, so a bad one was simply never written.

use crate::expand::{suffixed, take_attrs, upper_camel};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::Parser as _;
use syn::spanned::Spanned;
use syn::{Ident, ImplItem, ItemImpl, Path, Token, Type, parse2, punctuated::Punctuated};

/// One `#[rule(pins(a, b))]` method.
struct Rule {
    /// The method, and the rule's stable name.
    ident: Ident,
    /// `pins(email)` → `[Email]`, as field-id variants.
    pins: Vec<Ident>,
}

pub(crate) fn expand(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let entity = parse_entity(attr)?;
    let mut item: ItemImpl = parse2(item)?;

    if let Some((_, path, _)) = &item.trait_ {
        return Err(syn::Error::new(
            path.span(),
            "`#[bolted::rules]` goes on an inherent impl block; it emits the trait impl itself",
        ));
    }
    let Type::Path(self_ty) = item.self_ty.as_ref() else {
        return Err(syn::Error::new(
            item.self_ty.span(),
            "`#[bolted::rules]` expects `impl <Entity>Draft`",
        ));
    };
    let draft = self_ty.path.clone();

    let field_id = suffixed(&entity, "Field");
    let rule_set = suffixed(&entity, "Rules");
    let rules = parse_rules(&mut item)?;

    if rules.is_empty() {
        return Err(syn::Error::new(
            item.span(),
            "a `#[bolted::rules]` block with no `#[rule(..)]` method: drop the block, and the \
             `rules` argument from `#[bolted::entity]`",
        ));
    }

    let collect = rules.iter().map(|r| {
        let (method, name) = (&r.ident, r.ident.to_string());
        let pins = &r.pins;
        quote! {
            if let Err(error) = self.#method() {
                out.push(::bolted_core::RuleViolation {
                    rule: #name,
                    pins: vec![#(#field_id::#pins),*],
                    error,
                });
            }
        }
    });

    Ok(quote! {
        #item

        impl #rule_set for #draft {
            fn rules(&self) -> ::std::vec::Vec<::bolted_core::RuleViolation<#field_id>> {
                let mut out = ::std::vec::Vec::new();
                #(#collect)*
                out
            }
        }
    })
}

/// `#[bolted::rules(entity = Profile)]`.
///
/// The entity is named rather than derived from `ProfileDraft` by stripping `"Draft"`: a macro that
/// guesses a type's name from another type's spelling is a macro that breaks on the first feature
/// called `Redraft`.
fn parse_entity(attr: TokenStream2) -> syn::Result<Ident> {
    let span = attr.span();
    let metas = Punctuated::<syn::Meta, Token![,]>::parse_terminated.parse2(attr)?;
    for meta in metas {
        if let syn::Meta::NameValue(nv) = &meta
            && nv.path.is_ident("entity")
            && let syn::Expr::Path(p) = &nv.value
            && let Some(ident) = p.path.get_ident()
        {
            return Ok(ident.clone());
        }
    }
    Err(syn::Error::new(
        span,
        "`#[bolted::rules(entity = Profile)]` — name the entity whose field ids the rules pin to",
    ))
}

fn parse_rules(item: &mut ItemImpl) -> syn::Result<Vec<Rule>> {
    let mut out = Vec::new();
    for member in item.items.iter_mut() {
        let ImplItem::Fn(f) = member else { continue };
        let declared = take_attrs(&mut f.attrs, "rule");
        let attr = match declared.as_slice() {
            [] => continue,
            [attr] => attr,
            [_, second, ..] => {
                return Err(syn::Error::new(
                    second.span(),
                    "at most one `#[rule(..)]` per method",
                ));
            }
        };

        let mut pins = Vec::new();
        attr.parse_nested_meta(|m| {
            if !m.path.is_ident("pins") {
                return Err(m.error("expected `pins(field, ..)`"));
            }
            let inner = m.input;
            let content;
            syn::parenthesized!(content in inner);
            for path in Punctuated::<Path, Token![,]>::parse_terminated(&content)? {
                let Some(ident) = path.get_ident() else {
                    return Err(syn::Error::new(path.span(), "`pins` takes field names"));
                };
                pins.push(upper_camel(ident));
            }
            Ok(())
        })?;

        if pins.is_empty() {
            return Err(syn::Error::new(
                attr.span(),
                "a rule must pin its error to at least one field, or no shell can show it",
            ));
        }
        out.push(Rule {
            ident: f.sig.ident.clone(),
            pins,
        });
    }
    Ok(out)
}
