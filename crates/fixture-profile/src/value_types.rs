//! Hand-written value types — each is exactly what `#[bolted::value]` would generate: sanitize
//! first, then validate; errors are keyed data with params. `From<XError> for ErrorData` is the
//! bridge the entity layer uses to build reports (kept here, not on the core `Value` trait).

use bolted_core::{Constraint, ErrorData, Value};

/// Minimal ordinal date (no chrono). Fields ordered most-significant-first so the derived `Ord`
/// is a correct chronological comparison. No calendar validation — out of scope for the spike.
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

// ---------------------------------------------------------------------------------------------
// Username — trim; 3..=20 chars; ASCII alphanumeric + '_'.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Username(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsernameError {
    TooShort { min: u32, actual: u32 },
    TooLong { max: u32, actual: u32 },
    InvalidChars,
}

impl Username {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Value for Username {
    type Raw = String;
    type Error = UsernameError;

    fn try_new(raw: String) -> Result<Self, UsernameError> {
        let s = raw.trim();
        let len = s.chars().count() as u32;
        if len < 3 {
            return Err(UsernameError::TooShort {
                min: 3,
                actual: len,
            });
        }
        if len > 20 {
            return Err(UsernameError::TooLong {
                max: 20,
                actual: len,
            });
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(UsernameError::InvalidChars);
        }
        Ok(Username(s.to_string()))
    }

    fn into_raw(self) -> String {
        self.0
    }

    fn constraints() -> &'static [Constraint] {
        &[
            Constraint::LenChars { min: 3, max: 20 },
            Constraint::Custom("ascii_alnum_underscore"),
        ]
    }
}

impl From<UsernameError> for ErrorData {
    fn from(e: UsernameError) -> Self {
        match e {
            UsernameError::TooShort { min, actual } => ErrorData {
                key: "too_short",
                params: vec![("min", min.to_string()), ("actual", actual.to_string())],
            },
            UsernameError::TooLong { max, actual } => ErrorData {
                key: "too_long",
                params: vec![("max", max.to_string()), ("actual", actual.to_string())],
            },
            UsernameError::InvalidChars => ErrorData::new("invalid_chars"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// PersonName — trim; 1..=30 chars.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonName(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonNameError {
    TooShort { min: u32, actual: u32 },
    TooLong { max: u32, actual: u32 },
}

impl PersonName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Value for PersonName {
    type Raw = String;
    type Error = PersonNameError;

    fn try_new(raw: String) -> Result<Self, PersonNameError> {
        let s = raw.trim();
        let len = s.chars().count() as u32;
        if len == 0 {
            return Err(PersonNameError::TooShort {
                min: 1,
                actual: len,
            });
        }
        if len > 30 {
            return Err(PersonNameError::TooLong {
                max: 30,
                actual: len,
            });
        }
        Ok(PersonName(s.to_string()))
    }

    fn into_raw(self) -> String {
        self.0
    }

    fn constraints() -> &'static [Constraint] {
        &[Constraint::LenChars { min: 1, max: 30 }]
    }
}

impl From<PersonNameError> for ErrorData {
    fn from(e: PersonNameError) -> Self {
        match e {
            PersonNameError::TooShort { min, actual } => ErrorData {
                key: "too_short",
                params: vec![("min", min.to_string()), ("actual", actual.to_string())],
            },
            PersonNameError::TooLong { max, actual } => ErrorData {
                key: "too_long",
                params: vec![("max", max.to_string()), ("actual", actual.to_string())],
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Email — trim + lowercase; must contain '@' with non-empty local and domain parts.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailError {
    Invalid,
}

impl Email {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The domain part (everything after the first `@`). Always present for a valid `Email`.
    pub fn domain(&self) -> &str {
        self.0.split_once('@').map(|(_, d)| d).unwrap_or("")
    }
}

impl Value for Email {
    type Raw = String;
    type Error = EmailError;

    fn try_new(raw: String) -> Result<Self, EmailError> {
        let s = raw.trim().to_lowercase();
        match s.split_once('@') {
            Some((local, domain)) if !local.is_empty() && !domain.is_empty() => Ok(Email(s)),
            _ => Err(EmailError::Invalid),
        }
    }

    fn into_raw(self) -> String {
        self.0
    }

    fn constraints() -> &'static [Constraint] {
        &[Constraint::Custom("email")]
    }
}

impl From<EmailError> for ErrorData {
    fn from(e: EmailError) -> Self {
        match e {
            EmailError::Invalid => ErrorData::new("invalid_email"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// DateRange — composite value object: Raw = (Date, Date), invariant start <= end.
// ---------------------------------------------------------------------------------------------

/// Deliberately **not** `Copy`, even though it easily could be: generated checkout/rebase code
/// clones every field uniformly, and `clippy::clone_on_copy` rejects `.clone()` on a `Copy` field
/// under `-D warnings`. Value objects are `Clone`-only so codegen stays uniform (ARCHITECTURE §8,
/// step-01 friction F4). `Date` is a raw part, not a `Value`, and stays `Copy`.
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
