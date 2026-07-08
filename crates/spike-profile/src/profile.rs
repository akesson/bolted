//! The hand-written "as-if-generated" profile feature: entity, field-id enum, draft, the tier-2
//! rule, and the `Draft` / `StoreDraft` impls. Every per-field line is written as plainly and
//! mechanically as the future `#[bolted::entity]` macro would emit it.

use crate::value_types::{
    Date, DateRange, DateRangeError, Email, EmailError, PersonName, PersonNameError, Username,
    UsernameError,
};
use bolted_core::{
    CheckState, CheckToken, Constraint, Draft, DraftHandle, DraftStatus, ErrorData, Field,
    RuleViolation, SingleFlight, Store, StoreDraft, SyncState, ValidationReport, Validity, Value,
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
    /// the value actually changed (ARCHITECTURE §2/§8, invariant I13 — a verdict endorses a
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

    pub fn base_version(&self) -> u64 {
        self.base_version
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
/// `ErrorData`; `Unset` → `required` (every `Profile` field is required). The `V::Error:
/// Into<ErrorData>` bound is what lets this be one generic helper instead of per-field code — the
/// step-01 report notes this as a candidate for the `Value` trait itself.
fn tier1_error<V>(field: &Field<V>) -> Option<ErrorData>
where
    V: Value,
    V::Error: Into<ErrorData>,
{
    match field.validity() {
        Validity::Valid(_) => None,
        Validity::Invalid { error, .. } => Some(error.clone().into()),
        Validity::Unset => Some(ErrorData::new("required")),
    }
}

impl Draft for ProfileDraft {
    type Entity = Profile;
    type FieldId = ProfileField;

    fn status(&self) -> DraftStatus {
        self.status
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
        if matches!(self.username.sync(), SyncState::Conflicted { .. }) {
            out.push(ProfileField::Username);
        }
        if matches!(self.name.sync(), SyncState::Conflicted { .. }) {
            out.push(ProfileField::Name);
        }
        if matches!(self.email.sync(), SyncState::Conflicted { .. }) {
            out.push(ProfileField::Email);
        }
        if matches!(self.availability.sync(), SyncState::Conflicted { .. }) {
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

        // Async uniqueness: a pending OR failed check blocks (modelled as a rule pinned to
        // Username). A never-run (Idle) or passed check does not block — see the step-01 report's
        // friction note on whether commit should require a completed successful check.
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
            CheckState::Idle | CheckState::Done { verdict: Ok(()) } => {}
        }

        report
    }

    fn commit(self) -> Result<Profile, ValidationReport<ProfileField>> {
        let mut report = self.validate();

        // commit additionally forbids unresolved conflicts and orphaned status (invariant I7);
        // these are not validation tiers, so they are injected as rule violations here.
        for field in self.conflicts() {
            report.rule_errors.push(RuleViolation {
                rule: "unresolved_conflict",
                pins: vec![field],
                error: ErrorData::new("field_conflicted"),
            });
        }
        if matches!(self.status, DraftStatus::Orphaned) {
            report.rule_errors.push(RuleViolation {
                rule: "orphaned",
                pins: Vec::new(),
                error: ErrorData::new("draft_orphaned"),
            });
        }

        if !report.is_ok() {
            return Err(report);
        }

        // `report.is_ok()` guarantees every field is `Valid`; move the values out.
        match (
            self.username.into_valid(),
            self.name.into_valid(),
            self.email.into_valid(),
            self.availability.into_valid(),
        ) {
            (Some(username), Some(name), Some(email), Some(availability)) => Ok(Profile {
                username,
                name,
                email,
                availability,
            }),
            // Unreachable: an ok report implies all four are `Valid`.
            _ => Err(report),
        }
    }
}

impl StoreDraft for ProfileDraft {
    fn from_canonical(base: Option<&Profile>, base_version: u64) -> Self {
        match base {
            Some(p) => ProfileDraft {
                username: Field::from_base(p.username.clone()),
                name: Field::from_base(p.name.clone()),
                email: Field::from_base(p.email.clone()),
                // `DateRange` is `Copy`, so no clone (clippy::clone_on_copy). The uniform
                // per-field `.clone()` a generator would emit collides with this lint for Copy
                // value objects — see the step-01 report friction log.
                availability: Field::from_base(p.availability),
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

    fn rebase(&mut self, entity: &Profile) {
        if matches!(self.status, DraftStatus::Orphaned) {
            return;
        }
        // Guarded: a rebase that adopts or converges the username moves its value and resets the
        // check; one that conflicts (your value preserved) leaves the verdict standing (I13).
        self.with_username_guard(|s| {
            s.username.rebase(entity.username.clone());
            s.name.rebase(entity.name.clone());
            s.email.rebase(entity.email.clone());
            s.availability.rebase(entity.availability); // Copy — see note in `from_canonical`
        });
    }

    fn orphan(&mut self) {
        self.status = DraftStatus::Orphaned;
    }
}
