//! Declared constraint metadata. This is *not* executable validation logic — enforcement lives
//! in [`crate::value::Value::try_new`]. Constraints exist so a future shell layer can derive UI
//! affordances (a `maxLength`, a required marker, a pattern hint) from the same single source of
//! truth that the core validates against.

/// A single declared constraint on a field or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    /// The field must hold a value (non-optional). A *field-level* concern — see the step-01
    /// report for the open question of whether `Required` belongs on the value or the field.
    Required,
    /// Character-count bounds, inclusive.
    LenChars { min: u32, max: u32 },
    /// A named, opaque predicate (e.g. `"ascii_alnum_underscore"`). The name is for the shell;
    /// the semantics live in `try_new`.
    Custom(&'static str),
}
