//! `gen-note` — `spike-note`, declared instead of written.
//!
//! **This crate is written before `gen-profile`, on purpose.** A macro given exactly one input grows
//! that input's shape, silently, exactly as step 08's conformance suite grew `spike-profile`'s. Had
//! `#[bolted::entity]` met the rule-and-check feature first, it could have come to assume a rule and
//! a check — and nothing about reading it would reveal the assumption. What this feature *cannot*
//! declare, the macros had no business requiring.
//!
//! So: two plain text fields. No composite value object, no tier-2 rule, no async check. The whole
//! feature is the twelve lines below, against `spike-note`'s 335 — and it passes the same
//! conformance suite, minus the four invariants it does not owe.
//!
//! The one visible difference from the hand-written original is that a length-bounded value's
//! rejection is `too_short`, not `blank`. A uniform DSL normalizes error keys; that is a real
//! migration cost for a shell holding l10n strings, and it is why `custom(..)` takes a `key`
//! override while `len_chars` does not. Recorded in `docs/steps/step-09-report.md`.
#![forbid(unsafe_code)]

/// Trim; 1..=40 chars.
#[bolted_macros::value]
#[sanitize(trim)]
#[validate(len_chars(min = 1, max = 40))]
pub struct Title(String);

/// Trim; 1..=200 chars.
#[bolted_macros::value]
#[sanitize(trim)]
#[validate(len_chars(min = 1, max = 200))]
pub struct Body(String);

#[bolted_macros::entity]
pub struct Note {
    pub title: Title,
    pub body: Body,
}

/// The store is `Send` by construction (D16), proved at rung 1 rather than asserted in a report.
/// Generated code has no more right to reach for an `Rc` than hand-written code does.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<NoteStore>();
    assert_send::<NoteDraft>();
    assert_send::<Note>();
};
