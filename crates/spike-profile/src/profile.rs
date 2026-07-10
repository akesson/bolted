//! The hand-written "as-if-generated" profile feature: entity, field-id enum, draft, the tier-2
//! rule, and the `Draft` / `StoreDraft` impls. Every per-field line is written as plainly and
//! mechanically as the future `#[bolted::entity]` macro would emit it.

use crate::value_types::{
    Date, DateRange, DateRangeError, Email, EmailError, PersonName, PersonNameError, Username,
    UsernameError,
};
use bolted_core::{
    CheckState, CheckToken, CommitError, Constraint, Draft, DraftHandle, DraftStatus, ErrorData,
    Field, RuleViolation, SingleFlight, Store, StoreDraft, ValidationReport, Validity, Value,
};

/// The always-valid entity (canonical state). `#[bolted::entity]` would generate this alongside
/// the draft, the field-id enum, and the monomorphic setters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub username: Username,
    pub name: PersonName,
    pub email: Email,
    pub availability: DateRange,
}

/// Typed field identifiers. Rule errors pin to these; pinning a nonexistent field would be a
/// compile error (the enum has no such variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileField {
    Username,
    Name,
    Email,
    Availability,
}

impl ProfileField {
    /// Constraints a shell would export for this field. `Required` is prepended because every
    /// `Profile` field is non-optional (a field-level concern), followed by the value type's own
    /// intrinsic constraints. See the step-01 report for the value-vs-field `Required` question.
    pub fn constraints(self) -> Vec<Constraint> {
        let mut out = vec![Constraint::Required];
        let intrinsic: &'static [Constraint] = match self {
            ProfileField::Username => Username::constraints(),
            ProfileField::Name => PersonName::constraints(),
            ProfileField::Email => Email::constraints(),
            ProfileField::Availability => DateRange::constraints(),
        };
        out.extend_from_slice(intrinsic);
        out
    }
}

/// Convenience aliases for the prototype store specialised to this feature.
pub type ProfileStore = Store<ProfileDraft>;
pub type ProfileHandle = DraftHandle<ProfileDraft>;

/// The draft: one `Field<V>` per entity field, plus the async uniqueness check and lifecycle bits.
pub struct ProfileDraft {
    pub username: Field<Username>,
    pub name: Field<PersonName>,
    pub email: Field<Email>,
    pub availability: Field<DateRange>,
    username_check: SingleFlight<Result<(), ErrorData>>,
    status: DraftStatus,
    base_version: u64,
}

