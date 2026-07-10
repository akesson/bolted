//! The `#[bolted::rules]` declaration: tier 2, the relational rules of one entity.

use crate::naming::{take_attrs, upper_camel};
use proc_macro2::TokenStream as TokenStream2;
use syn::parse::Parser as _;
use syn::spanned::Spanned;
use syn::{Ident, ImplItem, ItemImpl, Path, Token, Type, parse2, punctuated::Punctuated};

/// One `#[rule(pins(a, b))]` method.
pub struct Rule {
    /// The method, and the rule's stable name.
    pub ident: Ident,
    /// `pins(email)` → `[Email]`, as field-id variants.
    pub pins: Vec<Ident>,
}

/// A parsed `#[bolted::rules(entity = Profile)]` impl block.
pub struct RulesDecl {
    /// The impl block with `#[rule(..)]` stripped from its methods.
    pub item: ItemImpl,
    pub entity: Ident,
    /// The type the block is written on: `ProfileDraft`.
    pub draft: Path,
    pub rules: Vec<Rule>,
}

impl RulesDecl {
    pub fn parse(attr: TokenStream2, item: TokenStream2) -> syn::Result<Self> {
        Self::from_item(parse_entity(attr)?, parse2(item)?)
    }

    pub fn from_item(entity: Ident, mut item: ItemImpl) -> syn::Result<Self> {
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
        let rules = parse_rules(&mut item)?;

        if rules.is_empty() {
            return Err(syn::Error::new(
                item.span(),
                "a `#[bolted::rules]` block with no `#[rule(..)]` method: drop the block, and the \
                 `rules` argument from `#[bolted::entity]`",
            ));
        }
        Ok(RulesDecl {
            item,
            entity,
            draft,
            rules,
        })
    }
}

/// `#[bolted::rules(entity = Profile)]`.
///
/// The entity is named rather than derived from `ProfileDraft` by stripping `"Draft"`: a macro that
/// guesses a type's name from another type's spelling is a macro that breaks on the first feature
/// called `Redraft`.
pub fn parse_entity(attr: TokenStream2) -> syn::Result<Ident> {
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
