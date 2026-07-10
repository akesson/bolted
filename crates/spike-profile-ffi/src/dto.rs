//! The monomorphic FFI projection of the `spike-profile` feature — the hand-written stand-in for
//! what `#[bolted::entity]`/`#[bolted::value]` would emit for the FFI boundary.
//!
//! Why any of this exists: `bolted_core::Field<V>` is generic, and BoltFFI `#[data]` forbids
//! generics, tuples, borrowed data and `&'static str`. So every generic thing in the core has to be
//! stamped out into a concrete, owned `#[data]` shape here — **one field-state family per value
//! type**, even when two value types share a raw representation (`String`). Counting these lines is
//! a deliverable (see the step-02 report): it is the honest per-field cost of the "drafts core-side
//! / snapshot-per-change" decisions.

use bolted_core::FieldStash;
use bolted_core::report::ErrorData as CoreErrorData;
use boltffi::*;
use spike_profile::ProfileStash;

// =================================================================================================
// Shared primitives
// =================================================================================================

/// Renamed from the core's `Date`: a `#[data] Date` lands in the generated Swift module next to
/// `Foundation.Date` and shadows it. FINDING: bolted-ffi needs a platform-stdlib name-collision
/// policy (Date, URL, Data, Error, …). Kept `PlainDate` here.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlainDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// The composite value object's raw/value shape, projected off the tuple `(Date, Date)` (tuples do
/// not cross). Also the shape the setter takes as *two arguments*, never a tuple.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlainDateRange {
    pub start: PlainDate,
    pub end: PlainDate,
}

/// Mirrors `spike_profile::ProfileField`.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileFieldId {
    Username,
    Name,
    Email,
    Availability,
}

/// Declared constraint metadata, mirroring `bolted_core::Constraint`. Crossing this lets the shell
/// derive `maxLength`, character counters and required markers from the SAME source the core
/// validates against — so there is no numeric constraint literal on the Swift side (ARCHITECTURE
/// §1). `Custom`'s `&'static str` projects to an owned `String`.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstraintFfi {
    Required,
    LenChars { min: u32, max: u32 },
    Custom { key: String },
}

/// A single localisable error param. The core's `(&'static str, String)` tuple cannot cross, so it
/// is projected to a named record.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub key: String,
    pub value: String,
}

/// The core's `ErrorData` projected: `key: &'static str` → `String`, `params: Vec<(…)>` → `Vec<Param>`.
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
                .map(|(k, v)| Param {
                    key: k.to_string(),
                    value: v,
                })
                .collect(),
        }
    }
}

// =================================================================================================
// Per-field display state — ONE FAMILY PER VALUE TYPE (the monomorphic projection of `Field<V>`).
//
// Username / PersonName / Email are structurally identical (raw = value = String): the generator
// stamps them out anyway, since it keys on the value type, not the raw type. Availability differs
// (raw = value = PlainDateRange). This near-duplication is a measured finding, not an accident.
// =================================================================================================