impl ProfileDraft {
    /// Run a mutation that may move the `username` value, resetting the async uniqueness check iff
    /// the value actually changed (ARCHITECTURE §2/§8, conformance C13 — a verdict endorses a
    /// *value*, so a changed value un-endorses it). Implemented once by value comparison rather
    /// than per call site, so no mutation path can silently skip it. Comparing `value()` (`None`
    /// for `Unset`/`Invalid`) gets every case right: edit-to-different / edit-to-invalid /
    /// rebase-adopt / take-theirs all move the value and reset; edit-to-same, keep-mine, and a
    /// conflict that preserves your value leave the verdict standing.
    fn with_username_guard<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let before = self.username.value().cloned();
        let out = f(self);
        if self.username.value() != before.as_ref() {
            self.username_check.reset();
        }
        out
    }

    /// Read the async uniqueness check's sub-state, so the FFI layer can project it into snapshots
    /// (step-02 finding 7). The field stays private; this is a read-only getter — `validate()` is
    /// unchanged (a `Pending`/`Done(Err)` check still blocks; `Idle`/`Done(Ok)` still pass).
    pub fn username_check_state(&self) -> &CheckState<Result<(), ErrorData>> {
        self.username_check.state()
    }

    // --- monomorphic setters (one per field; `try_set_availability` takes the grouped raw) ---

    pub fn try_set_username(&mut self, raw: String) -> Result<(), UsernameError> {
        self.with_username_guard(|s| s.username.try_set(raw))
    }

    pub fn try_set_name(&mut self, raw: String) -> Result<(), PersonNameError> {
        self.name.try_set(raw)
    }

    pub fn try_set_email(&mut self, raw: String) -> Result<(), EmailError> {
        self.email.try_set(raw)
    }

    pub fn try_set_availability(&mut self, start: Date, end: Date) -> Result<(), DateRangeError> {
        self.availability.try_set((start, end))
    }

    // --- async uniqueness check (single-flight, driven by the shell/test) ---

    pub fn begin_username_check(&mut self) -> CheckToken {
        self.username_check.begin()
    }

    pub fn complete_username_check(
        &mut self,
        token: CheckToken,
        verdict: Result<(), ErrorData>,
    ) -> bool {
        self.username_check.complete(token, verdict)
    }

    // --- conflict resolution, dispatched per field id ---

    pub fn resolve_keep_mine(&mut self, field: ProfileField) {
        self.with_username_guard(|s| match field {
            ProfileField::Username => s.username.resolve_keep_mine(),
            ProfileField::Name => s.name.resolve_keep_mine(),
            ProfileField::Email => s.email.resolve_keep_mine(),
            ProfileField::Availability => s.availability.resolve_keep_mine(),
        })
    }

    pub fn resolve_take_theirs(&mut self, field: ProfileField) {
        self.with_username_guard(|s| match field {
            ProfileField::Username => s.username.resolve_take_theirs(),
            ProfileField::Name => s.name.resolve_take_theirs(),
            ProfileField::Email => s.email.resolve_take_theirs(),
            ProfileField::Availability => s.availability.resolve_take_theirs(),
        })
    }

    /// Tier-2 rule `corporate_email`, pinned to `Email` (as `#[rule(pins(email))]` would emit): a
    /// `corp_`-prefixed username requires the `corp.example` email domain. Evaluated only over
    /// valid values — invalid/unset involved fields are already flagged by tier 1.
    fn corporate_email(&self) -> Result<(), RuleViolation<ProfileField>> {
        if let (Some(u), Some(em)) = (self.username.value(), self.email.value())
            && u.as_str().starts_with("corp_")
            && em.domain() != "corp.example"
        {
            return Err(RuleViolation {
                rule: "corporate_email",
                pins: vec![ProfileField::Email],
                error: ErrorData {
                    key: "corporate_email_domain",
                    params: vec![
                        ("expected", "corp.example".to_string()),
                        ("actual", em.domain().to_string()),
                    ],
                },
            });
        }
        Ok(())
    }
}

/// Tier-1 error for a field's current validity, if any. `Invalid` → its typed error mapped to
/// `ErrorData` (by `Field::invalid_error`, which the `Value::Error: Into<ErrorData>` bound makes
/// possible); `Unset` → `required`, because every `Profile` field is non-optional — a *field*-level
/// judgement the value type cannot make (ARCHITECTURE §8, step-01 D3/Q3).
fn tier1_error<V: Value>(field: &Field<V>) -> Option<ErrorData> {
    match field.validity() {
        Validity::Valid(_) => None,
        Validity::Invalid { .. } => field.invalid_error(),
        Validity::Unset => Some(ErrorData::new("required")),
    }
}

impl Draft for ProfileDraft {
    type Entity = Profile;
    type FieldId = ProfileField;

    fn status(&self) -> DraftStatus {
        self.status
    }

    fn base_version(&self) -> u64 {
        self.base_version
    }

    fn dirty_fields(&self) -> Vec<ProfileField> {
        let mut out = Vec::new();
        if self.username.is_dirty() {
            out.push(ProfileField::Username);
        }
        if self.name.is_dirty() {
            out.push(ProfileField::Name);
        }
        if self.email.is_dirty() {
            out.push(ProfileField::Email);
        }
        if self.availability.is_dirty() {
            out.push(ProfileField::Availability);
        }
        out
    }

    fn conflicts(&self) -> Vec<ProfileField> {
        let mut out = Vec::new();
        if self.username.is_conflicted() {
            out.push(ProfileField::Username);
        }
        if self.name.is_conflicted() {
            out.push(ProfileField::Name);
        }
        if self.email.is_conflicted() {
            out.push(ProfileField::Email);
        }
        if self.availability.is_conflicted() {
            out.push(ProfileField::Availability);
        }
        out
    }

