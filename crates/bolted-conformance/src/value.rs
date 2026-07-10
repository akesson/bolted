//! The value tier: C01. A claim about [`Value`] alone — no field, no draft, no store.

use crate::check;
use bolted_core::Value;
use proptest::prelude::*;
use proptest::strategy::BoxedStrategy;
use proptest::test_runner::TestCaseError;

/// What the suite needs to know about a value type in order to test it.
///
/// A **marker type** implements this, naming its value type — `struct UsernameFixture;` with
/// `type Value = Username` — rather than the value type implementing it directly. Not a style
/// choice: the value lives in a feature crate and this trait lives here, so a `impl ValueFixture for
/// Username` written in the feature's *test* crate would violate the orphan rule. A fixture type is
/// local to whoever writes it. [`crate::ConformanceFeature`] has the same shape for the same reason.
pub trait ValueFixture {
    /// The `Debug` bound is the fixture's, not the core's: [`Value`] deliberately does not require it
    /// (a value object may hold a secret), but a property test that cannot print its shrunk
    /// counterexample is not worth running.
    type Value: Value + std::fmt::Debug;

    /// Raw forms that may or may not be valid. Feeds C01, which filters.
    ///
    /// A good `any_raw` includes forms the value type **sanitizes** (`"  alice  "` for a trimming
    /// type, `"ALICE@X.COM"` for a lowercasing one). That is the interesting half of the roundtrip:
    /// `into_raw` must return the canonical form, not the form that was typed.
    fn any_raw() -> BoxedStrategy<RawOf<Self>>;

    /// Raw forms that must always parse. The suite asserts this, so a careless fixture fails loudly
    /// rather than silently rejecting every case (see [`parse`]).
    fn valid_raw() -> BoxedStrategy<RawOf<Self>>;

    /// A raw form that must never parse.
    fn invalid_raw() -> RawOf<Self>;
}

/// The value type a fixture speaks for.
pub type ValueOf<F> = <F as ValueFixture>::Value;
/// Its raw form.
pub type RawOf<F> = <ValueOf<F> as Value>::Raw;

/// Parse a raw the fixture promised was valid. A broken promise is a **test failure**, not a rejected
/// case — a `prop_assume!` here would let a fixture whose `valid_raw()` never validates silently pass
/// an empty suite. (Step 07's lesson, turned on the harness itself: a missing precondition does not
/// weaken a property, it hides one.)
pub(crate) fn parse<F: ValueFixture>(raw: RawOf<F>) -> Result<ValueOf<F>, TestCaseError> {
    ValueOf::<F>::try_new(raw.clone())
        .map_err(|e| TestCaseError::fail(format!("valid_raw() produced {raw:?}, rejected: {e:?}")))
}

/// C01 — `Value::try_new(v.into_raw()) == Ok(v)` for every valid `v`. Holding a `Value` is proof of
/// validity, and the raw form loses none of it.
pub fn c01_value_raw_roundtrip<F: ValueFixture>() {
    check(F::any_raw(), |raw| {
        if let Ok(v) = ValueOf::<F>::try_new(raw) {
            prop_assert_eq!(ValueOf::<F>::try_new(v.clone().into_raw()), Ok(v));
        }
        Ok(())
    });
}
