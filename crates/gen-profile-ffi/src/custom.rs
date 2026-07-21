//! The escape hatch: everything the generator refuses to guess.
//!
//! `Profile::availability` is a `DateRange` — a **composite** value object, whose raw form is
//! `(Date, Date)` and whose invariant spans two parts. D20 keeps composites out of `#[bolted::value]`,
//! so `gen-profile` hand-writes its `Value` impl, and `bolted-ffi-gen` has no declaration to read.
//!
//! The generator's response is not to guess. It emits `use crate::custom::*;` and references four
//! types and six functions by name. Every one of them is below. If one were missing, `mise run check`
//! would fail to **compile** — rung 2 — rather than emit a binding that quietly lost a field.
//!
//! The names are the generator's, derived from the *field* (`availability`), not from the value type.
//! That is why `PlainDateRange` is spelled `AvailabilityRaw` here: a second composite field of the same
//! type would need its own projection anyway, since only the field knows what it means.
//!
//! Contrast this file with `fixture-profile-ffi/src/dto.rs`, where **all four** fields cost this much.

use bolted_core::{Field, FieldStash, SyncState, Validity};
use bolted_ffi::ErrorData;
use boltffi::*;
use gen_profile::{Date, DateRange, DateRangeError};

// =================================================================================================
// The wire shapes
// =================================================================================================

/// Renamed from the core's `Date`: a `#[data] Date` lands in the generated Swift module next to
/// `Foundation.Date` and shadows it. `bolted-ffi` still owes a platform-stdlib name-collision policy
/// (`Date`, `URL`, `Data`, `Error`) — step 11.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlainDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// `DateRange::Raw` = `(Date, Date)`. Tuples do not cross, so it is a record.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvailabilityRaw {
    pub start: PlainDate,
    pub end: PlainDate,
}

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AvailabilityValidity {
    Unset,
    Valid {
        value: AvailabilityRaw,
    },
    Invalid {
        raw: AvailabilityRaw,
        error: ErrorData,
    },
}

#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvailabilityFieldSync {
    InSync,
    Conflicted {
        base: Option<AvailabilityRaw>,
        theirs: AvailabilityRaw,
    },
}

#[data]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailabilityFieldState {
    pub validity: AvailabilityValidity,
    pub sync: AvailabilityFieldSync,
    pub dirty: bool,
}

#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvailabilityStash {
    pub raw: Option<AvailabilityRaw>,
    pub base: Option<AvailabilityRaw>,
}

/// The setter's typed refusal. `DraftClosed` is D23, and a custom field must carry it too: the
/// generated setter has exactly one error type to return, whoever wrote it.
#[error]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvailabilityErrorFfi {
    StartAfterEnd { start: PlainDate, end: PlainDate },
    DraftClosed,
}

impl std::fmt::Display for AvailabilityErrorFfi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid availability: {self:?}")
    }
}
impl std::error::Error for AvailabilityErrorFfi {}
impl From<UnexpectedFfiCallbackError> for AvailabilityErrorFfi {
    fn from(_: UnexpectedFfiCallbackError) -> Self {
        AvailabilityErrorFfi::DraftClosed
    }
}

// =================================================================================================
// The six functions `generated.rs` calls by name
// =================================================================================================

/// `availability_state` — project the live `Field<DateRange>`.
pub fn availability_state(f: &Field<DateRange>) -> AvailabilityFieldState {
    let validity = match f.validity() {
        Validity::Unset => AvailabilityValidity::Unset,
        Validity::Valid(v) => AvailabilityValidity::Valid { value: wire(v) },
        Validity::Invalid { raw, error } => AvailabilityValidity::Invalid {
            raw: wire_raw(*raw),
            error: bolted_ffi::error_data(error.clone()),
        },
    };
    let sync = match f.sync() {
        SyncState::InSync => AvailabilityFieldSync::InSync,
        SyncState::Conflicted { theirs } => AvailabilityFieldSync::Conflicted {
            base: f.base().map(wire),
            theirs: wire(theirs),
        },
    };
    AvailabilityFieldState {
        validity,
        sync,
        dirty: f.is_dirty(),
    }
}

/// `availability_raw` — wire → `DateRange::Raw`.
pub fn availability_raw(r: AvailabilityRaw) -> (Date, Date) {
    (core_date(r.start), core_date(r.end))
}

/// `availability_stash` — `FieldStash<(Date, Date)>` → wire.
pub fn availability_stash(s: &FieldStash<(Date, Date)>) -> AvailabilityStash {
    AvailabilityStash {
        raw: s.raw.map(wire_raw),
        base: s.base.map(wire_raw),
    }
}

/// `availability_from_stash` — wire → `FieldStash<(Date, Date)>`.
pub fn availability_from_stash(s: &AvailabilityStash) -> FieldStash<(Date, Date)> {
    FieldStash {
        raw: s.raw.map(availability_raw),
        base: s.base.map(availability_raw),
    }
}

/// `availability_error` — `DateRange::Error` → the setter's FFI error.
pub fn availability_error(e: DateRangeError) -> AvailabilityErrorFfi {
    match e {
        DateRangeError::StartAfterEnd { start, end } => AvailabilityErrorFfi::StartAfterEnd {
            start: plain_date(start),
            end: plain_date(end),
        },
    }
}

/// `availability_closed` — D23, for this field's error type.
pub fn availability_closed() -> AvailabilityErrorFfi {
    AvailabilityErrorFfi::DraftClosed
}

// =================================================================================================
// Small conversions
// =================================================================================================

fn wire(r: &DateRange) -> AvailabilityRaw {
    AvailabilityRaw {
        start: plain_date(r.start()),
        end: plain_date(r.end()),
    }
}

fn wire_raw((start, end): (Date, Date)) -> AvailabilityRaw {
    AvailabilityRaw {
        start: plain_date(start),
        end: plain_date(end),
    }
}

fn plain_date(d: Date) -> PlainDate {
    PlainDate {
        year: d.year,
        month: d.month,
        day: d.day,
    }
}

fn core_date(d: PlainDate) -> Date {
    Date::new(d.year, d.month, d.day)
}
