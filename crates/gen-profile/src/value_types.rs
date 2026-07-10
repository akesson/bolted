//! What `#[bolted::value]` does **not** generate, and the predicates it calls.
//!
//! `DateRange` is the composite value object: its raw is `(Date, Date)` and its invariant relates two
//! parts. D20 keeps it hand-written — §5's sketch says `#[bolted::value]` declares a *newtype*, and
//! `try_new` here is one comparison that no DSL would improve. What a composite needs (struct-shaped
//! parts, a tuple raw, a cross-field invariant) is a second macro shape justified by exactly one
//! example, and step 09 has no evidence about which way it should go.
//!
//! The two predicates below are the `custom(..)` validators. They are ordinary functions the compiler
//! checks, which is the whole reason `custom` takes a path rather than a pattern or an expression:
//! the macro must never become the place where "what is valid" is decided.

use bolted_core::{Constraint, ErrorData, Value};

/// Minimal ordinal date (no chrono). Fields ordered most-significant-first so the derived `Ord` is a
/// correct chronological comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub fn new(year: u16, month: u8, day: u8) -> Self {
        Date { year, month, day }
    }
}

/// ASCII alphanumeric plus `_`. A `custom(..)` predicate: `fn(&str) -> bool`.
pub fn ascii_alnum_underscore(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// One `@`, with a non-empty local part and a non-empty domain.
pub fn email(s: &str) -> bool {
    matches!(s.split_once('@'), Some((local, domain)) if !local.is_empty() && !domain.is_empty())
}

// -------------------------------------------------------------------------------------------------
// DateRange — the composite. Deliberately not `Copy`, even though it easily could be (D8).
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateRange {
    start: Date,
    end: Date,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateRangeError {
    StartAfterEnd { start: Date, end: Date },
}

impl DateRange {
    pub fn start(&self) -> Date {
        self.start
    }

    pub fn end(&self) -> Date {
        self.end
    }
}

impl Value for DateRange {
    type Raw = (Date, Date);
    type Error = DateRangeError;

    fn try_new(raw: (Date, Date)) -> Result<Self, DateRangeError> {
        let (start, end) = raw;
        if start <= end {
            Ok(DateRange { start, end })
        } else {
            Err(DateRangeError::StartAfterEnd { start, end })
        }
    }

    fn into_raw(self) -> (Date, Date) {
        (self.start, self.end)
    }

    fn constraints() -> &'static [Constraint] {
        &[Constraint::Custom("start_le_end")]
    }
}

impl From<DateRangeError> for ErrorData {
    fn from(e: DateRangeError) -> Self {
        match e {
            DateRangeError::StartAfterEnd { start, end } => ErrorData {
                key: "range_reversed",
                params: vec![("start", fmt_date(start)), ("end", fmt_date(end))],
            },
        }
    }
}

fn fmt_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}
