//! `bolted-check` — the verification harness's constraint-surface analyzer (Phase 4).
//!
//! This is the **third emitter over the one parser** (D25). `bolted-macros` reads a
//! [`bolted_decl::Feature`] and emits the feature; `bolted-ffi-gen` reads the same `Feature` and
//! emits the FFI layer; `bolted-check` reads it and renders a **human-readable snapshot of the
//! declared constraint surface** — every sanitizer, validator, error key, field type, tier-2 rule,
//! and the field-level `constraints()` a shell would derive UI from. The snapshot is committed beside
//! each feature and byte-checked inside `mise run check`, so a one-token constraint edit
//! (`max = 30` → `29`) becomes a loud, isolated, reviewable diff instead of noise inside a 600-line
//! regenerated `generated.rs`.
//!
//! It pays a debt D27 wrote down by name: *"constraint tightening is a build-time event —
//! `bolted-check`'s constraint-semver snapshot (Phase 4) fails the build until the team makes a
//! version decision."* The runtime half (a versioned stash envelope, wholesale refusal at the parse
//! gate) shipped in step 12; this is the build-time half — the thing that tells the team a tightening
//! happened, and names the [`STASH_SCHEMA_VERSION`](RuntimeSurface::schema_version) duty.
//!
//! # Two seams, kept apart (kill criterion 1 / 2)
//!
//! - The **renderer is pure**: a function of an already-parsed `Feature` plus a [`RuntimeSurface`] the
//!   caller supplies. It depends on `bolted-decl` and `bolted-core` only — never `boltffi`, never
//!   `bolted-ffi-gen`. Analyzer and emitter are different seams over the same parse.
//! - It **never links or executes a feature crate.** A composite value object (`DateRange`, D20) is
//!   invisible to a source scan — its `Constraint::Custom("start_le_end")` exists only at runtime. So
//!   the caller (a per-feature test or generator that *does* link the feature) reads each field's
//!   `FieldId::constraints()` and passes it in as the `RuntimeSurface`. The renderer cross-checks that
//!   surface against the declared field list, so an omitted field is a [`RenderError`], not a silent
//!   gap.
#![forbid(unsafe_code)]

pub mod budget;
pub mod doctor;

use bolted_core::Constraint;
use bolted_decl::{ErrorVariant, Feature, ParamTy, Sanitizer, Validator};
use std::collections::BTreeSet;
use std::fmt;

/// The snapshot text format. Bump when the *rendering* changes in a way that reflows every committed
/// `.snap` (so the drift diff is understood as a tooling change, not a constraint change). Independent
/// of the feature's `STASH_SCHEMA_VERSION`, which versions the persisted stash wire format.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// The runtime-derived half of the surface, supplied by a caller that links the feature.
///
/// The renderer cannot compute this itself: `FieldId::constraints()` is macro-generated (it prepends
/// `Required`, D13) and a composite's constraints are hand-written (D20), so both live behind a
/// compiled feature crate the analyzer deliberately does not depend on. The per-feature drift test
/// and the `gen-constraints` generator both build this from the linked feature and hand it in.
pub struct RuntimeSurface {
    /// The feature's `STASH_SCHEMA_VERSION`, read from its generated module. A human-bumped constant
    /// (`bolted-ffi-gen/src/dto.rs`); the snapshot renders it in the header so "constraints changed,
    /// version did not" is a single visible diff.
    pub schema_version: u32,
    /// Every entity field's full runtime constraint list, in any order — the renderer re-orders to the
    /// declaration and enforces exact coverage.
    pub fields: Vec<RuntimeField>,
}

/// One entity field's runtime constraint list: `FieldId::<Variant>.constraints()`, keyed by the
/// field's declared name (the snake-case ident, e.g. `"availability"`).
pub struct RuntimeField {
    /// The declared field ident, matched against [`bolted_decl::EntityField`]'s `ident`.
    pub field: String,
    /// `Required` (prepended by the macro) followed by the value type's intrinsics — including a
    /// composite's hand-written `Constraint::Custom(..)`, which is why this is where composites are
    /// covered.
    pub constraints: Vec<Constraint>,
}

impl RuntimeField {
    pub fn new(field: impl Into<String>, constraints: Vec<Constraint>) -> Self {
        RuntimeField {
            field: field.into(),
            constraints,
        }
    }
}

/// Why a `RuntimeSurface` could not be rendered against a declaration. Both variants mean the caller's
/// runtime field list does not match the declared entity's fields exactly — the coverage guarantee
/// that keeps composites from silently dropping out of the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// A declared field has no entry in the `RuntimeSurface` — the caller forgot to pass its
    /// `constraints()`. An omission must fail, or the snapshot would under-report the surface.
    RuntimeMissingField(String),
    /// The `RuntimeSurface` names a field the declaration does not have — a stale or misspelled entry.
    RuntimeUnknownField(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::RuntimeMissingField(name) => write!(
                f,
                "runtime surface is missing declared field `{name}`: pass its \
                 `FieldId::constraints()` (an omitted field would silently under-report the surface)"
            ),
            RenderError::RuntimeUnknownField(name) => write!(
                f,
                "runtime surface names field `{name}`, which the declaration does not have: \
                 remove the stale entry"
            ),
        }
    }
}

