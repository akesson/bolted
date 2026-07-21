//! `#[bolted_macros::rules]` — tier 2, the relational rules of one entity.
//!
//! A rule is an ordinary private method returning `Result<(), ErrorData>`. The macro's entire
//! contribution is to wrap each `Err` in a `RuleViolation` carrying the rule's name and the field
//! ids it pins to, and to collect them in declaration order. It never decides what a rule *means*.
//!
//! `pins(email)` becomes `ProfileField::Email`, so a typo names a variant that does not exist and the
//! build fails. `fixture-profile` has asserted that property in a comment since step 01, where nothing
//! could test it — pins were written out longhand, so a bad one was simply never written.
//!
//! Since step 10 the declaration is parsed by `bolted-decl` (D25); this file is emission only.

use bolted_decl::RulesDecl;
use bolted_decl::naming::suffixed;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

pub(crate) fn expand(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let decl = RulesDecl::parse(attr, item)?;
    let (item, draft) = (&decl.item, &decl.draft);

    let field_id = suffixed(&decl.entity, "Field");
    let rule_set = suffixed(&decl.entity, "Rules");

    let collect = decl.rules.iter().map(|r| {
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
