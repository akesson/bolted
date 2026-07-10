//! A whole feature, scanned out of one source file.
//!
//! `bolted-macros` never needs this: a proc macro is handed one item at a time. `bolted-ffi-gen` does,
//! because the FFI layer of `Profile` mentions `Username`'s error variants, and nothing hands those to
//! it. So it reads the feature crate's source with `syn` — which is, exactly, how BoltFFI reads ours
//! (step 10, M0). The symmetry is not a coincidence: neither tool can see expanded code.

use crate::naming::is_bolted_attr;
use crate::{EntityDecl, Rule, RulesDecl, ValueDecl, entity, rules};
use syn::{Ident, Item};

/// One entity, the values its fields are typed with, and its tier-2 rules.
pub struct Feature {
    pub values: Vec<ValueDecl>,
    pub entity: EntityDecl,
    pub rules: Vec<Rule>,
}

impl Feature {
    /// Scan a parsed source file. Declarations are recognised by spelling
    /// ([`is_bolted_attr`]), because a source scanner cannot resolve a `use` alias.
    pub fn from_file(file: &syn::File) -> syn::Result<Self> {
        let mut values = Vec::new();
        let mut entity: Option<EntityDecl> = None;
        let mut rules: Option<RulesDecl> = None;

        for item in &file.items {
            match item {
                Item::Struct(s) if find(&s.attrs, "value").is_some() => {
                    values.push(ValueDecl::from_item(strip_struct(s.clone()))?);
                }
                Item::Struct(s) if find(&s.attrs, "entity").is_some() => {
                    let attr = find(&s.attrs, "entity").expect("just matched");
                    let has_rules = match &attr.meta {
                        syn::Meta::Path(_) => false,
                        syn::Meta::List(list) => entity::parse_entity_attr(list.tokens.clone())?,
                        syn::Meta::NameValue(nv) => {
                            return Err(syn::Error::new_spanned(
                                nv,
                                "`#[bolted::entity]` takes no value",
                            ));
                        }
                    };
                    if entity.is_some() {
                        return Err(syn::Error::new_spanned(
                            s,
                            "two `#[bolted::entity]` declarations in one file: the FFI generator \
                             emits one feature per crate",
                        ));
                    }
                    entity = Some(EntityDecl::from_item(has_rules, strip_struct(s.clone()))?);
                }
                Item::Impl(i) if find(&i.attrs, "rules").is_some() => {
                    let attr = find(&i.attrs, "rules").expect("just matched");
                    let syn::Meta::List(list) = &attr.meta else {
                        return Err(syn::Error::new_spanned(
                            attr,
                            "`#[bolted::rules(entity = Profile)]` — name the entity",
                        ));
                    };
                    let named = rules::parse_entity(list.tokens.clone())?;
                    let mut item = i.clone();
                    item.attrs.retain(|a| !is_bolted_attr(a, "rules"));
                    rules = Some(RulesDecl::from_item(named, item)?);
                }
                _ => {}
            }
        }

        let Some(entity) = entity else {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "no `#[bolted::entity]` declaration found in this file",
            ));
        };

        // `#[bolted::entity(rules)]` and a `#[bolted::rules]` block are two halves of one statement.
        // The macros already make a mismatch a compile error at rung 2 (a missing impl, or a
        // conflicting one). Here it would instead produce an FFI layer that silently omits the
        // feature's tier-2 rules, so it is checked rather than assumed.
        match (&rules, entity.has_rules) {
            (Some(r), true) if r.entity != entity.name => {
                return Err(syn::Error::new_spanned(
                    &r.entity,
                    format!(
                        "`#[bolted::rules(entity = {})]` does not name this file's entity `{}`",
                        r.entity, entity.name
                    ),
                ));
            }
            (Some(_), true) | (None, false) => {}
            (Some(r), false) => {
                return Err(syn::Error::new_spanned(
                    &r.entity,
                    "a `#[bolted::rules]` block exists, but the entity is not `#[bolted::entity(rules)]`",
                ));
            }
            (None, true) => {
                return Err(syn::Error::new_spanned(
                    &entity.name,
                    "`#[bolted::entity(rules)]` promises a `#[bolted::rules]` block this file does \
                     not contain",
                ));
            }
        }

        Ok(Feature {
            values,
            entity,
            rules: rules.map(|r| r.rules).unwrap_or_default(),
        })
    }

    /// The declaration for a field's value type, if this file declares one.
    ///
    /// `None` means the value is hand-written — a composite (D20), like `Profile::availability`. The
    /// FFI generator does **not** guess at its projection; it emits a reference to a `custom` module
    /// the feature's FFI crate must supply, and a missing one is a compile error.
    pub fn value(&self, name: &Ident) -> Option<&ValueDecl> {
        self.values.iter().find(|v| &v.name == name)
    }
}

