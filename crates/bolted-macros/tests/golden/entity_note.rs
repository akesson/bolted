#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub title: Title,
    pub body: Body,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteField {
    Title,
    Body,
}
impl NoteField {
    /// `Required` is prepended because every entity field is non-optional — a *field*-level
    /// judgement no value type can make (D13) — followed by the value type's own intrinsics.
    pub fn constraints(self) -> ::std::vec::Vec<::bolted_core::Constraint> {
        let mut out = vec![::bolted_core::Constraint::Required];
        let intrinsic: &'static [::bolted_core::Constraint] = match self {
            NoteField::Title => <Title as ::bolted_core::Value>::constraints(),
            NoteField::Body => <Body as ::bolted_core::Value>::constraints(),
        };
        out.extend_from_slice(intrinsic);
        out
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct NoteStash {
    pub title: ::bolted_core::FieldStash<<Title as ::bolted_core::Value>::Raw>,
    pub body: ::bolted_core::FieldStash<<Body as ::bolted_core::Value>::Raw>,
    /// The store version this draft was last based on.
    pub base_version: u64,
    /// A draft orphaned before the process died stays orphaned (C11).
    pub orphaned: bool,
}
pub struct NoteDraft {
    pub title: ::bolted_core::Field<Title>,
    pub body: ::bolted_core::Field<Body>,
    status: ::bolted_core::DraftStatus,
    base_version: u64,
}
pub type NoteStore = ::bolted_core::Store<NoteDraft>;
impl NoteDraft {
    fn bolted_guard<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        f(self)
    }
}
impl NoteDraft {
    pub fn try_set_title(
        &mut self,
        raw: <Title as ::bolted_core::Value>::Raw,
    ) -> ::core::result::Result<(), <Title as ::bolted_core::Value>::Error> {
        self.bolted_guard(|__d| __d.title.try_set(raw))
    }
    pub fn try_set_body(
        &mut self,
        raw: <Body as ::bolted_core::Value>::Raw,
    ) -> ::core::result::Result<(), <Body as ::bolted_core::Value>::Error> {
        self.bolted_guard(|__d| __d.body.try_set(raw))
    }
}
#[doc(hidden)]
pub trait NoteRules {
    fn rules(&self) -> ::std::vec::Vec<::bolted_core::RuleViolation<NoteField>> {
        ::std::vec::Vec::new()
    }
}
impl NoteRules for NoteDraft {}
impl ::bolted_core::Draft for NoteDraft {
    type Entity = Note;
    type FieldId = NoteField;
    fn status(&self) -> ::bolted_core::DraftStatus {
        self.status
    }
    fn base_version(&self) -> u64 {
        self.base_version
    }
    fn dirty_fields(&self) -> ::std::vec::Vec<NoteField> {
        let mut out = ::std::vec::Vec::new();
        if self.title.is_dirty() {
            out.push(NoteField::Title);
        }
        if self.body.is_dirty() {
            out.push(NoteField::Body);
        }
        out
    }
    fn conflicts(&self) -> ::std::vec::Vec<NoteField> {
        let mut out = ::std::vec::Vec::new();
        if self.title.is_conflicted() {
            out.push(NoteField::Title);
        }
        if self.body.is_conflicted() {
            out.push(NoteField::Body);
        }
        out
    }
    fn validate(&self) -> ::bolted_core::ValidationReport<NoteField> {
        let mut report = ::bolted_core::ValidationReport::new();
        if let Some(e) = self.title.required_error() {
            report.field_errors.push((NoteField::Title, e));
        }
        if let Some(e) = self.body.required_error() {
            report.field_errors.push((NoteField::Body, e));
        }
        report.rule_errors.extend(NoteRules::rules(self));
        report
    }
    fn resolve_keep_mine(&mut self, field: NoteField) {
        self.bolted_guard(|__d| match field {
            NoteField::Title => __d.title.resolve_keep_mine(),
            NoteField::Body => __d.body.resolve_keep_mine(),
        })
    }
    fn resolve_take_theirs(&mut self, field: NoteField) {
        self.bolted_guard(|__d| match field {
            NoteField::Title => __d.title.resolve_take_theirs(),
            NoteField::Body => __d.body.resolve_take_theirs(),
        })
    }
    fn commit(
        self,
    ) -> ::core::result::Result<Note, (Self, ::bolted_core::CommitError<NoteField>)> {
        if let Some(e) = ::bolted_core::commit_gates(&self) {
            return Err((self, e));
        }
        match (self.title.value().cloned(), self.body.value().cloned()) {
            (Some(title), Some(body)) => Ok(Note { title, body }),
            _ => {
                let report = <Self as ::bolted_core::Draft>::validate(&self);
                Err((self, ::bolted_core::CommitError::Validation(report)))
            }
        }
    }
}
impl ::bolted_core::StoreDraft for NoteDraft {
    fn from_canonical(base: Option<&Note>, base_version: u64) -> Self {
        match base {
            Some(__e) => {
                NoteDraft {
                    title: ::bolted_core::Field::from_base(__e.title.clone()),
                    body: ::bolted_core::Field::from_base(__e.body.clone()),
                    status: ::bolted_core::DraftStatus::Live,
                    base_version,
                }
            }
            None => {
                NoteDraft {
                    title: ::bolted_core::Field::new_unset(),
                    body: ::bolted_core::Field::new_unset(),
                    status: ::bolted_core::DraftStatus::Live,
                    base_version,
                }
            }
        }
    }
    fn rebase(&mut self, entity: &Note, version: u64) {
        if matches!(self.status, ::bolted_core::DraftStatus::Orphaned) {
            return;
        }
        self.bolted_guard(|__d| {
            __d.title.rebase(entity.title.clone());
            __d.body.rebase(entity.body.clone());
        });
        self.base_version = version;
    }
    fn orphan(&mut self) {
        self.status = ::bolted_core::DraftStatus::Orphaned;
    }
    fn is_based(&self) -> bool {
        self.title.base().is_some() || self.body.base().is_some()
    }
}
impl ::bolted_core::Stashable for NoteDraft {
    type Stash = NoteStash;
    fn stash(&self) -> NoteStash {
        NoteStash {
            title: self.title.stash(),
            body: self.body.stash(),
            base_version: self.base_version,
            orphaned: matches!(self.status, ::bolted_core::DraftStatus::Orphaned),
        }
    }
    fn from_stash(stash: &NoteStash) -> Self {
        NoteDraft {
            title: ::bolted_core::Field::from_stash(&stash.title),
            body: ::bolted_core::Field::from_stash(&stash.body),
            status: if stash.orphaned {
                ::bolted_core::DraftStatus::Orphaned
            } else {
                ::bolted_core::DraftStatus::Live
            },
            base_version: stash.base_version,
        }
    }
}