// ---- Username ----------------------------------------------------------------------------------
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsernameValidity {
    Unset,
    Valid { value: String },
    Invalid { raw: String, error: ErrorData },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsernameFieldSync {
    InSync,
    Conflicted {
        base: Option<String>,
        theirs: String,
    },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsernameFieldState {
    pub validity: UsernameValidity,
    pub sync: UsernameFieldSync,
    pub dirty: bool,
}

// ---- PersonName --------------------------------------------------------------------------------
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonNameValidity {
    Unset,
    Valid { value: String },
    Invalid { raw: String, error: ErrorData },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonNameFieldSync {
    InSync,
    Conflicted {
        base: Option<String>,
        theirs: String,
    },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonNameFieldState {
    pub validity: PersonNameValidity,
    pub sync: PersonNameFieldSync,
    pub dirty: bool,
}

// ---- Email -------------------------------------------------------------------------------------
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmailValidity {
    Unset,
    Valid { value: String },
    Invalid { raw: String, error: ErrorData },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmailFieldSync {
    InSync,
    Conflicted {
        base: Option<String>,
        theirs: String,
    },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmailFieldState {
    pub validity: EmailValidity,
    pub sync: EmailFieldSync,
    pub dirty: bool,
}

// ---- Availability (composite value object) -----------------------------------------------------
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AvailabilityValidity {
    Unset,
    Valid {
        value: PlainDateRange,
    },
    Invalid {
        raw: PlainDateRange,
        error: ErrorData,
    },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AvailabilityFieldSync {
    InSync,
    Conflicted {
        base: Option<PlainDateRange>,
        theirs: PlainDateRange,
    },
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailabilityFieldState {
    pub validity: AvailabilityValidity,
    pub sync: AvailabilityFieldSync,
    pub dirty: bool,
}

// =================================================================================================
// Whole-draft snapshot (the `observe` verb's item; also the store's canonical stream item)
// =================================================================================================

#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftStatusFfi {
    Live,
    Orphaned,
}

/// The async uniqueness check's sub-state, projected from `bolted_core::CheckState` via the
/// `ProfileDraft::username_check_state()` getter added in step 03 (`Idle→Unchecked`,
/// `Pending→Pending`, `Done(Ok)→Passed`, `Done(Err)→Failed`). This closes step-02 finding 7: the
/// check is now observable as verdict *state*, not only as its `validate()` effect, so a shell can
/// render a spinner. It is core-owned verdict state, not a UI visibility policy (ARCHITECTURE §2).
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsernameCheckFfi {
    Unchecked,
    Pending,
    Passed,
    Failed { error: ErrorData },
}

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileSnapshot {
    pub username: UsernameFieldState,
    pub name: PersonNameFieldState,
    pub email: EmailFieldState,
    pub availability: AvailabilityFieldState,
    /// The async uniqueness check's observable sub-state (step-02 finding 7).
    pub username_check: UsernameCheckFfi,
    pub any_dirty: bool,
    pub conflicts: Vec<ProfileFieldId>,
    pub status: DraftStatusFfi,
    /// The draft's `base_version`, so a Swift subscriber can version-stamp a `snapshot()`-then-
    /// `subscribe()` sequence and detect a missed event in the gap (the subscribe-race probe).
    pub version: u64,
}

/// Raw values for seeding / replacing the canonical entity (`apply_canonical`). Each is validated
/// through the real value types; the composite range crosses as a record, never a tuple.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileValues {
    pub username: String,
    pub name: String,
    pub email: String,
    pub availability: PlainDateRange,
}

/// The verdict the foreign-implemented `UniquenessChecker` capability returns.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UniquenessVerdictFfi {
    Unique,
    Taken,
}

// =================================================================================================
// The draft stash (C20) — what a shell persists so an edit session survives process death.
//
// **Note the shape, against the field-state families above.** Those needed one struct per *value*
// type, because `Validity<V>` mentions `V`. The stash mentions only `V::Raw`, so three of the four
// fields collapse onto ONE `TextFieldStashFfi`. That is the cleanest evidence yet for ARCHITECTURE
// §9's "codegen dedup by raw type" (step 09): dedup is trivially right here and impossible above.
// =================================================================================================

/// `bolted_core::FieldStash<String>`. `Option` because a create-flow field has neither.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFieldStashFfi {
    pub raw: Option<String>,
    pub base: Option<String>,
}

/// `bolted_core::FieldStash<(Date, Date)>`, with the tuple projected onto a record as usual.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateRangeFieldStashFfi {
    pub raw: Option<PlainDateRange>,
    pub base: Option<PlainDateRange>,
}

/// `spike_profile::ProfileStash`. Carries no `sync` and no async verdict, on purpose — see C20.
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileStashFfi {
    pub username: TextFieldStashFfi,
    pub name: TextFieldStashFfi,
    pub email: TextFieldStashFfi,
    pub availability: DateRangeFieldStashFfi,
    pub base_version: u64,
    pub orphaned: bool,
}

// =================================================================================================
// Validation report (projection of `ValidationReport<ProfileField>`)
// =================================================================================================

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldErrorFfi {
    pub field: ProfileFieldId,
    pub error: ErrorData,
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleViolationFfi {
    pub rule: String,
    pub pins: Vec<ProfileFieldId>,
    pub error: ErrorData,
}
#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationReportFfi {
    pub field_errors: Vec<FieldErrorFfi>,
    pub rule_errors: Vec<RuleViolationFfi>,
}

// =================================================================================================
// Typed errors (#[error]) — these become Swift `throws` with associated data.
//
// Boilerplate note: every `#[error]` type hand-writes Display + std::error::Error +
// From<UnexpectedFfiCallbackError> (the demo does the same — the macro does NOT synthesise them).
// This per-error-type triple is part of the measured codegen cost.
// =================================================================================================

/// One-off fallback ctor for the `From<UnexpectedFfiCallbackError>` impls: that conversion only
/// fires if a *callback* Result unwinds unexpectedly, which none of these setter/submit errors do.
macro_rules! unexpected_callback_is {
    ($ty:ty, $variant:expr) => {
        impl From<UnexpectedFfiCallbackError> for $ty {
            fn from(_: UnexpectedFfiCallbackError) -> Self {
                $variant
            }
        }
    };
}

#[error]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsernameErrorFfi {
    TooShort { min: u32, actual: u32 },
    TooLong { max: u32, actual: u32 },
    InvalidChars,
}
impl std::fmt::Display for UsernameErrorFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid username: {self:?}")
    }
}
impl std::error::Error for UsernameErrorFfi {}
unexpected_callback_is!(UsernameErrorFfi, UsernameErrorFfi::InvalidChars);

