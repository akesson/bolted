//! Per-field projection: the one place that decides how a field crosses the boundary.
//!
//! Two cases, and the second is the interesting one.
//!
//! - The field's value type is **declared** in the feature's source with `#[bolted::value]`. The
//!   generator knows its raw type and its error variants, and projects it itself. `Raw = String` gets
//!   `bolted_ffi::TextFieldState` (D24).
//! - The field's value type is **not declared** — a composite, hand-written under D20, like
//!   `Profile::availability: DateRange`. The generator does not guess. It emits references into a
//!   `custom` module the feature's FFI crate must supply, and a missing item is a compile error.
//!
//! Nothing here decides *behaviour*. Every projection reduces to `Value::try_new` / `Value::into_raw`
//! and a `#[data]` shape; the judgements stayed in `bolted-core` where step 09 put them.

use bolted_decl::naming::upper_camel;
use bolted_decl::{EntityField, Feature, ValueDecl};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;

pub struct FieldProj<'a> {
    pub field: &'a EntityField,
    /// `None` when the value type is hand-written (the `custom` escape hatch).
    pub value: Option<&'a ValueDecl>,
}

impl<'a> FieldProj<'a> {
    pub fn all(feature: &'a Feature) -> Vec<FieldProj<'a>> {
        feature
            .entity
            .fields
            .iter()
            .map(|field| FieldProj {
                field,
                value: field.value_ident().and_then(|id| feature.value(id)),
            })
            .collect()
    }

    pub fn ident(&self) -> &Ident {
        &self.field.ident
    }

    /// `availability` → `Availability`, the prefix for every `custom::` item this field needs.
    fn camel(&self) -> Ident {
        upper_camel(&self.field.ident)
    }

    fn custom_fn(&self, suffix: &str) -> TokenStream2 {
        let f = format_ident!("{}_{}", self.field.ident, suffix);
        quote!(crate::custom::#f)
    }

    /// A **bare** ident, not `crate::custom::X`. BoltFFI's bindgen resolves `#[data]` field types
    /// syntactically, and a qualified path is not what it expects to see; the generated module globs
    /// `crate::custom::*` instead, exactly as it globs `bolted_ffi::*`.
    fn custom_ty(&self, suffix: &str) -> TokenStream2 {
        let t = format_ident!("{}{}", self.camel(), suffix);
        quote!(#t)
    }

    /// Does this field need the `custom` escape hatch?
    pub fn is_custom(&self) -> bool {
        self.value.is_none()
    }

    /// Is this a declared `Raw = String` value? Only these share `TextFieldState`.
    fn is_text(&self) -> bool {
        self.value.is_some_and(|v| v.is_text())
    }

    /// The `#[data]` type of this field inside the snapshot.
    pub fn state_ty(&self) -> TokenStream2 {
        if self.is_text() {
            quote!(TextFieldState)
        } else {
            self.custom_ty("FieldState")
        }
    }

    /// Project a live `Field<V>` off a draft.
    pub fn state_expr(&self, draft: &TokenStream2) -> TokenStream2 {
        let id = self.ident();
        if self.is_text() {
            quote!(bolted_ffi::text_field_state(&#draft.#id))
        } else {
            let f = self.custom_fn("state");
            quote!(#f(&#draft.#id))
        }
    }

    /// The wire form of `V::Raw` — what a setter takes and `…Values` carries.
    pub fn wire_ty(&self) -> TokenStream2 {
        if self.is_text() {
            quote!(String)
        } else {
            self.custom_ty("Raw")
        }
    }

    /// wire → `V::Raw`.
    pub fn to_core_raw(&self, wire: TokenStream2) -> TokenStream2 {
        if self.is_text() {
            wire
        } else {
            let f = self.custom_fn("raw");
            quote!(#f(#wire))
        }
    }

    pub fn stash_ty(&self) -> TokenStream2 {
        if self.is_text() {
            quote!(TextFieldStashFfi)
        } else {
            self.custom_ty("Stash")
        }
    }

    pub fn stash_expr(&self, stash: &TokenStream2) -> TokenStream2 {
        let id = self.ident();
        if self.is_text() {
            quote!(bolted_ffi::text_stash(&#stash.#id))
        } else {
            let f = self.custom_fn("stash");
            quote!(#f(&#stash.#id))
        }
    }

    pub fn core_stash_expr(&self, stash: &TokenStream2) -> TokenStream2 {
        let id = self.ident();
        if self.is_text() {
            quote!(bolted_ffi::to_core_text_stash(&#stash.#id))
        } else {
            let f = self.custom_fn("from_stash");
            quote!(#f(&#stash.#id))
        }
    }

    /// The `#[error]` type this field's setter raises. Declared values get a generated one named after
    /// the value (`UsernameErrorFfi`); a custom field names its own.
    pub fn error_ty(&self) -> TokenStream2 {
        match self.value {
            Some(v) => {
                let e = format_ident!("{}ErrorFfi", v.name);
                quote!(#e)
            }
            None => self.custom_ty("ErrorFfi"),
        }
    }

    /// `V::Error` → the FFI error type.
    pub fn error_from(&self) -> TokenStream2 {
        match self.value {
            Some(_) => {
                let e = self.error_ty();
                quote!(#e::from)
            }
            None => self.custom_fn("error"),
        }
    }

    /// D23: what a mutator on a released draft returns.
    pub fn closed(&self) -> TokenStream2 {
        match self.value {
            Some(_) => {
                let e = self.error_ty();
                quote!(#e::DraftClosed)
            }
            None => {
                let f = self.custom_fn("closed");
                quote!(#f())
            }
        }
    }

    /// `try_set_username`.
    pub fn setter(&self) -> Ident {
        format_ident!("try_set_{}", self.field.ident)
    }
}
