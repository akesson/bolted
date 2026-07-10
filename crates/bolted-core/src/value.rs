//! The value-type contract (tier 1 of validation).

use crate::constraint::Constraint;
use crate::report::ErrorData;

/// A constrained value type: a newtype constructible only by passing its declared constraints.
///
/// "Parse, don't validate" — holding a `Value` is proof of validity; the raw form is recoverable
/// but re-parsing it always succeeds (conformance C01). In the real framework `#[bolted::value]`
/// generates this impl; in the spike it is hand-written exactly as the macro would emit it.
///
/// **Value objects must not be `Copy`** (ARCHITECTURE §8, step-01 friction F4). Generated
/// checkout/rebase code clones every field uniformly, and `clippy::clone_on_copy` rejects that for
/// a `Copy` field under `-D warnings`. Rust cannot express a negative bound, so this is enforced by
/// `#[bolted::value]` (which will not emit `Copy`) and by `bolted-check`.
pub trait Value: Clone + PartialEq + Send + Sync + 'static {
    /// The unvalidated input form (e.g. `String`, or `(Date, Date)` for a composite value object).
    ///
    /// `Debug` is required (beyond the ARCHITECTURE §5 sketch) so [`crate::field::Field`] and
    /// [`crate::field::Validity`] can derive `Debug` — the retained raw of a rejected input must be
    /// inspectable in test/diagnostic output. Recorded as a deviation in the step-01 report.
    type Raw: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static;
    /// The structured, localisable rejection reason. Never a message string.
    ///
    /// `Into<ErrorData>` is part of the contract (ARCHITECTURE §8): every tier-1 error must be
    /// projectable into a keyed report entry, and every consumer that builds a report or renders an
    /// inline error needs exactly this bound. The spike carried it as an external bridge plus a
    /// restated `where` clause in two crates before it was promoted here (step-01 Q2, step-04
    /// friction 3).
    type Error: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static + Into<ErrorData>;

    /// Sanitize, then validate. `Ok` is a proof-of-validity value; `Err` is structured data.
    fn try_new(raw: Self::Raw) -> Result<Self, Self::Error>;
    /// Recover the raw form. Roundtrip: `try_new(v.into_raw()) == Ok(v)` for every valid `v`.
    fn into_raw(self) -> Self::Raw;
    /// The declared constraints, for shell-affordance export.
    fn constraints() -> &'static [Constraint];
}
