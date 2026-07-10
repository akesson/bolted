//! `spike-profile` — the hand-written, "as-if-generated" feature exercising `bolted-core`.
//!
//! A deliberately gnarly profile editor: a composite value object (`DateRange`), a relational
//! tier-2 rule (`corporate_email`), an async uniqueness check, and live rebase with field-level
//! conflicts. The invariant suite (`docs/CONFORMANCE.md`, C01–C21) lives in `tests/`.
#![forbid(unsafe_code)]

pub mod profile;
pub mod value_types;

pub use profile::{Profile, ProfileCheck, ProfileDraft, ProfileField, ProfileStash, ProfileStore};
pub use value_types::{
    Date, DateRange, DateRangeError, Email, EmailError, PersonName, PersonNameError, Username,
    UsernameError,
};
