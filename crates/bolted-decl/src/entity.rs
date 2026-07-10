//! The `#[bolted::entity]` declaration: the entity, and the draft that edits one.

use crate::naming::{take_attrs, upper_camel};
use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use syn::spanned::Spanned;
use syn::{Fields, Ident, ItemStruct, LitStr, Type, Visibility, parse2};

/// A declared field, plus the async check pinned to it (if any).
pub struct EntityField {
    pub ident: Ident,
    pub ty: Type,
    /// `username` → `Username`, the field-id variant.
    pub variant: Ident,
    pub check: Option<Check>,
}

impl EntityField {
    /// The value type's name, when it is a plain path (`Username`, `DateRange`). The FFI generator
    /// uses it to look the field's value declaration up; a field typed with anything more exotic than
    /// a path has no declaration to find.
    pub fn value_ident(&self) -> Option<&Ident> {
        match &self.ty {
            Type::Path(p) => p.path.get_ident(),
            _ => None,
        }
    }
}

/// `#[check(rule = "…", pending_key = "…", required_key = "…")]`
pub struct Check {
    /// The stable rule name a violation reports under.
    pub rule: String,
    /// `username_unique` → `UsernameUnique`, the `CheckId` variant.
    pub variant: Ident,
    /// The private `SingleFlight` field on the draft.
    pub slot: Ident,
    pub pending_key: String,
    pub required_key: String,
}

/// A parsed `#[bolted::entity]` struct.
pub struct EntityDecl {
    /// The struct with `#[check(..)]` stripped from its fields.
    pub item: ItemStruct,
    pub name: Ident,
    pub vis: Visibility,
    /// `#[bolted::entity(rules)]` — a `#[bolted::rules]` impl block exists for this entity.
    pub has_rules: bool,
    pub fields: Vec<EntityField>,
}

impl EntityDecl {
    pub fn parse(attr: TokenStream2, item: TokenStream2) -> syn::Result<Self> {
        Self::from_item(parse_entity_attr(attr)?, parse2(item)?)
    }

    pub fn from_item(has_rules: bool, mut item: ItemStruct) -> syn::Result<Self> {
        let fields = parse_fields(&mut item)?;
        Ok(EntityDecl {
            name: item.ident.clone(),
            vis: item.vis.clone(),
            has_rules,
            fields,
            item,
        })
    }

    pub fn checks(&self) -> Vec<&Check> {
        self.fields
            .iter()
            .filter_map(|f| f.check.as_ref())
            .collect()
    }
}

/// `#[bolted::entity]` or `#[bolted::entity(rules)]`.
pub fn parse_entity_attr(attr: TokenStream2) -> syn::Result<bool> {
    if attr.is_empty() {
        return Ok(false);
    }
    let ident: Ident = parse2(attr)?;
    if ident == "rules" {
        Ok(true)
    } else {
        Err(syn::Error::new(
            ident.span(),
            "the only argument is `rules`, meaning a `#[bolted::rules]` impl block exists",
        ))
    }
}

fn parse_fields(item: &mut ItemStruct) -> syn::Result<Vec<EntityField>> {
    let Fields::Named(named) = &mut item.fields else {
        return Err(syn::Error::new(
            item.span(),
            "`#[bolted::entity]` declares a struct with named fields",
        ));
    };
    if named.named.is_empty() {
        return Err(syn::Error::new(
            item.span(),
            "an entity with no fields has no draft to edit",
        ));
    }

    let mut out = Vec::new();
    for field in named.named.iter_mut() {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new(field.span(), "a named field is required"));
        };
        let declared = take_attrs(&mut field.attrs, "check");
        let check = match declared.as_slice() {
            [] => None,
            [attr] => Some(parse_check(attr, &ident)?),
            [_, second, ..] => {
                return Err(syn::Error::new(
                    second.span(),
                    "at most one `#[check(..)]` per field",
                ));
            }
        };
        out.push(EntityField {
            variant: upper_camel(&ident),
            ty: field.ty.clone(),
            ident,
            check,
        });
    }
    Ok(out)
}

fn parse_check(attr: &syn::Attribute, field: &Ident) -> syn::Result<Check> {
    let (mut rule, mut pending_key, mut required_key) = (None, None, None);
    attr.parse_nested_meta(|m| {
        let target = if m.path.is_ident("rule") {
            &mut rule
        } else if m.path.is_ident("pending_key") {
            &mut pending_key
        } else if m.path.is_ident("required_key") {
            &mut required_key
        } else {
            return Err(m.error("expected `rule`, `pending_key` or `required_key`"));
        };
        *target = Some(m.value()?.parse::<LitStr>()?.value());
        Ok(())
    })?;

    let (Some(rule), Some(pending_key), Some(required_key)) = (rule, pending_key, required_key)
    else {
        // All three are l10n keys that shells already ship. Defaulting them from the field name
        // would move a translation key on a rename, silently.
        return Err(syn::Error::new(
            attr.span(),
            "`#[check(..)]` needs `rule`, `pending_key` and `required_key` — they are stable \
             localisation keys, and a macro must not invent them",
        ));
    };

    // `format_ident!` panics on a string that is not an identifier, and a rule name is a *string* the
    // user chose. Parse it, so a bad one is a `compile_error!` rather than an ICE-shaped panic.
    let rule_ident: Ident = syn::parse_str(&rule).map_err(|_| {
        syn::Error::new(
            attr.span(),
            "a `rule` name must be a valid identifier: it becomes a `CheckId` variant",
        )
    })?;

    Ok(Check {
        variant: upper_camel(&rule_ident),
        slot: format_ident!("{}_check", field, span = field.span()),
        rule,
        pending_key,
        required_key,
    })
}