impl From<spike_profile::UsernameError> for UsernameErrorFfi {
    fn from(e: spike_profile::UsernameError) -> Self {
        use spike_profile::UsernameError as E;
        match e {
            E::TooShort { min, actual } => UsernameErrorFfi::TooShort { min, actual },
            E::TooLong { max, actual } => UsernameErrorFfi::TooLong { max, actual },
            E::InvalidChars => UsernameErrorFfi::InvalidChars,
        }
    }
}

#[error]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersonNameErrorFfi {
    TooShort { min: u32, actual: u32 },
    TooLong { max: u32, actual: u32 },
}
impl std::fmt::Display for PersonNameErrorFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid name: {self:?}")
    }
}
impl std::error::Error for PersonNameErrorFfi {}
unexpected_callback_is!(
    PersonNameErrorFfi,
    PersonNameErrorFfi::TooShort { min: 0, actual: 0 }
);

impl From<spike_profile::PersonNameError> for PersonNameErrorFfi {
    fn from(e: spike_profile::PersonNameError) -> Self {
        use spike_profile::PersonNameError as E;
        match e {
            E::TooShort { min, actual } => PersonNameErrorFfi::TooShort { min, actual },
            E::TooLong { max, actual } => PersonNameErrorFfi::TooLong { max, actual },
        }
    }
}

#[error]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmailErrorFfi {
    Invalid,
}
impl std::fmt::Display for EmailErrorFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid email")
    }
}
impl std::error::Error for EmailErrorFfi {}
unexpected_callback_is!(EmailErrorFfi, EmailErrorFfi::Invalid);

impl From<spike_profile::EmailError> for EmailErrorFfi {
    fn from(e: spike_profile::EmailError) -> Self {
        match e {
            spike_profile::EmailError::Invalid => EmailErrorFfi::Invalid,
        }
    }
}

#[error]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DateRangeErrorFfi {
    StartAfterEnd { start: PlainDate, end: PlainDate },
}
impl std::fmt::Display for DateRangeErrorFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid date range: {self:?}")
    }
}
impl std::error::Error for DateRangeErrorFfi {}
unexpected_callback_is!(
    DateRangeErrorFfi,
    DateRangeErrorFfi::StartAfterEnd {
        start: PlainDate {
            year: 0,
            month: 0,
            day: 0
        },
        end: PlainDate {
            year: 0,
            month: 0,
            day: 0
        },
    }
);

