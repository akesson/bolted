//! `fixture-note` — the *second* implementor of the conformance fixture, and the only reason step 08
//! is entitled to call the suite "generic".
//!
//! It is deliberately the opposite of `fixture-profile` in every optional dimension: two plain text
//! fields, **no composite value object, no tier-2 rule, no async check**. Everything a Bolted feature
//! may have and need not.
//!
//! A conformance suite with one implementor proves nothing about genericity — the trait simply grows
//! whatever shape that implementor has, and the tests still read as if they were about it. This crate
//! is the falsifier. What it cannot implement, the suite had no business demanding.
#![forbid(unsafe_code)]

use bolted_core::{
    CommitError, Constraint, Draft, DraftStatus, ErrorData, Field, FieldStash, Stashable, Store,
    StoreDraft, ValidationReport, Validity, Value,
};

// =================================================================================================
// Tier 1: two value types, both `Raw = String`, both sanitizing.
// =================================================================================================

/// Trim; 1..=40 chars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleError {
    Blank,
    TooLong { max: u32, actual: u32 },
}

impl Title {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Value for Title {
    type Raw = String;
    type Error = TitleError;

    fn try_new(raw: String) -> Result<Self, TitleError> {
        let s = raw.trim();
        let len = s.chars().count() as u32;
        if len == 0 {
            return Err(TitleError::Blank);
        }
        if len > 40 {
            return Err(TitleError::TooLong {
                max: 40,
                actual: len,
            });
        }
        Ok(Title(s.to_string()))
    }

    fn into_raw(self) -> String {
        self.0
    }

    fn constraints() -> &'static [Constraint] {
        &[Constraint::LenChars { min: 1, max: 40 }]
    }
}

impl From<TitleError> for ErrorData {
    fn from(e: TitleError) -> Self {
        match e {
            TitleError::Blank => ErrorData::new("blank"),
            TitleError::TooLong { max, actual } => ErrorData {
                key: "too_long",
                params: vec![("max", max.to_string()), ("actual", actual.to_string())],
            },
        }
    }
}

/// Trim; 1..=200 chars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyError {
    Blank,
    TooLong { max: u32, actual: u32 },
}

impl Body {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Value for Body {
    type Raw = String;
    type Error = BodyError;

    fn try_new(raw: String) -> Result<Self, BodyError> {
        let s = raw.trim();
        let len = s.chars().count() as u32;
        if len == 0 {
            return Err(BodyError::Blank);
        }
        if len > 200 {
            return Err(BodyError::TooLong {
                max: 200,
                actual: len,
            });
        }
        Ok(Body(s.to_string()))
    }

    fn into_raw(self) -> String {
        self.0
    }

    fn constraints() -> &'static [Constraint] {
        &[Constraint::LenChars { min: 1, max: 200 }]
    }
}

impl From<BodyError> for ErrorData {
    fn from(e: BodyError) -> Self {
        match e {
            BodyError::Blank => ErrorData::new("blank"),
            BodyError::TooLong { max, actual } => ErrorData {
                key: "too_long",
                params: vec![("max", max.to_string()), ("actual", actual.to_string())],
            },
        }
    }
}

// =================================================================================================
// The entity, its fields, its draft.
// =================================================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub title: Title,
    pub body: Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteField {
    Title,
    Body,
}

pub type NoteStore = Store<NoteDraft>;

#[derive(Debug, Clone, PartialEq)]
pub struct NoteStash {
    pub title: FieldStash<String>,
    pub body: FieldStash<String>,
    pub base_version: u64,
    pub orphaned: bool,
}

pub struct NoteDraft {
    pub title: Field<Title>,
    pub body: Field<Body>,
    status: DraftStatus,
    base_version: u64,
}

impl NoteDraft {
    pub fn try_set_title(&mut self, raw: String) -> Result<(), TitleError> {
        self.title.try_set(raw)
    }

    pub fn try_set_body(&mut self, raw: String) -> Result<(), BodyError> {
        self.body.try_set(raw)
    }
}

/// Tier-1 error for a field's current validity. `Unset` → `required`, a *field*-level judgement the
/// value type cannot make (D13).
fn tier1_error<V: Value>(field: &Field<V>) -> Option<ErrorData> {
    match field.validity() {
        Validity::Valid(_) => None,
        Validity::Invalid { .. } => field.invalid_error(),
        Validity::Unset => Some(ErrorData::new("required")),
    }
}