    fn validate(&self) -> ValidationReport<ProfileField> {
        let mut report = ValidationReport::new();

        // Tier 1: per-field validity.
        if let Some(e) = tier1_error(&self.username) {
            report.field_errors.push((ProfileField::Username, e));
        }
        if let Some(e) = tier1_error(&self.name) {
            report.field_errors.push((ProfileField::Name, e));
        }
        if let Some(e) = tier1_error(&self.email) {
            report.field_errors.push((ProfileField::Email, e));
        }
        if let Some(e) = tier1_error(&self.availability) {
            report.field_errors.push((ProfileField::Availability, e));
        }

        // Tier 2: relational rules.
        if let Err(violation) = self.corporate_email() {
            report.rule_errors.push(violation);
        }

        // Async uniqueness, modelled as a rule pinned to Username. A pending or failed check blocks.
        // A never-run check blocks *only while the field is dirty*: a clean field still holds the
        // canonical value, which was verified when it was committed, so demanding a fresh check to
        // submit an unrelated edit would be theatre. A surviving `Done(Ok)` was necessarily computed
        // for the value now in the field (C13 resets the verdict on any value change), so "passed"
        // can be trusted. Together these close step-01's F1 and F2 by construction rather than by
        // shell convention (ARCHITECTURE §8; conformance C16).
        match self.username_check.state() {
            CheckState::Pending { .. } => report.rule_errors.push(RuleViolation {
                rule: "username_unique",
                pins: vec![ProfileField::Username],
                error: ErrorData::new("username_check_pending"),
            }),
            CheckState::Done { verdict: Err(e) } => report.rule_errors.push(RuleViolation {
                rule: "username_unique",
                pins: vec![ProfileField::Username],
                error: e.clone(),
            }),
            CheckState::Idle if self.username.is_dirty() => {
                report.rule_errors.push(RuleViolation {
                    rule: "username_unique",
                    pins: vec![ProfileField::Username],
                    error: ErrorData::new("username_check_required"),
                })
            }
            CheckState::Idle | CheckState::Done { verdict: Ok(()) } => {}
        }

        report
    }

    fn commit(self) -> Result<Profile, (Self, CommitError<ProfileField>)> {
        // The three gates of C07, each reported in its own shape. Before the freeze the last two
        // were re-encoded as synthetic rule violations so they could fit a `ValidationReport`
        // (step-01 F5); they are not validation tiers and no longer pretend to be.
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

        // `report.is_ok()` guarantees every field is `Valid`. The values are *cloned* rather than
        // moved out with `into_valid()`: dismembering `self` before the last fallible step would
        // leave nothing to hand back on the (unreachable) `_` arm, and `commit` promises the draft
        // back on every failure. Four small clones is the price of that promise.
        match (
            self.username.value().cloned(),
            self.name.value().cloned(),
            self.email.value().cloned(),
            self.availability.value().cloned(),
        ) {
            (Some(username), Some(name), Some(email), Some(availability)) => Ok(Profile {
                username,
                name,
                email,
                availability,
            }),
            // Unreachable: an ok report implies all four are `Valid`.
            _ => {
                let report = self.validate();
                Err((self, CommitError::Validation(report)))
            }
        }
    }
}

impl StoreDraft for ProfileDraft {
    fn from_canonical(base: Option<&Profile>, base_version: u64) -> Self {
        match base {
            // Every field clones, uniformly — which is only possible because value objects are not
            // `Copy` (ARCHITECTURE §8, step-01 F4). This is the shape `#[bolted::entity]` emits.
            Some(p) => ProfileDraft {
                username: Field::from_base(p.username.clone()),
                name: Field::from_base(p.name.clone()),
                email: Field::from_base(p.email.clone()),
                availability: Field::from_base(p.availability.clone()),
                username_check: SingleFlight::new(),
                status: DraftStatus::Live,
                base_version,
            },
            None => ProfileDraft {
                username: Field::new_unset(),
                name: Field::new_unset(),
                email: Field::new_unset(),
                availability: Field::new_unset(),
                username_check: SingleFlight::new(),
                status: DraftStatus::Live,
                base_version,
            },
        }
    }

    fn rebase(&mut self, entity: &Profile, version: u64) {
        if matches!(self.status, DraftStatus::Orphaned) {
            return; // orphan is terminal, and the draft is based on no canonical at all
        }
        // Guarded: a rebase that adopts or converges the username moves its value and resets the
        // check; one that conflicts (your value preserved) leaves the verdict standing (C13).
        self.with_username_guard(|s| {
            s.username.rebase(entity.username.clone());
            s.name.rebase(entity.name.clone());
            s.email.rebase(entity.email.clone());
            s.availability.rebase(entity.availability.clone());
        });
        // The draft is now based on this canonical. Before the freeze `base_version` was written
        // once at checkout and never again, so every draft snapshot carried a stamp that was stale
        // the moment a rebase landed — and the version-guarded reconcile it was shipped for could
        // never fire (step-05 structural finding; conformance C15).
        self.base_version = version;
    }

    fn orphan(&mut self) {
        self.status = DraftStatus::Orphaned;
    }
}
