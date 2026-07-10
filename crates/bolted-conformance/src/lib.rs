//! `bolted-conformance` — the executable form of [`docs/CONFORMANCE.md`], generic over a feature.
//!
//! Every normative statement C01–C22 exists here as a `cNN_*` function. A feature proves it is a
//! Bolted feature by implementing the fixture traits and stamping the suites:
//!
//! ```ignore
//! bolted_conformance::field_suite!(username, Username);
//! bolted_conformance::feature_suite!(profile, ProfileFixture);
//! bolted_conformance::rule_suite!(profile_rule, ProfileFixture);
//! bolted_conformance::async_check_suite!(profile_check, ProfileFixture);
//! ```
//!
//! ## Three tiers, because three tiers is what the invariants actually are
//!
//! - **Value** (C01) and **field** (C02–C06, C09, C14, C19, C20) are claims about `Value` and
//!   `Field<V>`. They need no feature at all, only a [`ValueFixture`] — so they run once **per value
//!   type**, which is strictly more coverage than the single-type tests they replace.
//! - **Feature** (C06–C08, C10–C22) are claims about a `Draft` inside a `Store`. They need a
//!   [`ConformanceFeature`], with [`RuleFeature`] and [`AsyncCheckFeature`] for the invariants that
//!   presuppose a tier-2 rule or an async check. A feature with neither is still a Bolted feature;
//!   it simply has fewer invariants to satisfy, and the trait bounds say which.
//!
//! ## A fixture cannot skip an ID
//!
//! The `*_suite!` macros stamp **every** test in their tier. They are `macro_rules!`, and they only
//! stamp names — the same doctrine ARCHITECTURE §5 sets for `bolted-macros`, for the same reason
//! (macro output is the least verifiable code, so it must stay trivial). `tests/manifest.rs` checks
//! three ways: every documented `CNN` has a `cNN_*` here, every `cNN_*` here is documented, and every
//! `cNN_*` here is stamped by some suite macro. Without the third, a test could exist and never run.
//!
//! ## On panicking
//!
//! `CLAUDE.md` forbids `unwrap`/`expect`/`panic!` in library code. This library's *purpose* is to
//! fail a test process: `assert!` and a panicking proptest runner are its return values, not an
//! error-handling shortcut. The rule is suspended here, deliberately, and nowhere else.
//!
//! [`docs/CONFORMANCE.md`]: https://github.com/../docs/CONFORMANCE.md

pub mod feature;
pub mod field;
mod macros;
pub mod value;

pub use feature::{AsyncCheckFeature, ConformanceFeature, RuleFeature};
pub use value::ValueFixture;

pub use feature::*;
pub use field::*;
pub use value::c01_value_raw_roundtrip;

use proptest::test_runner::{Config, TestCaseError, TestRunner};

/// Run a property, panicking with proptest's shrunk counterexample on failure.
///
/// `failure_persistence: None` because this crate has no source file to write a regression seed
/// beside — the failing input belongs to whichever feature crate stamped the suite.
pub(crate) fn check<S: proptest::strategy::Strategy>(
    strategy: S,
    body: impl Fn(S::Value) -> Result<(), TestCaseError>,
) {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    if let Err(failure) = TestRunner::new(config).run(&strategy, body) {
        panic!("conformance property failed: {failure}");
    }
}
