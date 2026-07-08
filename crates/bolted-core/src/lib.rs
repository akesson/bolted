//! `bolted-core` — prototype framework primitives for Bolted's draft/field/store semantics.
//!
//! Sans-io, zero runtime dependencies, no macros, no FFI. Everything here is generic; the
//! concrete "as-if-generated" feature lives in the `spike-profile` crate. This crate is the
//! Phase-1 spike validating ARCHITECTURE §1–§5 and §7 — see `docs/steps/step-01-core-semantics.md`.
#![forbid(unsafe_code)]

pub mod constraint;
pub mod draft;
pub mod field;
pub mod report;
pub mod single_flight;
pub mod store;
pub mod value;

pub use constraint::Constraint;
pub use draft::{Draft, DraftStatus};
pub use field::{Field, SyncState, Validity};
pub use report::{ErrorData, RuleViolation, ValidationReport};
pub use single_flight::{CheckState, CheckToken, SingleFlight};
pub use store::{DraftHandle, Store, StoreDraft, SubmitError, SubmitFailure};
pub use value::Value;