impl Draft for NoteDraft {
    type Entity = Note;
    type FieldId = NoteField;

    fn status(&self) -> DraftStatus {
        self.status
    }

    fn base_version(&self) -> u64 {
        self.base_version
    }

    fn dirty_fields(&self) -> Vec<NoteField> {
        let mut out = Vec::new();
        if self.title.is_dirty() {
            out.push(NoteField::Title);
        }
        if self.body.is_dirty() {
            out.push(NoteField::Body);
        }
        out
    }

    fn conflicts(&self) -> Vec<NoteField> {
        let mut out = Vec::new();
        if self.title.is_conflicted() {
            out.push(NoteField::Title);
        }
        if self.body.is_conflicted() {
            out.push(NoteField::Body);
        }
        out
    }

    /// Tier 1 only. There is no tier-2 rule and no async check — which is the whole point of this
    /// crate: `validate` is allowed to be this short, and the suite must not assume otherwise.
    fn validate(&self) -> ValidationReport<NoteField> {
        let mut report = ValidationReport::new();
        if let Some(e) = tier1_error(&self.title) {
            report.field_errors.push((NoteField::Title, e));
        }
        if let Some(e) = tier1_error(&self.body) {
            report.field_errors.push((NoteField::Body, e));
        }
        report
    }

    fn resolve_keep_mine(&mut self, field: NoteField) {
        match field {
            NoteField::Title => self.title.resolve_keep_mine(),
            NoteField::Body => self.body.resolve_keep_mine(),
        }
    }

    fn resolve_take_theirs(&mut self, field: NoteField) {
        match field {
            NoteField::Title => self.title.resolve_take_theirs(),
            NoteField::Body => self.body.resolve_take_theirs(),
        }
    }

    fn commit(self) -> Result<Note, (Self, CommitError<NoteField>)> {
        if matches!(self.status, DraftStatus::Orphaned) {
            return Err((self, CommitError::Orphaned));
        }
        let conflicts = self.conflicts();
        if !conflicts.is_empty() {
            return Err((self, CommitError::Conflicted { fields: conflicts }));
        }
        let report = self.validate();
        if !report.is_ok() {
            return Err((self, CommitError::Validation(report)));
        }
        match (self.title.value().cloned(), self.body.value().cloned()) {
            (Some(title), Some(body)) => Ok(Note { title, body }),
            _ => {
                let report = self.validate();
                Err((self, CommitError::Validation(report)))
            }
        }
    }
}

impl StoreDraft for NoteDraft {
    fn from_canonical(base: Option<&Note>, base_version: u64) -> Self {
        match base {
            Some(n) => NoteDraft {
                title: Field::from_base(n.title.clone()),
                body: Field::from_base(n.body.clone()),
                status: DraftStatus::Live,
                base_version,
            },
            None => NoteDraft {
                title: Field::new_unset(),
                body: Field::new_unset(),
                status: DraftStatus::Live,
                base_version,
            },
        }
    }

    fn rebase(&mut self, entity: &Note, version: u64) {
        if matches!(self.status, DraftStatus::Orphaned) {
            return;
        }
        self.title.rebase(entity.title.clone());
        self.body.rebase(entity.body.clone());
        self.base_version = version;
    }

    fn orphan(&mut self) {
        self.status = DraftStatus::Orphaned;
    }

    fn is_based(&self) -> bool {
        self.title.base().is_some() || self.body.base().is_some()
    }
}

impl Stashable for NoteDraft {
    type Stash = NoteStash;

    fn stash(&self) -> NoteStash {
        NoteStash {
            title: self.title.stash(),
            body: self.body.stash(),
            base_version: self.base_version,
            orphaned: matches!(self.status, DraftStatus::Orphaned),
        }
    }

    fn from_stash(stash: &NoteStash) -> Self {
        NoteDraft {
            title: Field::from_stash(&stash.title),
            body: Field::from_stash(&stash.body),
            status: if stash.orphaned {
                DraftStatus::Orphaned
            } else {
                DraftStatus::Live
            },
            base_version: stash.base_version,
        }
    }
}

const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<NoteStore>();
};
