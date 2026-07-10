//! `#[bolted_macros::entity]` — the entity, and the draft that edits one.
//!
//! Read the emitted code as a table, not as a program. Every block below is one line per declared
//! field, and the line is always the same line. Where a judgement has to be made — is this field's
//! input an error? may this draft commit? does an unrun check block? — the emitted code calls a
//! generic in `bolted-core` and does not make the judgement itself.
//!
//! Three exceptions to "one line per field" exist, and each is a bug this project has already had:
//!
//! - `is_based()` **ORs over every field**. A single-field answer passes 21 of the 22 conformance
//!   invariants (step 08, verified by mutation) and silently overwrites the server.
//! - `dirty_fields()` / `conflicts()` push in **declaration order**, which is observable.
//! - Every mutation that can move a checked field's value is wrapped in **one** generated guard, so
//!   no path can skip C13's verdict reset. `spike-profile` needed this on five call sites; a macro
//!   that emitted the reset per call site would drop it from the sixth. *"Can move"* is read as the
//!   compiler reads it: `try_set_name` cannot touch `username`, so it is not guarded, and does not
//!   clone a `Username` on every keystroke of the name box (see `setters`).

use crate::expand::{suffixed, take_attrs, upper_camel};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Fields, Ident, ItemStruct, LitStr, Type, Visibility, parse2};

/// A declared field, plus the async check pinned to it (if any).
struct EntityField {
    ident: Ident,
    ty: Type,
    /// `username` → `Username`, the field-id variant.
    variant: Ident,
    check: Option<Check>,
}

/// `#[check(rule = "…", pending_key = "…", required_key = "…")]`
struct Check {
    /// The stable rule name a violation reports under.
    rule: String,
    /// `username_unique` → `UsernameUnique`, the `CheckId` variant.
    variant: Ident,
    /// The private `SingleFlight` field on the draft.
    slot: Ident,
    pending_key: String,
    required_key: String,
}

pub(crate) fn expand(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let has_rules = parse_entity_attr(attr)?;
    let mut item: ItemStruct = parse2(item)?;
    let fields = parse_fields(&mut item)?;

    let entity = item.ident.clone();
    let vis = item.vis.clone();
    let field_id = suffixed(&entity, "Field");
    let check_id = suffixed(&entity, "Check");
    let draft = suffixed(&entity, "Draft");
    let stash = suffixed(&entity, "Stash");
    let store = suffixed(&entity, "Store");
    let rule_set = suffixed(&entity, "Rules");

    let checks: Vec<&Check> = fields.iter().filter_map(|f| f.check.as_ref()).collect();

    let entity_decl = entity_decl(&item, &fields);
    let field_enum = field_enum(&vis, &field_id, &fields);
    let check_enum = check_enum(&vis, &check_id, &checks);
    let stash_decl = stash_decl(&vis, &stash, &fields);
    let draft_decl = draft_decl(&vis, &draft, &fields, &checks);
    let setters = setters(&draft, &fields);
    let guard = guard(&draft, &fields);
    let rule_set_decl = rule_set_decl(&vis, &rule_set, &field_id, &draft, has_rules);
    let draft_impl = draft_impl(&entity, &draft, &field_id, &rule_set, &fields);
    let store_draft_impl = store_draft_impl(&entity, &draft, &fields, &checks);
    let stashable_impl = stashable_impl(&draft, &stash, &fields, &checks);
    let checked_impl = checked_impl(&draft, &check_id, &field_id, &fields);

    Ok(quote! {
        #entity_decl
        #field_enum
        #check_enum
        #stash_decl
        #draft_decl

        #vis type #store = ::bolted_core::Store<#draft>;

        #guard
        #setters
        #rule_set_decl
        #draft_impl
        #store_draft_impl
        #stashable_impl
        #checked_impl
    })
}