fn find<'a>(attrs: &'a [syn::Attribute], name: &str) -> Option<&'a syn::Attribute> {
    attrs.iter().find(|a| is_bolted_attr(a, name))
}

/// Remove the `#[bolted::…]` marker itself; the declaration parsers strip only the helper attributes
/// nested under it (`#[sanitize]`, `#[validate]`, `#[check]`), because in a proc-macro invocation the
/// marker has already been consumed by the compiler.
fn strip_struct(mut item: syn::ItemStruct) -> syn::ItemStruct {
    item.attrs
        .retain(|a| !is_bolted_attr(a, "value") && !is_bolted_attr(a, "entity"));
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
        #[bolted_macros::value]
        #[sanitize(trim)]
        #[validate(len_chars(min = 3, max = 20))]
        pub struct Username(String);

        #[bolted_macros::entity(rules)]
        pub struct Profile {
            #[check(rule = "username_unique", pending_key = "p", required_key = "r", failed_key = "f")]
            pub username: Username,
            pub availability: DateRange,
        }

        #[bolted_macros::rules(entity = Profile)]
        impl ProfileDraft {
            #[rule(pins(username))]
            fn a_rule(&self) -> Result<(), ErrorData> { Ok(()) }
        }
    "#;

    fn feature(src: &str) -> syn::Result<Feature> {
        Feature::from_file(&syn::parse_file(src).expect("parses"))
    }

    /// `expect_err` wants `T: Debug`, and a `Feature` holds `syn` items.
    fn err(src: &str, why: &str) -> String {
        feature(src).map(|_| ()).expect_err(why).to_string()
    }

    #[test]
    fn a_feature_is_scanned_out_of_source_text() {
        let f = feature(SRC).expect("scans");
        assert_eq!(f.entity.name.to_string(), "Profile");
        assert_eq!(f.values.len(), 1);
        assert_eq!(f.rules.len(), 1);
        assert_eq!(f.entity.checks().len(), 1);
    }

    /// The escape hatch (D25): an undeclared value type is not guessed at.
    #[test]
    fn an_undeclared_value_type_has_no_declaration_to_find() {
        let f = feature(SRC).expect("scans");
        let ids: Vec<_> = f
            .entity
            .fields
            .iter()
            .map(|x| x.value_ident().cloned())
            .collect();
        assert!(f.value(ids[0].as_ref().expect("path type")).is_some()); // Username: declared
        assert!(f.value(ids[1].as_ref().expect("path type")).is_none()); // DateRange: hand-written
    }

    #[test]
    fn promising_rules_without_writing_them_is_refused() {
        let src = SRC.replace("#[bolted_macros::rules(entity = Profile)]", "");
        assert!(err(&src, "entity(rules) with no rules block").contains("does not contain"));
    }

    #[test]
    fn writing_rules_without_promising_them_is_refused() {
        let src = SRC.replace(
            "#[bolted_macros::entity(rules)]",
            "#[bolted_macros::entity]",
        );
        assert!(
            err(&src, "rules block with a plain entity").contains("not `#[bolted::entity(rules)]`")
        );
    }

    #[test]
    fn rules_must_name_this_files_entity() {
        let src = SRC.replace("rules(entity = Profile)", "rules(entity = Note)");
        assert!(err(&src, "wrong entity named").contains("does not name this file's entity"));
    }
}