impl std::error::Error for RenderError {}

/// Render the constraint-surface snapshot for one feature. Deterministic (declaration order
/// throughout, no `HashMap` in the output path) and line-oriented (each constraint on its own line,
/// so a single-token change is a single-line diff). `feature_name` is the generated feature crate's
/// name (`"gen-note"`), used only in the header.
pub fn render_constraint_snapshot(
    feature_name: &str,
    feature: &Feature,
    runtime: &RuntimeSurface,
) -> Result<String, RenderError> {
    // Coverage cross-check first, so an omitted composite is a hard error and never a blank section.
    let declared: BTreeSet<String> = feature.entity.fields.iter().map(field_ident).collect();
    for rf in &runtime.fields {
        if !declared.contains(&rf.field) {
            return Err(RenderError::RuntimeUnknownField(rf.field.clone()));
        }
    }

    let mut out = String::new();
    out.push_str(&header(feature_name, runtime.schema_version));

    out.push_str("\n[values]\n");
    for v in &feature.values {
        out.push('\n');
        out.push_str(&render_value(v));
    }

    out.push_str("\n[entity]\n");
    out.push_str(&format!("\nentity {}\n", feature.entity.name));
    for f in &feature.entity.fields {
        out.push_str(&render_field(feature, f));
    }

    out.push_str("\n[rules]\n");
    if feature.rules.is_empty() {
        out.push_str("(none)\n");
    } else {
        for r in &feature.rules {
            let pins = r
                .pins
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("rule {} pins={pins}\n", r.ident));
        }
    }

    out.push_str("\n[runtime-constraints]\n");
    for f in &feature.entity.fields {
        let ident = field_ident(f);
        let Some(rf) = runtime.fields.iter().find(|rf| rf.field == ident) else {
            return Err(RenderError::RuntimeMissingField(ident));
        };
        out.push_str(&format!(
            "{ident}: {}\n",
            render_constraints(&rf.constraints)
        ));
    }

    Ok(out)
}

fn header(feature_name: &str, schema_version: u32) -> String {
    // A comment block, so the file explains its own tripwire to whoever hits the diff.
    format!(
        "# bolted constraint surface\n\
         # feature: {feature_name}\n\
         # snapshot-format: v{SNAPSHOT_FORMAT_VERSION}\n\
         # stash-schema-version: {schema_version}\n\
         #\n\
         # Generated by `mise run gen:ffi`; byte-checked by `mise run check`. Nothing formats this\n\
         # file (D28), which is what makes the byte comparison honest. A diff here is a change to the\n\
         # constraint surface: review it, decide whether STASH_SCHEMA_VERSION must move (D27 — a\n\
         # *tightening* strands every stashed draft whose raw no longer parses), then regenerate with\n\
         # `mise run gen:ffi`. The schema version is a human-bumped constant\n\
         # (crates/bolted-ffi-gen/src/dto.rs), never auto-derived from these constraints: that is\n\
         # D27's own rejected alternative, because a *loosening* would kill every stash for no reason.\n"
    )
}

fn render_value(v: &bolted_decl::ValueDecl) -> String {
    let mut s = String::new();
    s.push_str(&format!("value {}\n", v.name));
    s.push_str(&format!("  raw {}\n", tokens_to_string(&v.raw)));
    for san in &v.sanitizers {
        let name = match san {
            Sanitizer::Trim => "trim",
            Sanitizer::Lowercase => "lowercase",
        };
        s.push_str(&format!("  sanitize {name}\n"));
    }
    for val in &v.validators {
        s.push_str(&format!("  validate {}\n", render_validator(val)));
    }
    for ev in &v.error_variants() {
        s.push_str(&format!("  error {}\n", render_error_variant(ev)));
    }
    s
}

fn render_validator(v: &Validator) -> String {
    match v {
        Validator::LenChars { min, max } => format!("len_chars(min = {min}, max = {max})"),
        Validator::Custom {
            path,
            variant,
            key,
            constraint,
        } => format!(
            "custom({}) variant={variant} key={key} constraint={constraint}",
            tokens_to_string(path)
        ),
    }
}