/// `#[bolted::entity]` or `#[bolted::entity(rules)]`.
fn parse_entity_attr(attr: TokenStream2) -> syn::Result<bool> {
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

// =================================================================================================
// declarations
// =================================================================================================

/// The entity, with `#[check(..)]` stripped. Always-valid canonical state: every field holds a
/// parsed `Value`, so an entity that exists is an entity that was valid when it was committed.
fn entity_decl(item: &ItemStruct, fields: &[EntityField]) -> TokenStream2 {
    let attrs = &item.attrs;
    let vis = &item.vis;
    let name = &item.ident;
    let decls = fields.iter().map(|f| {
        let (ident, ty) = (&f.ident, &f.ty);
        quote!(pub #ident: #ty)
    });
    quote! {
        #(#attrs)*
        #[derive(Debug, Clone, PartialEq)]
        #vis struct #name { #(#decls,)* }
    }
}

/// Typed field identifiers. Rule errors pin to these, so pinning a nonexistent field cannot compile.
fn field_enum(vis: &Visibility, field_id: &Ident, fields: &[EntityField]) -> TokenStream2 {
    let variants = fields.iter().map(|f| &f.variant);
    let arms = fields.iter().map(|f| {
        let (variant, ty) = (&f.variant, &f.ty);
        quote!(#field_id::#variant => <#ty as ::bolted_core::Value>::constraints())
    });
    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #vis enum #field_id { #(#variants,)* }

        impl #field_id {
            /// `Required` is prepended because every entity field is non-optional — a *field*-level
            /// judgement no value type can make (D13) — followed by the value type's own intrinsics.
            pub fn constraints(self) -> ::std::vec::Vec<::bolted_core::Constraint> {
                let mut out = vec![::bolted_core::Constraint::Required];
                let intrinsic: &'static [::bolted_core::Constraint] = match self { #(#arms,)* };
                out.extend_from_slice(intrinsic);
                out
            }
        }
    }
}

/// Typed check identifiers (D18). Emitted only when the entity declares a check; a feature without
/// one has no `CheckId` and does not implement `Checked`.
fn check_enum(vis: &Visibility, check_id: &Ident, checks: &[&Check]) -> Option<TokenStream2> {
    if checks.is_empty() {
        return None;
    }
    let variants = checks.iter().map(|c| &c.variant);
    Some(quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #vis enum #check_id { #(#variants,)* }
    })
}

/// The serializable projection: per field `{raw, base}`, plus the whole-draft bits.
///
/// Note `FieldStash<<Username as Value>::Raw>`. That projection **is** D19's answer: three fields
/// with `Raw = String` collapse to one `FieldStash<String>` because the generic is keyed on the raw
/// type, and the compiler does it. There is no dedup pass to write.
///
/// `sync` is absent, and so is any async verdict (C20). Both would be lies on the far side of a
/// process death.
fn stash_decl(vis: &Visibility, stash: &Ident, fields: &[EntityField]) -> TokenStream2 {
    let decls = fields.iter().map(|f| {
        let (ident, ty) = (&f.ident, &f.ty);
        quote!(pub #ident: ::bolted_core::FieldStash<<#ty as ::bolted_core::Value>::Raw>)
    });
    quote! {
        #[derive(Debug, Clone, PartialEq)]
        #vis struct #stash {
            #(#decls,)*
            /// The store version this draft was last based on.
            pub base_version: u64,
            /// A draft orphaned before the process died stays orphaned (C11).
            pub orphaned: bool,
        }
    }
}

fn draft_decl(
    vis: &Visibility,
    draft: &Ident,
    fields: &[EntityField],
    checks: &[&Check],
) -> TokenStream2 {
    let decls = fields.iter().map(|f| {
        let (ident, ty) = (&f.ident, &f.ty);
        quote!(pub #ident: ::bolted_core::Field<#ty>)
    });
    let slots = checks.iter().map(|c| {
        let slot = &c.slot;
        quote!(#slot: ::bolted_core::SingleFlight<::core::result::Result<(), ::bolted_core::ErrorData>>)
    });
    quote! {
        #vis struct #draft {
            #(#decls,)*
            #(#slots,)*
            status: ::bolted_core::DraftStatus,
            base_version: u64,
        }
    }
}

// =================================================================================================
// the guard — C13, once, for every mutation path
// =================================================================================================

/// Run a mutation, then reset every check whose pinned field's **value** moved.
///
/// By value comparison, not by call site. `value()` is `None` for `Unset`/`Invalid`, which gets every
/// case right at once: edit-to-different, edit-to-invalid, rebase-adopt and take-theirs all move the
/// value and reset; edit-to-same, keep-mine, and a conflict that preserves your value leave the
/// verdict standing. That is C13 exactly, and it is why the reset is not written per mutation.
///
/// With no checks declared, this is `f(self)` — the identity — and the setters below still route
/// through it, so adding a check to a feature later cannot miss a path.
fn guard(draft: &Ident, fields: &[EntityField]) -> TokenStream2 {
    let checked: Vec<&EntityField> = fields.iter().filter(|f| f.check.is_some()).collect();

    // No checks: the guard is the identity. Written as `f(self)` and not `let x = f(self); x`,
    // because the latter is `clippy::let_and_return` and generated code must be clippy-clean under
    // `-D warnings` like any other.
    if checked.is_empty() {
        return quote! {
            impl #draft {
                fn bolted_guard<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
                    f(self)
                }
            }
        };
    }

    let befores = checked.iter().map(|f| {
        let (ident, before) = (&f.ident, before_ident(&f.ident));
        quote!(let #before = self.#ident.value().cloned();)
    });
    let resets = checked.iter().map(|f| {
        let ident = &f.ident;
        let before = before_ident(ident);
        let slot = f.check.as_ref().map(|c| &c.slot);
        quote! {
            if self.#ident.value() != #before.as_ref() { self.#slot.reset(); }
        }
    });

    quote! {
        impl #draft {
            fn bolted_guard<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
                #(#befores)*
                let __out = f(self);
                #(#resets)*
                __out
            }
        }
    }
}