impl From<spike_profile::DateRangeError> for DateRangeErrorFfi {
    fn from(e: spike_profile::DateRangeError) -> Self {
        match e {
            spike_profile::DateRangeError::StartAfterEnd { start, end } => {
                DateRangeErrorFfi::StartAfterEnd {
                    start: to_plain_date(start),
                    end: to_plain_date(end),
                }
            }
        }
    }
}

/// Mirrors `bolted_core::SubmitError<ProfileField>`, plus one FFI-only lifecycle variant.
///
/// FINDING (§4): `AlreadySubmitted` has no analogue in core `SubmitError`. It exists because the
/// foreign *handle* outlives the core-side draft: after `submit` consumes the draft, the Swift
/// object is still alive and can call `submit` again. Core never has this problem (submit consumes
/// the only owner). The FFI boundary needs a lifecycle error the core does not.
#[error]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitErrorFfi {
    Validation { report: ValidationReportFfi },
    Conflicted { fields: Vec<ProfileFieldId> },
    Orphaned,
    AlreadySubmitted,
}
impl std::fmt::Display for SubmitErrorFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmitErrorFfi::Validation { .. } => write!(f, "submit failed validation"),
            SubmitErrorFfi::Conflicted { .. } => write!(f, "submit blocked by conflicts"),
            SubmitErrorFfi::Orphaned => write!(f, "submit of an orphaned draft"),
            SubmitErrorFfi::AlreadySubmitted => write!(f, "draft already submitted"),
        }
    }
}
impl std::error::Error for SubmitErrorFfi {}
unexpected_callback_is!(SubmitErrorFfi, SubmitErrorFfi::AlreadySubmitted);

// =================================================================================================
// Small value conversions
// =================================================================================================

pub fn to_plain_date(d: spike_profile::Date) -> PlainDate {
    PlainDate {
        year: d.year,
        month: d.month,
        day: d.day,
    }
}

pub fn to_core_date(d: PlainDate) -> spike_profile::Date {
    spike_profile::Date::new(d.year, d.month, d.day)
}

pub fn to_plain_range(r: &spike_profile::DateRange) -> PlainDateRange {
    PlainDateRange {
        start: to_plain_date(r.start()),
        end: to_plain_date(r.end()),
    }
}

// ---- stash conversions (C20) ------------------------------------------------------------------

pub fn to_text_stash_ffi(s: &FieldStash<String>) -> TextFieldStashFfi {
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

pub fn to_range_stash_ffi(
    s: &FieldStash<(spike_profile::Date, spike_profile::Date)>,
) -> DateRangeFieldStashFfi {
    let project = |(start, end): &(spike_profile::Date, spike_profile::Date)| PlainDateRange {
        start: to_plain_date(*start),
        end: to_plain_date(*end),
    };
    DateRangeFieldStashFfi {
        raw: s.raw.as_ref().map(project),
        base: s.base.as_ref().map(project),
    }
}

pub fn to_core_range_stash(
    s: &DateRangeFieldStashFfi,
) -> FieldStash<(spike_profile::Date, spike_profile::Date)> {
    let project = |r: &PlainDateRange| (to_core_date(r.start), to_core_date(r.end));
    FieldStash {
        raw: s.raw.as_ref().map(project),
        base: s.base.as_ref().map(project),
    }
}

pub fn to_stash_ffi(s: &ProfileStash) -> ProfileStashFfi {
    ProfileStashFfi {
        username: to_text_stash_ffi(&s.username),
        name: to_text_stash_ffi(&s.name),
        email: to_text_stash_ffi(&s.email),
        availability: to_range_stash_ffi(&s.availability),
        base_version: s.base_version,
        orphaned: s.orphaned,
    }
}

pub fn to_core_stash(s: &ProfileStashFfi) -> ProfileStash {
    ProfileStash {
        username: to_core_text_stash(&s.username),
        name: to_core_text_stash(&s.name),
        email: to_core_text_stash(&s.email),
        availability: to_core_range_stash(&s.availability),
        base_version: s.base_version,
        orphaned: s.orphaned,
    }
}