fn render_error_variant(ev: &ErrorVariant) -> String {
    let params = ev
        .params
        .iter()
        .map(|(name, ty)| format!("{name}: {}", param_ty(*ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} key={} params=({params})", ev.ident, ev.key)
}

fn param_ty(ty: ParamTy) -> &'static str {
    match ty {
        ParamTy::U32 => "u32",
    }
}

fn render_field(feature: &Feature, f: &bolted_decl::EntityField) -> String {
    // `declared` if this file's `#[bolted::value]` produced its type; `custom` if it is a hand-written
    // composite the source scan cannot see (D20) — the distinction that tells a reader where to look.
    let source = match f.value_ident() {
        Some(id) if feature.value(id).is_some() => "declared",
        _ => "custom",
    };
    let ty = f
        .value_ident()
        .map(|id| id.to_string())
        .unwrap_or_else(|| tokens_to_string(&f.ty));
    let mut s = format!("  field {} type={ty} source={source}\n", field_ident(f));
    if let Some(c) = &f.check {
        s.push_str(&format!(
            "    check rule={} pending_key={} required_key={} failed_key={}\n",
            c.rule, c.pending_key, c.required_key, c.failed_key
        ));
    }
    s
}

fn render_constraints(constraints: &[Constraint]) -> String {
    // `Constraint` derives `Debug`, and its derived form (`Required`, `LenChars { min: 1, max: 40 }`,
    // `Custom("email")`) is exactly the stable, readable projection wanted here.
    constraints
        .iter()
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn field_ident(f: &bolted_decl::EntityField) -> String {
    f.ident.to_string()
}

/// Stringify a `syn` node (a value's raw type, a custom predicate path) without this crate having to
/// name `syn`: `quote` only needs `ToTokens`, which every `syn` node implements.
fn tokens_to_string<T: quote::ToTokens>(t: &T) -> String {
    quote::quote!(#t).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(src: &str) -> Feature {
        Feature::from_file(&syn::parse_file(src).expect("sample source parses")).expect("scans")
    }

    /// A `RuntimeSurface` matching a feature's declared fields exactly, with synthetic constraints.
    /// The renderer's unit tests do not need the *real* runtime values (the `-ffi` drift tests, which
    /// link the feature, prove those); they need a covering surface so the cross-check passes.
    fn covering_surface(feature: &Feature, schema_version: u32) -> RuntimeSurface {
        let fields = feature
            .entity
            .fields
            .iter()
            .map(|f| RuntimeField::new(f.ident.to_string(), vec![Constraint::Required]))
            .collect();
        RuntimeSurface {
            schema_version,
            fields,
        }
    }

    const NOTE: &str = r#"
        #[bolted_macros::value]
        #[sanitize(trim)]
        #[validate(len_chars(min = 1, max = 40))]
        pub struct Title(String);

        #[bolted_macros::value]
        #[sanitize(trim)]
        #[validate(len_chars(min = 1, max = 200))]
        pub struct Body(String);

        #[bolted_macros::entity]
        pub struct Note {
            pub title: Title,
            pub body: Body,
        }
    "#;

    const PROFILE: &str = r#"
        #[bolted_macros::value]
        #[sanitize(trim)]
        #[validate(
            len_chars(min = 3, max = 20),
            custom(ascii_alnum_underscore, variant = InvalidChars, key = "invalid_chars")
        )]
        pub struct Username(String);

        #[bolted_macros::value]
        #[sanitize(trim, lowercase)]
        #[validate(custom(email, variant = Invalid, key = "invalid_email"))]
        pub struct Email(String);

        #[bolted_macros::entity(rules)]
        pub struct Profile {
            #[check(rule = "username_unique", pending_key = "p", required_key = "r", failed_key = "f")]
            pub username: Username,
            pub email: Email,
            pub availability: DateRange,
        }

        #[bolted_macros::rules(entity = Profile)]
        impl ProfileDraft {
            #[rule(pins(email))]
            fn corporate_email(&self) -> Result<(), ErrorData> { Ok(()) }
        }
    "#;

    #[test]
    fn a_simple_feature_renders_every_declared_layer() {
        let f = feature(NOTE);
        let s = render_constraint_snapshot("gen-note", &f, &covering_surface(&f, 1))
            .expect("covering surface renders");
        assert!(s.contains("# feature: gen-note"));
        assert!(s.contains("# stash-schema-version: 1"));
        assert!(s.contains("value Title"));
        assert!(s.contains("  sanitize trim"));
        assert!(s.contains("  validate len_chars(min = 1, max = 40)"));
        assert!(s.contains("  error TooShort key=too_short params=(min: u32, actual: u32)"));
        assert!(s.contains("  error TooLong key=too_long params=(max: u32, actual: u32)"));
        assert!(s.contains("entity Note"));
        assert!(s.contains("  field title type=Title source=declared"));
        assert!(s.contains("[rules]\n(none)"));
    }

    #[test]
    fn a_zero_minimum_length_raises_no_too_short_line() {
        // The D25 subtlety, visible in the snapshot: `min = 0` emits only `TooLong`.
        let f = feature(
            r#"
            #[bolted_macros::value]
            #[validate(len_chars(min = 0, max = 40))]
            pub struct Body(String);
            #[bolted_macros::entity]
            pub struct Note { pub body: Body }
        "#,
        );
        let s = render_constraint_snapshot("x", &f, &covering_surface(&f, 1)).expect("renders");
        assert!(s.contains("  error TooLong"));
        assert!(!s.contains("TooShort"));
    }

    #[test]
    fn a_custom_validator_and_check_and_rule_render() {
        let f = feature(PROFILE);
        let s = render_constraint_snapshot("gen-profile", &f, &covering_surface(&f, 1))
            .expect("renders");
        assert!(s.contains(
            "  validate custom(ascii_alnum_underscore) variant=InvalidChars \
             key=invalid_chars constraint=ascii_alnum_underscore"
        ));
        assert!(s.contains("  error InvalidChars key=invalid_chars params=()"));
        assert!(s.contains("  sanitize trim\n  sanitize lowercase"));
        assert!(
            s.contains("    check rule=username_unique pending_key=p required_key=r failed_key=f")
        );
        // The composite: undeclared by the source scan, marked `custom`.
        assert!(s.contains("  field availability type=DateRange source=custom"));
        assert!(s.contains("rule corporate_email pins=Email"));
    }

    #[test]
    fn the_runtime_section_covers_composites_the_declaration_cannot_see() {
        let f = feature(PROFILE);
        // The caller supplies DateRange's hand-written constraint the way the real -ffi test does.
        let surface = RuntimeSurface {
            schema_version: 1,
            fields: vec![
                RuntimeField::new("username", vec![Constraint::Required]),
                RuntimeField::new("email", vec![Constraint::Required]),
                RuntimeField::new(
                    "availability",
                    vec![Constraint::Required, Constraint::Custom("start_le_end")],
                ),
            ],
        };
        let s = render_constraint_snapshot("gen-profile", &f, &surface).expect("renders");
        assert!(s.contains(r#"availability: Required, Custom("start_le_end")"#));
    }

    #[test]
    fn an_omitted_field_is_a_render_error_not_a_silent_gap() {
        let f = feature(NOTE);
        let surface = RuntimeSurface {
            schema_version: 1,
            // `body` deliberately dropped.
            fields: vec![RuntimeField::new("title", vec![Constraint::Required])],
        };
        assert_eq!(
            render_constraint_snapshot("gen-note", &f, &surface),
            Err(RenderError::RuntimeMissingField("body".to_owned()))
        );
    }

    #[test]
    fn a_field_the_declaration_lacks_is_a_render_error() {
        let f = feature(NOTE);
        let surface = RuntimeSurface {
            schema_version: 1,
            fields: vec![
                RuntimeField::new("title", vec![Constraint::Required]),
                RuntimeField::new("body", vec![Constraint::Required]),
                RuntimeField::new("ghost", vec![Constraint::Required]),
            ],
        };
        assert_eq!(
            render_constraint_snapshot("gen-note", &f, &surface),
            Err(RenderError::RuntimeUnknownField("ghost".to_owned()))
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let f = feature(PROFILE);
        let surface = covering_surface(&f, 1);
        let a = render_constraint_snapshot("gen-profile", &f, &surface).expect("renders");
        let b = render_constraint_snapshot("gen-profile", &f, &surface).expect("renders");
        assert_eq!(a, b);
    }

    // The renderer, exercised against exactly the sources that ship — the same input the real
    // `-ffi` generators and drift tests parse. A covering (synthetic) surface stands in for the
    // linked runtime here; the real `constraints()` are proven in the `-ffi` drift tests.

    #[test]
    fn renders_the_real_gen_note_source() {
        let f = feature(include_str!("../../gen-note/src/lib.rs"));
        let s = render_constraint_snapshot("gen-note", &f, &covering_surface(&f, 1))
            .expect("real gen-note renders");
        assert!(s.contains("value Title"));
        assert!(s.contains("value Body"));
        assert!(s.contains("  validate len_chars(min = 1, max = 200)"));
        assert!(s.contains("[rules]\n(none)"));
    }

    #[test]
    fn renders_the_real_gen_profile_source() {
        let f = feature(include_str!("../../gen-profile/src/lib.rs"));
        let s = render_constraint_snapshot("gen-profile", &f, &covering_surface(&f, 1))
            .expect("real gen-profile renders");
        assert!(s.contains("value Username"));
        assert!(s.contains("value PersonName"));
        assert!(s.contains("value Email"));
        // The composite is a field but not a `value` — declared as `custom`.
        assert!(s.contains("  field availability type=DateRange source=custom"));
        assert!(s.contains("rule corporate_email pins=Email"));
    }
}