fn before_ident(field: &Ident) -> Ident {
    format_ident!("__before_{}", field, span = field.span())
}

/// One monomorphic setter per field — generic methods cannot cross a language boundary (§5).
///
/// A setter is guarded **iff its own field carries a check**. `try_set_name` cannot move `username`'s
/// value, so routing it through the guard would clone the username on every keystroke and compare it
/// with itself — on the exact path step 07's kill criterion 4 measures, and the path the whole
/// "core validates every keystroke" bet rests on. The resolvers and `rebase` are guarded
/// unconditionally, because they take a field id at *runtime* or move every field at once.
///
/// This is not a case of a macro deciding something. Which fields carry a check is written in the
/// declaration, and regeneration cannot forget a path the way a hand-written call site can.
fn setters(draft: &Ident, fields: &[EntityField]) -> TokenStream2 {
    let setters = fields.iter().map(|f| {
        let (ident, ty) = (&f.ident, &f.ty);
        let name = format_ident!("try_set_{}", ident, span = ident.span());
        let body = if f.check.is_some() {
            quote!(self.bolted_guard(|__d| __d.#ident.try_set(raw)))
        } else {
            quote!(self.#ident.try_set(raw))
        };
        quote! {
            pub fn #name(
                &mut self,
                raw: <#ty as ::bolted_core::Value>::Raw,
            ) -> ::core::result::Result<(), <#ty as ::bolted_core::Value>::Error> {
                #body
            }
        }
    });
    quote!(impl #draft { #(#setters)* })
}

// =================================================================================================
// the rule set — tier 2
// =================================================================================================

/// The seam between `#[bolted::entity]` and `#[bolted::rules]`.
///
/// With `rules`, the entity declares the trait and `#[bolted::rules]` implements it: forget the impl
/// and the build fails. Without `rules`, the entity implements it as empty: write a rules block
/// anyway and the build fails on a conflicting impl. Both mistakes are caught at rung 2, which is
/// the only reason the flag is acceptable at all.
fn rule_set_decl(
    vis: &Visibility,
    rule_set: &Ident,
    field_id: &Ident,
    draft: &Ident,
    has_rules: bool,
) -> TokenStream2 {
    let empty_impl = (!has_rules).then(|| quote!(impl #rule_set for #draft {}));
    let body = if has_rules {
        quote!(fn rules(&self) -> ::std::vec::Vec<::bolted_core::RuleViolation<#field_id>>;)
    } else {
        quote! {
            fn rules(&self) -> ::std::vec::Vec<::bolted_core::RuleViolation<#field_id>> {
                ::std::vec::Vec::new()
            }
        }
    };
    quote! {
        #[doc(hidden)]
        #vis trait #rule_set { #body }
        #empty_impl
    }
}

// =================================================================================================
// impl Draft
// =================================================================================================

fn draft_impl(
    entity: &Ident,
    draft: &Ident,
    field_id: &Ident,
    rule_set: &Ident,
    fields: &[EntityField],
) -> TokenStream2 {
    // Declaration order, and observably so: a shell that focuses the first invalid field walks this
    // list, and a user reads their form top to bottom.
    let dirty = fields.iter().map(|f| {
        let (ident, variant) = (&f.ident, &f.variant);
        quote!(if self.#ident.is_dirty() { out.push(#field_id::#variant); })
    });
    let conflicts = fields.iter().map(|f| {
        let (ident, variant) = (&f.ident, &f.variant);
        quote!(if self.#ident.is_conflicted() { out.push(#field_id::#variant); })
    });

    // Tier 1: `required_error` makes the `Unset` → `required` judgement, at rung 1.
    let tier1 = fields.iter().map(|f| {
        let (ident, variant) = (&f.ident, &f.variant);
        quote! {
            if let Some(e) = self.#ident.required_error() {
                report.field_errors.push((#field_id::#variant, e));
            }
        }
    });

    // Tier 3's client half: `SingleFlight::violation` folds C13 + C16, at rung 1.
    let check_violations = fields.iter().filter_map(|f| {
        let c = f.check.as_ref()?;
        let (ident, variant, slot) = (&f.ident, &f.variant, &c.slot);
        let (rule, pending, required) = (&c.rule, &c.pending_key, &c.required_key);
        Some(quote! {
            if let Some(v) = self.#slot.violation(
                #rule, #field_id::#variant, self.#ident.is_dirty(), #pending, #required,
            ) {
                report.rule_errors.push(v);
            }
        })
    });

    let keep_mine = fields.iter().map(|f| {
        let (ident, variant) = (&f.ident, &f.variant);
        quote!(#field_id::#variant => __d.#ident.resolve_keep_mine())
    });
    let take_theirs = fields.iter().map(|f| {
        let (ident, variant) = (&f.ident, &f.variant);
        quote!(#field_id::#variant => __d.#ident.resolve_take_theirs())
    });

    let idents: Vec<&Ident> = fields.iter().map(|f| &f.ident).collect();
    let takes = idents.iter().map(|i| quote!(self.#i.value().cloned()));

    quote! {
        impl ::bolted_core::Draft for #draft {
            type Entity = #entity;
            type FieldId = #field_id;

            fn status(&self) -> ::bolted_core::DraftStatus { self.status }
            fn base_version(&self) -> u64 { self.base_version }

            fn dirty_fields(&self) -> ::std::vec::Vec<#field_id> {
                let mut out = ::std::vec::Vec::new();
                #(#dirty)*
                out
            }

            fn conflicts(&self) -> ::std::vec::Vec<#field_id> {
                let mut out = ::std::vec::Vec::new();
                #(#conflicts)*
                out
            }

            fn validate(&self) -> ::bolted_core::ValidationReport<#field_id> {
                let mut report = ::bolted_core::ValidationReport::new();
                #(#tier1)*
                report.rule_errors.extend(#rule_set::rules(self));
                #(#check_violations)*
                report
            }

            fn resolve_keep_mine(&mut self, field: #field_id) {
                self.bolted_guard(|__d| match field { #(#keep_mine,)* })
            }

            fn resolve_take_theirs(&mut self, field: #field_id) {
                self.bolted_guard(|__d| match field { #(#take_theirs,)* })
            }

            fn commit(self)
                -> ::core::result::Result<#entity, (Self, ::bolted_core::CommitError<#field_id>)>
            {
                // C07's three gates, in order, decided once at rung 1.
                if let Some(e) = ::bolted_core::commit_gates(&self) {
                    return Err((self, e));
                }
                // The gates guarantee every field is `Valid`. Values are cloned rather than moved
                // out: dismembering `self` before the last fallible step would leave nothing to hand
                // back, and `commit` promises the draft back on every failure (F3).
                match (#(#takes,)*) {
                    (#(Some(#idents),)*) => Ok(#entity { #(#idents,)* }),
                    _ => {
                        let report = <Self as ::bolted_core::Draft>::validate(&self);
                        Err((self, ::bolted_core::CommitError::Validation(report)))
                    }
                }
            }
        }
    }
}

// =================================================================================================
// impl StoreDraft
// =================================================================================================

fn store_draft_impl(
    entity: &Ident,
    draft: &Ident,
    fields: &[EntityField],
    checks: &[&Check],
) -> TokenStream2 {
    let from_base = fields.iter().map(|f| {
        let ident = &f.ident;
        quote!(#ident: ::bolted_core::Field::from_base(__e.#ident.clone()))
    });
    let unset = fields.iter().map(|f| {
        let ident = &f.ident;
        quote!(#ident: ::bolted_core::Field::new_unset())
    });
    let rebases = fields.iter().map(|f| {
        let ident = &f.ident;
        quote!(__d.#ident.rebase(entity.#ident.clone());)
    });
    // C12. **Every** field, ORed. A draft that retains an ancestor anywhere is entity-backed: it
    // rebases, and it orphans. Consulting one field is invisible to 21 of the 22 invariants and lets
    // a stale edit silently overwrite the server (step 08).
    let bases = fields.iter().map(|f| {
        let ident = &f.ident;
        quote!(self.#ident.base().is_some())
    });
    let fresh = checks.iter().map(|c| {
        let slot = &c.slot;
        quote!(#slot: ::bolted_core::SingleFlight::new())
    });
    let fresh2 = fresh.clone();

    quote! {
        impl ::bolted_core::StoreDraft for #draft {
            fn from_canonical(base: Option<&#entity>, base_version: u64) -> Self {
                match base {
                    // Every field clones, uniformly — which is only possible because value objects
                    // are not `Copy` (D8).
                    Some(__e) => #draft {
                        #(#from_base,)*
                        #(#fresh,)*
                        status: ::bolted_core::DraftStatus::Live,
                        base_version,
                    },
                    None => #draft {
                        #(#unset,)*
                        #(#fresh2,)*
                        status: ::bolted_core::DraftStatus::Live,
                        base_version,
                    },
                }
            }

            fn rebase(&mut self, entity: &#entity, version: u64) {
                if matches!(self.status, ::bolted_core::DraftStatus::Orphaned) {
                    return; // orphan is terminal, and the draft is based on no canonical at all
                }
                self.bolted_guard(|__d| { #(#rebases)* });
                self.base_version = version;
            }

            fn orphan(&mut self) {
                self.status = ::bolted_core::DraftStatus::Orphaned;
            }

            fn is_based(&self) -> bool {
                #(#bases)||*
            }
        }
    }
}

// =================================================================================================
// impl Stashable
// =================================================================================================

fn stashable_impl(
    draft: &Ident,
    stash: &Ident,
    fields: &[EntityField],
    checks: &[&Check],
) -> TokenStream2 {
    let out = fields.iter().map(|f| {
        let ident = &f.ident;
        quote!(#ident: self.#ident.stash())
    });
    let back = fields.iter().map(|f| {
        let ident = &f.ident;
        quote!(#ident: ::bolted_core::Field::from_stash(&stash.#ident))
    });
    // The verdict deliberately does not survive: it endorses a value against a server state that may
    // have moved while the process was dead. C13 + C16 then make the restored draft safe (C20).
    let fresh = checks.iter().map(|c| {
        let slot = &c.slot;
        quote!(#slot: ::bolted_core::SingleFlight::new())
    });

    quote! {
        impl ::bolted_core::Stashable for #draft {
            type Stash = #stash;

            fn stash(&self) -> #stash {
                #stash {
                    #(#out,)*
                    base_version: self.base_version,
                    orphaned: matches!(self.status, ::bolted_core::DraftStatus::Orphaned),
                }
            }

            fn from_stash(stash: &#stash) -> Self {
                #draft {
                    #(#back,)*
                    #(#fresh,)*
                    status: if stash.orphaned {
                        ::bolted_core::DraftStatus::Orphaned
                    } else {
                        ::bolted_core::DraftStatus::Live
                    },
                    base_version: stash.base_version,
                }
            }
        }
    }
}

// =================================================================================================
// impl Checked (D18)
// =================================================================================================

/// Every arm is a one-line delegation to the `SingleFlight` that owns the sequencing. Emitted only
/// when the entity declares a check.
fn checked_impl(
    draft: &Ident,
    check_id: &Ident,
    field_id: &Ident,
    fields: &[EntityField],
) -> Option<TokenStream2> {
    let checked: Vec<(&EntityField, &Check)> = fields
        .iter()
        .filter_map(|f| f.check.as_ref().map(|c| (f, c)))
        .collect();
    if checked.is_empty() {
        return None;
    }

    let begin = checked.iter().map(|(_, c)| {
        let (variant, slot) = (&c.variant, &c.slot);
        quote!(#check_id::#variant => self.#slot.begin())
    });
    let complete = checked.iter().map(|(_, c)| {
        let (variant, slot) = (&c.variant, &c.slot);
        quote!(#check_id::#variant => self.#slot.complete(token, verdict))
    });
    let state = checked.iter().map(|(_, c)| {
        let (variant, slot) = (&c.variant, &c.slot);
        quote!(#check_id::#variant => self.#slot.state())
    });
    let pins = checked.iter().map(|(f, c)| {
        let (variant, field) = (&c.variant, &f.variant);
        quote!(#check_id::#variant => #field_id::#field)
    });

    Some(quote! {
        impl ::bolted_core::Checked for #draft {
            type CheckId = #check_id;

            fn begin_check(&mut self, check: #check_id) -> ::bolted_core::CheckToken {
                match check { #(#begin,)* }
            }

            fn complete_check(
                &mut self,
                check: #check_id,
                token: ::bolted_core::CheckToken,
                verdict: ::core::result::Result<(), ::bolted_core::ErrorData>,
            ) -> bool {
                match check { #(#complete,)* }
            }

            fn check_state(
                &self,
                check: #check_id,
            ) -> &::bolted_core::CheckState<::core::result::Result<(), ::bolted_core::ErrorData>> {
                match check { #(#state,)* }
            }

            fn check_pins(check: #check_id) -> #field_id {
                match check { #(#pins,)* }
            }
        }
    })
}
