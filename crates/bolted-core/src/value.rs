//! The value-type contract (tier 1 of validation).

use crate::constraint::Constraint;

/// A constrained value type: a newtype constructible only by passing its declared constraints.
///
/// "Parse, don't validate" — holding a `Value` is proof of validity; the raw form is recoverable
/// but re-parsing it always succeeds (invariant I1). In the real framework `#[bolted::value]`
/// generates this impl; in the spike it is hand-written exactly as the macro would emit it.
pub trait Value: Clone + PartialEq + Send + Sync + 'static {
    /// The unvalidated input form (e.g. `String`, or `(Date, Date)` for a composite value object).
    ///
    /// `Debug` is required (beyond the ARCHITECTURE §5 sketch) so [`crate::field::Field`] and
    /// [`crate::field::Validity`] can derive `Debug` — the retained raw of a rejected input must be
    /// inspectable in test/diagnostic output. Recorded as a deviation in the step-01 report.
    type Raw: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static;
    /// The structured, localisable rejection reason. Never a message string.
    type Error: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static;

    /// Sanitize, then validate. `Ok` is a proof-of-validity value; `Err` is structured data.
    fn try_new(raw: Self::Raw) -> Result<Self, Self::Error>;
    /// Recover the raw form. Roundtrip: `try_new(v.into_raw()) == Ok(v)` for every valid `v`.
    fn into_raw(self) -> Self::Raw;
    /// The declared constraints, for shell-affordance export.
    fn constraints() -> &'static [Constraint];
}
