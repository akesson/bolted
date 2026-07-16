//! What `#[bolted::value]` does not generate: the `custom(..)` predicates, and the one
//! hand-written value object (`Paused`, D20's route — its raw is a `bool`, which the text-first
//! DSL has no shape for; recorded in the step-18 report).

use bolted_core::{Constraint, ErrorData, Value};

/// An absolute path: starts with `/`. A `custom(..)` predicate: `fn(&str) -> bool`.
pub fn absolute_path(s: &str) -> bool {
    s.starts_with('/')
}

/// A whole number of minutes in `1..=1440`. The raw is the text a settings box sends; the range
/// judgement lives here, in ordinary code the compiler checks, not in the macro.
pub fn interval_in_range(s: &str) -> bool {
    matches!(s.parse::<u32>(), Ok(n) if (1..=1440).contains(&n))
}

// -------------------------------------------------------------------------------------------------
// Paused — hand-written (D20). Deliberately not `Copy`, even though it easily could be (D8).
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paused(bool);

/// A `bool` has no invalid inhabitant, so this enum has no variants and `try_new` cannot fail.
/// The type still exists because [`Value::Error`] must: the contract has no infallible arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PausedError {}

impl Paused {
    pub fn is_on(&self) -> bool {
        self.0
    }
}

impl Value for Paused {
    type Raw = bool;
    type Error = PausedError;

    fn try_new(raw: bool) -> Result<Self, PausedError> {
        Ok(Paused(raw))
    }

    fn into_raw(self) -> bool {
        self.0
    }

    fn constraints() -> &'static [Constraint] {
        &[]
    }
}

impl From<PausedError> for ErrorData {
    fn from(e: PausedError) -> Self {
        match e {}
    }
}
