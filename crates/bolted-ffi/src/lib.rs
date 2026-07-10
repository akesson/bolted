//! `bolted-ffi` — the DTOs and projections every generated FFI layer shares.
//!
//! `bolted_core::Field<V>` is generic and BoltFFI's `#[data]` forbids generics, so every generic thing
//! in the core has to be stamped into a concrete, owned shape before it can cross. `spike-profile-ffi`
//! stamped **one family per value type**, and three of its four families — `Username`, `PersonName`,
//! `Email` — came out structurally identical, because all three have `Raw = String`.
//!
//! **D24 stamps them once, here.** `Validity<V>` mentions `V`; `TextValidity` mentions only `V::Raw`.
//! The axis that varies across the boundary is the *raw* type, not the value type, which is exactly
//! the residue D19 left for this step. What is lost is per-field type naming: Swift sees
//! `snapshot.username: TextFieldState`. The field name carried the meaning; the type name never did.
//!
//! Error types stay per value (`UsernameErrorFfi` ≠ `EmailErrorFfi`) — they have different variants,
//! and a typed `throws` is a feature, not an accident.
//!
//! This crate is visible to bindgen only because it is a **direct dependency that itself depends on
//! boltffi** (step 10, M0, row 5). A feature's FFI crate that does not name `bolted_ffi` in its
//! `[dependencies]` gets none of these types, and the failure is silent.
//!
//! NOTE: no `#![forbid(unsafe_code)]` — `#[data]`/`#[error]` expand to code containing `unsafe`, and
//! the forbid would reject it. The FFI boundary is exactly where the no-unsafe discipline stops.

use boltffi::*;

use bolted_core::report::ErrorData as CoreErrorData;
use bolted_core::{
    CheckState, Constraint, DraftStatus, Field, FieldStash, SyncState, Validity, Value,
};

// =================================================================================================
// Errors as data (never strings)
// =================================================================================================

/// A single localisable error param. The core's `(&'static str, String)` tuple cannot cross, so it is
/// projected onto a named record.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub key: String,
    pub value: String,
}

/// The core's `ErrorData` projected: `key: &'static str` → `String`, `params` → `Vec<Param>`.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorData {
    pub key: String,
    pub params: Vec<Param>,
}

impl From<CoreErrorData> for ErrorData {
    fn from(core: CoreErrorData) -> Self {
        ErrorData {
            key: core.key.to_string(),
            params: core
                .params
                .into_iter()
                .map(|(key, value)| Param {
                    key: key.to_string(),
                    value,
                })
                .collect(),
        }
    }
}

/// `V::Error` → the wire shape, for any value type.
pub fn error_data<E: Into<CoreErrorData>>(e: E) -> ErrorData {
    ErrorData::from(e.into())
}

// =================================================================================================
// Declared constraint metadata
// =================================================================================================

/// Mirrors `bolted_core::Constraint`. Crossing this is what lets a shell derive `maxLength`,
/// character counters and required markers from the same source the core validates against — so there
/// is no numeric constraint literal in Swift or Kotlin (ARCHITECTURE §1).
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstraintFfi {
    Required,
    LenChars { min: u32, max: u32 },
    Custom { key: String },
}

pub fn constraint(c: Constraint) -> ConstraintFfi {
    match c {
        Constraint::Required => ConstraintFfi::Required,
        Constraint::LenChars { min, max } => ConstraintFfi::LenChars { min, max },
        Constraint::Custom(key) => ConstraintFfi::Custom {
            key: key.to_string(),
        },
    }
}

// =================================================================================================
// Draft lifecycle
// =================================================================================================

#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftStatusFfi {
    Live,
    Orphaned,
}

pub fn draft_status(status: DraftStatus) -> DraftStatusFfi {
    match status {
        DraftStatus::Live => DraftStatusFfi::Live,
        DraftStatus::Orphaned => DraftStatusFfi::Orphaned,
    }
}

/// **D23.** A mutating call on a draft the store no longer holds.
///
/// Reachable today, and today it lies: C17 says a successful `submit` releases the draft while the
/// foreign object survives it, so every setter takes the `draft_mut(id) → None` branch and returns
/// `Ok(())`. A silent no-op. This type is what a generated mutator raises instead.
///
/// It does **not** cover the other hazard. If the foreign object itself has been released — Kotlin's
/// `close()`, which frees the Rust object — the handle is a dangling pointer and no Rust of ours runs
/// before it is dereferenced. Generated bindings hold a `__boltffi_closed` flag and never consult it;
/// that is reported upstream, and it is not fixable from this side.
#[error]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftClosedFfi {
    /// The draft was submitted (C17) or closed (C18). Its edit session is over.
    DraftClosed,
}

impl std::fmt::Display for DraftClosedFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the draft is no longer live: it was submitted or closed")
    }
}
impl std::error::Error for DraftClosedFfi {}
impl From<UnexpectedFfiCallbackError> for DraftClosedFfi {
    fn from(_: UnexpectedFfiCallbackError) -> Self {
        DraftClosedFfi::DraftClosed
    }
}

/// D27 — the wholesale, typed refusal a versioned stash envelope raises when it cannot be trusted.
///
/// A persisted stash is the first **untrusted input** in the system (bytes the OS held while we were
/// dead, possibly written by an *older version of this app*). D27 makes the stash a *versioned
/// envelope*: the schema version is stamped into the generated DTO at write time, and `restore`
/// gates on it — parse-don't-validate. A version this build does not recognise is refused **as a
/// whole**, before any field is trusted; the shell then starts a fresh edit session and, because
/// this is a typed error rather than a silent `null`, can tell "the stash was refused" from "there
/// was no stash". Per-field degradation (C23) is the *other* mechanism, and it only applies *inside*
/// an envelope that parsed.
#[error]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StashRefusedFfi {
    /// The stash was written under a schema version this build does not accept. `stashed` is the
    /// version carried in the bytes; `expected` is what this binary writes and reads. A tightened
    /// constraint between app versions is the realistic cause (see the step-07 report, and D27's
    /// build-time `bolted-check` constraint-semver event in Phase 4).
    SchemaVersion { stashed: u32, expected: u32 },
}

