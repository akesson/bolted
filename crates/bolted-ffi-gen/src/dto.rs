//! The monomorphic `#[data]` / `#[error]` projection of one feature.
//!
//! `#[data]` forbids generics, tuples, borrowed data and `&'static str`, so everything generic in the
//! core is stamped into a concrete, owned shape. What is *not* stamped per feature lives in
//! `bolted-ffi`: `ErrorData`, `ConstraintFfi`, `DraftStatusFfi`, `CheckStateFfi`, and the whole
//! `Text*` family (D24). Only types that mention this feature's `FieldId` are emitted here.

use crate::field::FieldProj;
use bolted_decl::naming::suffixed;
use bolted_decl::{Feature, ParamTy, ValueDecl};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;

pub fn field_id_enum(feature: &Feature) -> TokenStream2 {
    let id = suffixed(&feature.entity.name, "FieldId");
    let core = &feature.entity.name;
    let core_field = suffixed(core, "Field");
    let variants: Vec<&Ident> = feature.entity.fields.iter().map(|f| &f.variant).collect();

    quote! {
        /// Mirrors the core's field id. Declaration order, which is observable: a shell focusing the
        /// first invalid field walks `dirty_fields()` in this order.
        #[data]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum #id { #(#variants,)* }

        fn to_field_id(f: #core_field) -> #id {
            match f { #(#core_field::#variants => #id::#variants,)* }
        }

        fn to_core_field(f: #id) -> #core_field {
            match f { #(#id::#variants => #core_field::#variants,)* }
        }
    }
}

fn param_ty(ty: ParamTy) -> TokenStream2 {
    match ty {
        ParamTy::U32 => quote!(u32),
    }
}

/// One `#[error]` enum per declared value, plus the `From<CoreError>` bridge and the three trait impls
/// BoltFFI needs and does not synthesise (`Display`, `Error`, `From<UnexpectedFfiCallbackError>`).
///
/// The variants come from [`ValueDecl::error_variants`], the same call `bolted-macros` makes when it
/// emits `UsernameError` (D25). A second `match` here would eventually disagree with it.
pub fn value_error(value: &ValueDecl) -> TokenStream2 {
    let core_error = value.error_ident();
    let ffi_error = format_ident!("{}ErrorFfi", value.name);
    let variants = value.error_variants();

    let declared = variants.iter().map(|v| {
        let ident = &v.ident;
        if v.params.is_empty() {
            return quote!(#ident);
        }
        let fields = v.params.iter().map(|(n, ty)| {
            let (n, ty) = (format_ident!("{n}"), param_ty(*ty));
            quote!(#n: #ty)
        });
        quote!(#ident { #(#fields),* })
    });

    let arms = variants.iter().map(|v| {
        let ident = &v.ident;
        if v.params.is_empty() {
            return quote!(#core_error::#ident => #ffi_error::#ident,);
        }
        let binds: Vec<Ident> = v.params.iter().map(|(n, _)| format_ident!("{n}")).collect();
        quote!(#core_error::#ident { #(#binds),* } => #ffi_error::#ident { #(#binds),* },)
    });

    let message = format!("invalid {}", value.name.to_string().to_lowercase());

    quote! {
        /// The setter's typed refusal. `DraftClosed` is D23: the draft was submitted (C17) or closed
        /// (C18), and a silent `Ok(())` would be a lie.
        #[error]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum #ffi_error {
            #(#declared,)*
            DraftClosed,
        }

        impl ::std::fmt::Display for #ffi_error {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, concat!(#message, ": {:?}"), self)
            }
        }
        impl ::std::error::Error for #ffi_error {}
        impl ::core::convert::From<UnexpectedFfiCallbackError> for #ffi_error {
            fn from(_: UnexpectedFfiCallbackError) -> Self { #ffi_error::DraftClosed }
        }
        impl ::core::convert::From<#core_error> for #ffi_error {
            fn from(e: #core_error) -> Self {
                match e { #(#arms)* }
            }
        }
    }
}

/// The foreign-implemented capability, one per `#[check(..)]`.
///
/// `Send + Sync` because the draft stores it and calls it from whatever thread drives the check.
/// Synchronous, matching the deterministic single-flight `begin`/`complete`.
pub fn checker_trait(p: &FieldProj<'_>) -> TokenStream2 {
    let trait_name = checker_trait_name(p);
    let wire = p.wire_ty();
    quote! {
        #[export]
        pub trait #trait_name: Send + Sync {
            fn check(&self, value: #wire) -> CheckVerdictFfi;
        }
    }
}

pub fn checker_trait_name(p: &FieldProj<'_>) -> Ident {
    format_ident!("{}Checker", bolted_decl::naming::upper_camel(p.ident()))
}

pub fn snapshot(feature: &Feature, fields: &[FieldProj<'_>]) -> TokenStream2 {
    let snap = suffixed(&feature.entity.name, "Snapshot");
    let id = suffixed(&feature.entity.name, "FieldId");

    let decls = fields.iter().map(|p| {
        let (ident, ty) = (p.ident(), p.state_ty());
        quote!(pub #ident: #ty)
    });
    let checks = fields.iter().filter(|p| p.field.check.is_some()).map(|p| {
        let slot = format_ident!("{}_check", p.ident());
        quote! {
            /// The async check's observable sub-state, so a shell can render a spinner rather than
            /// infer one from a rule violation (step-02 finding 7).
            pub #slot: CheckStateFfi
        }
    });

    quote! {
        /// One draft's whole observable state — the `observe` verb's item, and the store's canonical
        /// stream item.
        #[data]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct #snap {
            #(#decls,)*
            #(#checks,)*
            pub any_dirty: bool,
            pub conflicts: Vec<#id>,
            pub status: DraftStatusFfi,
            /// The draft's `base_version`: a subscriber can version-stamp a `snapshot()`-then-
            /// `subscribe()` sequence and detect a missed event in the gap.
            pub version: u64,
        }
    }
}

pub fn values(feature: &Feature, fields: &[FieldProj<'_>]) -> TokenStream2 {
    let values = suffixed(&feature.entity.name, "Values");
    let decls = fields.iter().map(|p| {
        let (ident, ty) = (p.ident(), p.wire_ty());
        quote!(pub #ident: #ty)
    });
    quote! {
        /// Raw values for seeding or replacing the canonical entity. Each is validated through the
        /// real value types — `apply_canonical` cannot install an invalid entity.
        #[data]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct #values { #(#decls,)* }
    }
}

pub fn stash(feature: &Feature, fields: &[FieldProj<'_>]) -> TokenStream2 {
    let stash = suffixed(&feature.entity.name, "StashFfi");
    let accepted = suffixed(&feature.entity.name, "StashAcceptedFfi");
    let decls = fields.iter().map(|p| {
        let (ident, ty) = (p.ident(), p.stash_ty());
        quote!(pub #ident: #ty)
    });
    quote! {
        /// The schema version this build stamps into every stash it writes, and gates on when it
        /// restores one (D27). It travels *inside* the DTO, so the version a stash was written under
        /// crosses process death with it and `restore` can refuse a stash from a schema this build
        /// no longer accepts — a typed, wholesale refusal, not a silent `null`.
        ///
        /// It is a fixed constant today. Deriving it from the declaration — so a tightened constraint
        /// bumps it and old stashes refuse automatically — is D27's build-time `bolted-check`
        /// constraint-semver event, and it is Phase 4's. The refusal *mechanism* does not wait for
        /// that: it gates on whatever this constant holds.
        pub const STASH_SCHEMA_VERSION: u32 = 1;

        /// What a shell persists so an edit session survives process death (C20). Carries no `sync`
        /// and no async verdict, on purpose: C13 + C16 then make a restored draft safe with no new
        /// invariant. Carries its `schema_version` (D27): the envelope's version gate.
        #[data]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct #stash {
            /// The `STASH_SCHEMA_VERSION` in force when this stash was written. `accept_stash` refuses
            /// a value this build does not recognise (D27) before any field is trusted.
            pub schema_version: u32,
            #(#decls,)*
            pub base_version: u64,
            pub orphaned: bool,
        }

        /// A stash whose envelope this build has **accepted** (D27, parse-don't-validate). The only
        /// way to obtain one is `accept_stash`, which gates the schema version; `restore` takes only
        /// this, so an un-gated stash cannot be restored — the type system carries the proof.
        ///
        /// (It is a distinct wrapper, not a fallible `restore`, because BoltFFI 0.27.3 cannot return
        /// a class handle from a throwing method — see the step-12 upstream filing. Gating into a
        /// `#[data]` token and restoring from it is the shape that fits the toolchain and is stronger
        /// than a bypassable guard.)
        #[data]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct #accepted {
            pub stash: #stash,
        }
    }
}

pub fn report(feature: &Feature) -> TokenStream2 {
    let id = suffixed(&feature.entity.name, "FieldId");
    quote! {
        #[data]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct FieldErrorFfi {
            pub field: #id,
            pub error: ErrorData,
        }

        #[data]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct RuleViolationFfi {
            pub rule: String,
            pub pins: Vec<#id>,
            pub error: ErrorData,
        }

        #[data]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ValidationReportFfi {
            pub field_errors: Vec<FieldErrorFfi>,
            pub rule_errors: Vec<RuleViolationFfi>,
        }

        /// Mirrors `bolted_core::SubmitError`, plus the one failure an *id* can have that a draft
        /// cannot: the foreign handle outlives the core-side draft, so it can submit twice.
        #[error]
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum SubmitErrorFfi {
            Validation { report: ValidationReportFfi },
            Conflicted { fields: Vec<#id> },
            Orphaned,
            AlreadySubmitted,
        }

        impl ::std::fmt::Display for SubmitErrorFfi {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    SubmitErrorFfi::Validation { .. } => write!(f, "submit failed validation"),
                    SubmitErrorFfi::Conflicted { .. } => write!(f, "submit blocked by conflicts"),
                    SubmitErrorFfi::Orphaned => write!(f, "submit of an orphaned draft"),
                    SubmitErrorFfi::AlreadySubmitted => write!(f, "draft already submitted"),
                }
            }
        }
        impl ::std::error::Error for SubmitErrorFfi {}
        impl ::core::convert::From<UnexpectedFfiCallbackError> for SubmitErrorFfi {
            fn from(_: UnexpectedFfiCallbackError) -> Self { SubmitErrorFfi::AlreadySubmitted }
        }
    }
}
