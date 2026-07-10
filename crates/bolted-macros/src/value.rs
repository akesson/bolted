//! `#[bolted_macros::value]` — tier 1, the parse-don't-validate boundary (D20).
//!
//! Sanitize, then validate, then wrap. The macro's whole job is to turn a declaration into the three
//! things a hand-written value type repeats: the newtype, the keyed error enum, and the
//! `From<Error> for ErrorData` bridge. Nothing here decides *what is valid* — the length comparison
//! and the user's `custom` predicate do that, and both are ordinary code the compiler checks.
//!
//! Since step 10 the *declaration* lives in `bolted-decl`; this file is emission only. In particular
//! [`bolted_decl::ValueDecl::error_variants`] decides which variants exist, because `bolted-ffi-gen`
//! has to reach the same answer (D25).

use bolted_decl::{ErrorVariant, ParamTy, Sanitizer, Validator, ValueDecl};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

pub(crate) fn expand(_attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let decl = ValueDecl::parse(item)?;

    let name = &decl.name;
    let raw = &decl.raw;
    let vis = &decl.vis;
    let error = decl.error_ident();
    let attrs = &decl.item.attrs;

    let sanitize = decl.sanitizers.iter().map(|s| match s {
        Sanitizer::Trim => quote!(let __raw = __raw.trim().to_owned();),
        Sanitizer::Lowercase => quote!(let __raw = __raw.to_lowercase();),
    });

    let needs_len = decl
        .validators
        .iter()
        .any(|v| matches!(v, Validator::LenChars { .. }));
    let len_binding = needs_len.then(|| quote!(let __len = __raw.chars().count() as u32;));

    let checks = decl.validators.iter().map(|v| match v {
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

    let variants = decl.error_variants();
    let error_variants = variants.iter().map(declare_variant);
    let error_arms = variants.iter().map(|v| error_arm(&error, v));

    let constraints = decl.validators.iter().map(|v| match v {
        Validator::LenChars { min, max } => {
            quote!(::bolted_core::Constraint::LenChars { min: #min, max: #max })
        }
        Validator::Custom { constraint, .. } => {
            quote!(::bolted_core::Constraint::Custom(#constraint))
        }
    });

    let as_str = decl.is_text().then(|| {
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

fn param_ty(ty: ParamTy) -> TokenStream2 {
    match ty {
        ParamTy::U32 => quote!(u32),
    }
}

/// `TooShort { min: u32, actual: u32 }`, or a bare `InvalidChars`.
fn declare_variant(v: &ErrorVariant) -> TokenStream2 {
    let ident = &v.ident;
    if v.params.is_empty() {
        return quote!(#ident);
    }
    let fields = v.params.iter().map(|(name, ty)| {
        let (name, ty) = (format_ident!("{name}"), param_ty(*ty));
        quote!(#name: #ty)
    });
    quote!(#ident { #(#fields),* })
}

/// One arm of `From<Error> for ErrorData`. Pure name-stamping: a variant becomes a key, its named
/// fields become params. This block is the most repetitive thing in a hand-written value type, and
/// generating it is most of why `#[bolted::value]` pays for itself.
fn error_arm(error: &syn::Ident, v: &ErrorVariant) -> TokenStream2 {
    let (ident, key) = (&v.ident, &v.key);
    if v.params.is_empty() {
        return quote!(#error::#ident => ::bolted_core::ErrorData::new(#key),);
    }
    let binds = v.params.iter().map(|(n, _)| format_ident!("{n}"));
    let params = v.params.iter().map(|(n, _)| {
        let bind = format_ident!("{n}");
        quote!((#n, ::std::string::ToString::to_string(&#bind)))
    });
    quote! {
        #error::#ident { #(#binds),* } => ::bolted_core::ErrorData {
            key: #key,
            params: vec![#(#params,)*],
        },
    }
}