impl std::fmt::Display for StashRefusedFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StashRefusedFfi::SchemaVersion { stashed, expected } => write!(
                f,
                "stash refused: written under schema version {stashed}, this build expects {expected}"
            ),
        }
    }
}
impl std::error::Error for StashRefusedFfi {}

// =================================================================================================
// The async check's observable sub-state (D18's `Checked::check_state`, projected)
// =================================================================================================

/// `Idle → Unchecked`, `Pending → Pending`, `Done(Ok) → Passed`, `Done(Err) → Failed`.
///
/// Core-owned verdict state, not a UI visibility policy (§2). Shared across every check of every
/// feature: `CheckState<Result<(), ErrorData>>` mentions no value type at all.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckStateFfi {
    Unchecked,
    Pending,
    Passed,
    Failed { error: ErrorData },
}

/// What a foreign checker answers. `Fail` carries no error: the l10n key is declared, in
/// `#[check(failed_key = "…")]`, next to `pending_key` and `required_key`.
///
/// The alternative — `Fail { error: ErrorData }`, letting the shell supply the key — is more general
/// and was rejected for the same reason step 09 gave `custom(..)` a `key` override: a localisation key
/// is part of the contract, and the contract is the declaration. A key that lives in Swift cannot be
/// checked against a Kotlin strings file.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckVerdictFfi {
    Pass,
    Fail,
}

pub fn check_state(state: &CheckState<Result<(), CoreErrorData>>) -> CheckStateFfi {
    match state {
        CheckState::Idle => CheckStateFfi::Unchecked,
        CheckState::Pending { .. } => CheckStateFfi::Pending,
        CheckState::Done { verdict: Ok(()) } => CheckStateFfi::Passed,
        CheckState::Done { verdict: Err(e) } => CheckStateFfi::Failed {
            error: ErrorData::from(e.clone()),
        },
    }
}

// =================================================================================================
// The text field-state family — one per RAW type, not one per value type (D24)
// =================================================================================================

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextValidity {
    Unset,
    Valid { value: String },
    Invalid { raw: String, error: ErrorData },
}

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextFieldSync {
    InSync,
    Conflicted {
        base: Option<String>,
        theirs: String,
    },
}

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFieldState {
    pub validity: TextValidity,
    pub sync: TextFieldSync,
    pub dirty: bool,
}

/// Project any `Field<V>` whose raw form is a `String`.
///
/// Generic, and therefore written once at rung 1 — the FFI's monomorphic surface is the *DTO*, not the
/// function that fills it. `into_raw` is what makes this work for a value type this crate has never
/// heard of; it is the only thing `Value` promises about `V`'s contents.
pub fn text_field_state<V: Value<Raw = String>>(f: &Field<V>) -> TextFieldState {
    let validity = match f.validity() {
        Validity::Unset => TextValidity::Unset,
        Validity::Valid(v) => TextValidity::Valid {
            value: v.clone().into_raw(),
        },
        Validity::Invalid { raw, error } => TextValidity::Invalid {
            raw: raw.clone(),
            error: error_data(error.clone()),
        },
    };
    let sync = match f.sync() {
        SyncState::InSync => TextFieldSync::InSync,
        // The DTO keeps the full 3-way shape for shells; the core no longer stores the ancestor
        // twice, so it is read from the field itself (step-01 F7).
        SyncState::Conflicted { theirs } => TextFieldSync::Conflicted {
            base: f.base().map(|b| b.clone().into_raw()),
            theirs: theirs.clone().into_raw(),
        },
    };
    TextFieldState {
        validity,
        sync,
        dirty: f.is_dirty(),
    }
}

/// The text a foreign checker should be asked about: the parsed value if there is one, the rejected
/// raw if there is not, and `""` for an unset field.
///
/// An *invalid* field is still worth checking — the user typed `ab` and it is too short, but a
/// uniqueness check on `ab` is what makes the spinner appear before they finish typing. C13 then binds
/// the verdict to the value that produced it.
pub fn text_of<V: Value<Raw = String>>(f: &Field<V>) -> String {
    match f.validity() {
        Validity::Valid(v) => v.clone().into_raw(),
        Validity::Invalid { raw, .. } => raw.clone(),
        Validity::Unset => String::new(),
    }
}

// =================================================================================================
// The stash (C20) — what a shell persists so an edit session survives process death
// =================================================================================================

/// `bolted_core::FieldStash<String>`. `Option` because a create-flow field has neither raw nor base.
///
/// Note the shape against the field-state family above. Those needed `Validity<V>`, which mentions
/// `V`; the stash mentions only `V::Raw`. `spike-profile-ffi` already collapsed its three text stashes
/// onto one struct and its comment called that "the cleanest evidence yet" for dedup by raw type.
/// D24 says the same is true one level up.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFieldStashFfi {
    pub raw: Option<String>,
    pub base: Option<String>,
}

pub fn text_stash(s: &FieldStash<String>) -> TextFieldStashFfi {
    TextFieldStashFfi {
        raw: s.raw.clone(),
        base: s.base.clone(),
    }
}

pub fn to_core_text_stash(s: &TextFieldStashFfi) -> FieldStash<String> {
    FieldStash {
        raw: s.raw.clone(),
        base: s.base.clone(),
    }
}
