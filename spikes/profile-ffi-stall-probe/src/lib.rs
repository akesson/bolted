//! Step-02 probe: the step-01 profile feature exported over BoltFFI (Apple).
//!
//! This wrapper is hand-written **as-if-generated** — the first draft of what `bolted-ffi`
//! would emit. Field/draft semantics are entirely `bolted-core`/`spike-profile`; what lives
//! here is (a) `#[data]`/`#[error]` mirrors of the contract types, (b) the thread-safe
//! re-hosting of the store *plumbing* (`bolted-core`'s prototype `Store` is deliberately
//! `Rc<RefCell>`-based and `!Send`, while BoltFFI classes must be `Send + Sync`), and
//! (c) the observation-contract probes (streams, burst, window rows).
//!
//! The `Mutex` used here is spike plumbing, NOT the core's threading contract — that stays
//! an ARCHITECTURE §9 / step-06 question; see the step-02 report.

use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use boltffi::{EventSubscription, data, error, export, ffi_stream};
use bolted_core::{
    CheckToken, CommitError, Constraint, Draft, DraftStatus, ErrorData, Field, StoreDraft,
    SyncState, ValidationReport, Validity, Value,
};
use spike_profile::{
    Date, DateRange, DateRangeError, Email, EmailError, PersonName, PersonNameError, Profile,
    ProfileDraft, ProfileField, Username, UsernameError,
};

/// Poison-safe lock (no `unwrap` in library code; a poisoned probe state is still readable).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// =============================================================================================
// #[data] mirrors of the contract types
// =============================================================================================

#[data]
#[derive(Clone, Copy, PartialEq)]
pub struct FfiDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl From<Date> for FfiDate {
    fn from(d: Date) -> Self {
        FfiDate {
            year: d.year,
            month: d.month,
            day: d.day,
        }
    }
}

impl From<FfiDate> for Date {
    fn from(d: FfiDate) -> Self {
        Date::new(d.year, d.month, d.day)
    }
}

/// Read-only canonical snapshot (the facet `observe` payload).
#[data]
#[derive(Clone, PartialEq)]
pub struct ProfileSnapshot {
    pub version: u64,
    pub exists: bool,
    pub username: String,
    pub name: String,
    pub email: String,
    pub start: FfiDate,
    pub end: FfiDate,
}

#[data]
#[derive(Clone, Copy, PartialEq)]
pub enum FfiProfileField {
    Username,
    Name,
    Email,
    Availability,
}

#[data]
#[derive(Clone, Copy, PartialEq)]
pub enum FfiDraftStatus {
    Live,
    Orphaned,
    /// The draft was consumed by a successful submit; the handle is dead.
    Consumed,
}

#[data]
#[derive(Clone, PartialEq)]
pub struct FfiParam {
    pub name: String,
    pub value: String,
}

/// Errors are key + params data, never strings (ARCHITECTURE §2).
#[data]
#[derive(Clone, PartialEq)]
pub struct FfiErrorData {
    pub key: String,
    pub params: Vec<FfiParam>,
}

#[data]
#[derive(Clone, PartialEq)]
pub struct FfiFieldError {
    pub field: FfiProfileField,
    pub error: FfiErrorData,
}

#[data]
#[derive(Clone, PartialEq)]
pub struct FfiRuleError {
    pub rule: String,
    pub pins: Vec<FfiProfileField>,
    pub error: FfiErrorData,
}

#[data]
#[derive(Clone, PartialEq)]
pub struct FfiValidationReport {
    pub ok: bool,
    pub field_errors: Vec<FfiFieldError>,
    pub rule_errors: Vec<FfiRuleError>,
}

/// Constraint metadata export — what lets shells derive affordances without literals.
#[data]
#[derive(Clone, PartialEq)]
pub enum FfiConstraint {
    Required,
    LenChars { min: u32, max: u32 },
    Custom { name: String },
}

/// One field of the draft snapshot: raw text + validity/dirty/conflict flags.
#[data]
#[derive(Clone, PartialEq)]
pub struct FfiFieldView {
    pub text: String,
    pub valid: bool,
    pub error_key: String,
    pub dirty: bool,
    pub conflicted: bool,
    pub theirs: String,
}

/// The draft's own snapshot (a draft is a mini facet — ARCHITECTURE §4).
#[data]
#[derive(Clone, PartialEq)]
pub struct FfiDraftSnapshot {
    pub status: FfiDraftStatus,
    pub base_version: u64,
    pub username: FfiFieldView,
    pub name: FfiFieldView,
    pub email: FfiFieldView,
    pub availability: FfiFieldView,
}

/// Window-scale payload probe row (ARCHITECTURE §1 windowed observation).
#[data]
#[derive(Clone, PartialEq)]
pub struct FfiRow {
    pub id: u64,
    pub title: String,
    pub subtitle: String,
}

// =============================================================================================
// #[error] mirrors — typed error enums with payload variants (probe C1b)
// =============================================================================================
//
// Each mirror carries the value type's variants plus `DraftClosed`, the boundary-layer error
// for calls on a consumed handle. That extra variant is wrapper reality, not core semantics —
// logged as friction (the real bolted-ffi needs a uniform closed-handle channel).

#[error]
#[derive(Clone, PartialEq)]
pub enum FfiUsernameError {
    TooShort { min: u32, actual: u32 },
    TooLong { max: u32, actual: u32 },
    InvalidChars,
    DraftClosed,
}

#[error]
#[derive(Clone, PartialEq)]
pub enum FfiPersonNameError {
    TooShort { min: u32, actual: u32 },
    TooLong { max: u32, actual: u32 },
    DraftClosed,
}

#[error]
#[derive(Clone, PartialEq)]
pub enum FfiEmailError {
    Invalid,
    DraftClosed,
}

#[error]
#[derive(Clone, PartialEq)]
pub enum FfiDateRangeError {
    StartAfterEnd { start: FfiDate, end: FfiDate },
    DraftClosed,
}

#[error]
#[derive(Clone, PartialEq)]
pub enum FfiApplyError {
    Invalid { field: String, key: String },
}

#[error]
#[derive(Clone, PartialEq)]
pub enum FfiSubmitError {
    Validation { report: FfiValidationReport },
    Conflicted { fields: Vec<FfiProfileField> },
    Orphaned,
    DraftClosed,
}

// =============================================================================================
// Conversions core → FFI mirrors
// =============================================================================================

fn field_to_ffi(f: ProfileField) -> FfiProfileField {
    match f {
        ProfileField::Username => FfiProfileField::Username,
        ProfileField::Name => FfiProfileField::Name,
        ProfileField::Email => FfiProfileField::Email,
        ProfileField::Availability => FfiProfileField::Availability,
    }
}

fn field_from_ffi(f: FfiProfileField) -> ProfileField {
    match f {
        FfiProfileField::Username => ProfileField::Username,
        FfiProfileField::Name => ProfileField::Name,
        FfiProfileField::Email => ProfileField::Email,
        FfiProfileField::Availability => ProfileField::Availability,
    }
}

fn error_data_to_ffi(e: &ErrorData) -> FfiErrorData {
    FfiErrorData {
        key: e.key.to_string(),
        params: e
            .params
            .iter()
            .map(|(name, value)| FfiParam {
                name: (*name).to_string(),
                value: value.clone(),
            })
            .collect(),
    }
}

fn constraint_to_ffi(c: &Constraint) -> FfiConstraint {
    match c {
        Constraint::Required => FfiConstraint::Required,
        Constraint::LenChars { min, max } => FfiConstraint::LenChars {
            min: *min,
            max: *max,
        },
        Constraint::Custom(name) => FfiConstraint::Custom {
            name: (*name).to_string(),
        },
    }
}

fn report_to_ffi(r: &ValidationReport<ProfileField>) -> FfiValidationReport {
    FfiValidationReport {
        ok: r.is_ok(),
        field_errors: r
            .field_errors
            .iter()
            .map(|(f, e)| FfiFieldError {
                field: field_to_ffi(*f),
                error: error_data_to_ffi(e),
            })
            .collect(),
        rule_errors: r
            .rule_errors
            .iter()
            .map(|v| FfiRuleError {
                rule: v.rule.to_string(),
                pins: v.pins.iter().map(|f| field_to_ffi(*f)).collect(),
                error: error_data_to_ffi(&v.error),
            })
            .collect(),
    }
}

fn username_error_to_ffi(e: UsernameError) -> FfiUsernameError {
    match e {
        UsernameError::TooShort { min, actual } => FfiUsernameError::TooShort { min, actual },
        UsernameError::TooLong { max, actual } => FfiUsernameError::TooLong { max, actual },
        UsernameError::InvalidChars => FfiUsernameError::InvalidChars,
    }
}

fn person_name_error_to_ffi(e: PersonNameError) -> FfiPersonNameError {
    match e {
        PersonNameError::TooShort { min, actual } => FfiPersonNameError::TooShort { min, actual },
        PersonNameError::TooLong { max, actual } => FfiPersonNameError::TooLong { max, actual },
    }
}

fn email_error_to_ffi(e: EmailError) -> FfiEmailError {
    match e {
        EmailError::Invalid => FfiEmailError::Invalid,
    }
}

fn date_range_error_to_ffi(e: DateRangeError) -> FfiDateRangeError {
    match e {
        DateRangeError::StartAfterEnd { start, end } => FfiDateRangeError::StartAfterEnd {
            start: start.into(),
            end: end.into(),
        },
    }
}

// =============================================================================================
// Field views (draft snapshot construction)
// =============================================================================================

/// View of a `Field<V>` whose raw is `String`. Text policy: the *attempted* raw survives
/// (`Invalid { raw }`), matching the echo rule's "raw is authoritative" side.
fn view_text_field<V>(field: &Field<V>) -> FfiFieldView
where
    V: Value<Raw = String>,
    V::Error: Into<ErrorData>,
{
    let (text, valid, error_key) = match field.validity() {
        Validity::Valid(v) => (v.clone().into_raw(), true, String::new()),
        Validity::Invalid { raw, error } => {
            let data: ErrorData = error.clone().into();
            (raw.clone(), false, data.key.to_string())
        }
        Validity::Unset => (String::new(), false, "unset".to_string()),
    };
    let (conflicted, theirs) = match field.sync() {
        SyncState::InSync => (false, String::new()),
        SyncState::Conflicted { theirs, .. } => (true, theirs.clone().into_raw()),
    };
    FfiFieldView {
        text,
        valid,
        error_key,
        dirty: field.is_dirty(),
        conflicted,
        theirs,
    }
}

fn fmt_range(raw: &(Date, Date)) -> String {
    let ((sy, sm, sd), (ey, em, ed)) = (
        (raw.0.year, raw.0.month, raw.0.day),
        (raw.1.year, raw.1.month, raw.1.day),
    );
    format!("{sy:04}-{sm:02}-{sd:02}..{ey:04}-{em:02}-{ed:02}")
}

fn view_range_field(field: &Field<DateRange>) -> FfiFieldView {
    let (text, valid, error_key) = match field.validity() {
        Validity::Valid(v) => (fmt_range(&(v.start(), v.end())), true, String::new()),
        Validity::Invalid { raw, error } => {
            let data: ErrorData = error.clone().into();
            (fmt_range(raw), false, data.key.to_string())
        }
        Validity::Unset => (String::new(), false, "unset".to_string()),
    };
    let (conflicted, theirs) = match field.sync() {
        SyncState::InSync => (false, String::new()),
        SyncState::Conflicted { theirs, .. } => (true, fmt_range(&(theirs.start(), theirs.end()))),
    };
    FfiFieldView {
        text,
        valid,
        error_key,
        dirty: field.is_dirty(),
        conflicted,
        theirs,
    }
}

fn empty_field_view() -> FfiFieldView {
    FfiFieldView {
        text: String::new(),
        valid: false,
        error_key: String::new(),
        dirty: false,
        conflicted: false,
        theirs: String::new(),
    }
}

// =============================================================================================
// Draft slot — the thread-safe re-hosting of Store's per-draft plumbing
// =============================================================================================

struct DraftSlot {
    /// `None` after a successful submit consumed the draft (status `Consumed`).
    draft: Option<ProfileDraft>,
    /// `CheckToken` is opaque (private seq, no constructor) so it cannot cross the FFI as a
    /// value; the wrapper holds tokens keyed by its own id. Friction — see the report.
    tokens: Vec<(u64, CheckToken)>,
    next_token: u64,
    last_snapshot: Option<FfiDraftSnapshot>,
    events: Arc<EventSubscription<FfiDraftSnapshot>>,
}

fn slot_snapshot(slot: &DraftSlot) -> FfiDraftSnapshot {
    match &slot.draft {
        Some(d) => FfiDraftSnapshot {
            status: match d.status() {
                DraftStatus::Live => FfiDraftStatus::Live,
                DraftStatus::Orphaned => FfiDraftStatus::Orphaned,
            },
            base_version: d.base_version(),
            username: view_text_field(&d.username),
            name: view_text_field(&d.name),
            email: view_text_field(&d.email),
            availability: view_range_field(&d.availability),
        },
        None => FfiDraftSnapshot {
            status: FfiDraftStatus::Consumed,
            base_version: 0,
            username: empty_field_view(),
            name: empty_field_view(),
            email: empty_field_view(),
            availability: empty_field_view(),
        },
    }
}

/// Recompute-and-compare snapshot emission — value-diffed, like the reduce loop (§5).
fn publish_slot(slot: &mut DraftSlot) {
    let snap = slot_snapshot(slot);
    if slot.last_snapshot.as_ref() != Some(&snap) {
        slot.last_snapshot = Some(snap.clone());
        slot.events.push_event(snap);
    }
}

// =============================================================================================
// ProfileFacet — the exported facet class (probe C1a)
// =============================================================================================
//
// `new_without_default` is allowed on the exported classes: BoltFFI's constructor convention
// wants `new()`, and a `Default` impl would be dead code on an FFI class.

struct FacetState {
    canonical: Option<Profile>,
    version: u64,
    /// Live drafts as weak refs with stable ids — the handle is the sole strong owner,
    /// mirroring `Store`'s design (`Weak<RefCell>` there, `Weak<Mutex>` here).
    live: Vec<(u64, Weak<Mutex<DraftSlot>>)>,
    next_draft_id: u64,
    last_snapshot: Option<ProfileSnapshot>,
}

pub struct ProfileFacet {
    state: Mutex<FacetState>,
    /// Default-capacity (256) snapshot stream — burst probe C2a(i).
    snapshots: Arc<EventSubscription<ProfileSnapshot>>,
    /// Capacity-1 snapshot stream — burst probe C2a(ii), the naive `Latest` candidate.
    snapshots_latest: Arc<EventSubscription<ProfileSnapshot>>,
    /// Capacity-1 wake stream (version numbers; drops harmless) — wake-and-read probe C2b.
    wakes: Arc<EventSubscription<u64>>,
    /// Callback-mode stream — delivery-thread probe C2d.
    wake_callbacks: Arc<EventSubscription<u64>>,
    /// Batch-mode (pull) stream — the candidate reliable path if push modes misbehave.
    snapshots_batch: Arc<EventSubscription<ProfileSnapshot>>,
}

fn facet_snapshot(state: &FacetState) -> ProfileSnapshot {
    match &state.canonical {
        Some(p) => ProfileSnapshot {
            version: state.version,
            exists: true,
            username: p.username.as_str().to_string(),
            name: p.name.as_str().to_string(),
            email: p.email.as_str().to_string(),
            start: p.availability.start().into(),
            end: p.availability.end().into(),
        },
        None => ProfileSnapshot {
            version: state.version,
            exists: false,
            username: String::new(),
            name: String::new(),
            email: String::new(),
            start: FfiDate {
                year: 0,
                month: 0,
                day: 0,
            },
            end: FfiDate {
                year: 0,
                month: 0,
                day: 0,
            },
        },
    }
}

impl ProfileFacet {
    fn push_all(&self, snap: ProfileSnapshot) {
        self.snapshots.push_event(snap.clone());
        self.snapshots_latest.push_event(snap.clone());
        self.snapshots_batch.push_event(snap.clone());
        self.wakes.push_event(snap.version);
        self.wake_callbacks.push_event(snap.version);
    }

    fn publish(&self, state: &mut FacetState) {
        let snap = facet_snapshot(state);
        if state.last_snapshot.as_ref() != Some(&snap) {
            state.last_snapshot = Some(snap.clone());
            self.push_all(snap);
        }
    }

    fn prune(state: &mut FacetState) {
        state.live.retain(|(_, w)| w.strong_count() > 0);
    }
}

#[export]
impl ProfileFacet {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        ProfileFacet {
            state: Mutex::new(FacetState {
                canonical: None,
                version: 0,
                live: Vec::new(),
                next_draft_id: 1,
                last_snapshot: None,
            }),
            snapshots: Arc::new(EventSubscription::new(256)),
            snapshots_latest: Arc::new(EventSubscription::new(1)),
            wakes: Arc::new(EventSubscription::new(1)),
            wake_callbacks: Arc::new(EventSubscription::new(1024)),
            snapshots_batch: Arc::new(EventSubscription::new(256)),
        }
    }

    pub fn version(&self) -> u64 {
        lock(&self.state).version
    }

    /// Current truth on demand — the read half of the wake-and-read `Latest` encoding (C2b).
    pub fn snapshot(&self) -> ProfileSnapshot {
        facet_snapshot(&lock(&self.state))
    }

    /// A canonical change arrived (server push, another device, …): validate the pieces,
    /// bump the version, live-rebase every draft, publish.
    pub fn apply_canonical(
        &self,
        username: String,
        name: String,
        email: String,
        start: FfiDate,
        end: FfiDate,
    ) -> Result<(), FfiApplyError> {
        let invalid = |field: &str, e: ErrorData| FfiApplyError::Invalid {
            field: field.to_string(),
            key: e.key.to_string(),
        };
        let profile = Profile {
            username: Username::try_new(username)
                .map_err(|e| invalid("username", e.into()))?,
            name: PersonName::try_new(name).map_err(|e| invalid("name", e.into()))?,
            email: Email::try_new(email).map_err(|e| invalid("email", e.into()))?,
            availability: DateRange::try_new((start.into(), end.into()))
                .map_err(|e| invalid("availability", e.into()))?,
        };

        let mut state = lock(&self.state);
        state.version += 1;
        let version = state.version;
        for (_, weak) in &state.live {
            if let Some(slot) = weak.upgrade() {
                let mut slot = lock(&slot);
                if let Some(d) = slot.draft.as_mut() {
                    d.rebase(&profile, version);
                }
                publish_slot(&mut slot);
            }
        }
        state.canonical = Some(profile);
        Self::prune(&mut state);
        self.publish(&mut state);
        Ok(())
    }

    /// Canonical deleted: orphan every live draft (invariant I11's boundary crossing).
    pub fn delete_canonical(&self) {
        let mut state = lock(&self.state);
        state.version += 1;
        for (_, weak) in &state.live {
            if let Some(slot) = weak.upgrade() {
                let mut slot = lock(&slot);
                if let Some(d) = slot.draft.as_mut() {
                    d.orphan();
                }
                publish_slot(&mut slot);
            }
        }
        state.canonical = None;
        Self::prune(&mut state);
        self.publish(&mut state);
    }

    /// Check out a draft — a method returning another exported class (probe C1a).
    /// Existing-entity checkouts register for live rebase; create-flow drafts do not (I12).
    pub fn checkout(&self) -> ProfileDraftFfi {
        let mut state = lock(&self.state);
        let draft = ProfileDraft::from_canonical(state.canonical.as_ref(), state.version);
        let id = state.next_draft_id;
        state.next_draft_id += 1;
        let slot = Arc::new(Mutex::new(DraftSlot {
            draft: Some(draft),
            tokens: Vec::new(),
            next_token: 1,
            last_snapshot: None,
            events: Arc::new(EventSubscription::new(64)),
        }));
        if state.canonical.is_some() {
            state.live.push((id, Arc::downgrade(&slot)));
        }
        ProfileDraftFfi { id, slot }
    }

    /// Submit a draft — an exported class as a *parameter* (probe C1a). Consumes the draft
    /// only on success (its status becomes `Consumed`); refusals leave it editable, matching
    /// the post-step-01 decision (ARCHITECTURE §8).
    pub fn submit(&self, draft: &ProfileDraftFfi) -> Result<(), FfiSubmitError> {
        let entity = {
            let mut slot = lock(&draft.slot);
            {
                let d = slot.draft.as_ref().ok_or(FfiSubmitError::DraftClosed)?;
                match d.status() {
                    DraftStatus::Orphaned => return Err(FfiSubmitError::Orphaned),
                    DraftStatus::Live => {}
                }
                let conflicts = d.conflicts();
                if !conflicts.is_empty() {
                    return Err(FfiSubmitError::Conflicted {
                        fields: conflicts.into_iter().map(field_to_ffi).collect(),
                    });
                }
                let report = d.validate();
                if !report.is_ok() {
                    return Err(FfiSubmitError::Validation {
                        report: report_to_ffi(&report),
                    });
                }
            }
            // Checks passed with the lock held — now consume. `commit` re-validates by
            // design (tier 3 floor); a refused commit hands the draft back, so the slot
            // keeps it editable instead of losing it.
            let d = slot.draft.take().ok_or(FfiSubmitError::DraftClosed)?;
            let entity = match d.commit() {
                Ok(e) => e,
                Err((d, err)) => {
                    slot.draft = Some(d);
                    return Err(match err {
                        CommitError::Validation(report) => FfiSubmitError::Validation {
                            report: report_to_ffi(&report),
                        },
                        CommitError::Conflicted { fields } => FfiSubmitError::Conflicted {
                            fields: fields.into_iter().map(field_to_ffi).collect(),
                        },
                        CommitError::Orphaned => FfiSubmitError::Orphaned,
                    });
                }
            };
            publish_slot(&mut slot);
            entity
        };

        let mut state = lock(&self.state);
        state.version += 1;
        let version = state.version;
        for (_, weak) in &state.live {
            if let Some(slot) = weak.upgrade() {
                let mut slot = lock(&slot);
                if let Some(d) = slot.draft.as_mut() {
                    d.rebase(&entity, version);
                }
                publish_slot(&mut slot);
            }
        }
        state.canonical = Some(entity);
        Self::prune(&mut state);
        self.publish(&mut state);
        Ok(())
    }

    /// Constraint metadata for a field — the single source of truth shells derive
    /// affordances from (no constraint literals in shell code).
    pub fn constraints_for(&self, field: FfiProfileField) -> Vec<FfiConstraint> {
        field_from_ffi(field)
            .constraints()
            .iter()
            .map(constraint_to_ffi)
            .collect()
    }

    /// Window-scale payload probe (C2c): 50-ish rows with strings out of a synthetic 10k
    /// collection. Measured from Swift at scroll-refetch frequency.
    pub fn window_rows(&self, offset: u32, len: u32) -> Vec<FfiRow> {
        const TOTAL: u32 = 10_000;
        let end = offset.saturating_add(len).min(TOTAL);
        (offset.min(TOTAL)..end)
            .map(|i| FfiRow {
                id: u64::from(i),
                title: format!("Row {i} — item title"),
                subtitle: format!("Subtitle for row {i}: a plausibly sized secondary line"),
            })
            .collect()
    }

    /// Burst probe (C2a): publish `count` version-stamped snapshots with no consumer delay.
    /// Deliberately synthetic — the question is stream behavior, not state semantics.
    pub fn emit_burst(&self, count: u32) {
        for i in 1..=u64::from(count) {
            let snap = ProfileSnapshot {
                version: i,
                exists: false,
                username: String::new(),
                name: String::new(),
                email: String::new(),
                start: FfiDate {
                    year: 0,
                    month: 0,
                    day: 0,
                },
                end: FfiDate {
                    year: 0,
                    month: 0,
                    day: 0,
                },
            };
            self.push_all(snap);
        }
    }

    pub fn noop(&self) {}

    #[ffi_stream(item = ProfileSnapshot)]
    pub fn snapshots(&self) -> Arc<EventSubscription<ProfileSnapshot>> {
        Arc::clone(&self.snapshots)
    }

    #[ffi_stream(item = ProfileSnapshot)]
    pub fn snapshots_latest(&self) -> Arc<EventSubscription<ProfileSnapshot>> {
        Arc::clone(&self.snapshots_latest)
    }

    #[ffi_stream(item = u64)]
    pub fn wakes(&self) -> Arc<EventSubscription<u64>> {
        Arc::clone(&self.wakes)
    }

    #[ffi_stream(item = u64, mode = "callback")]
    pub fn wake_callbacks(&self) -> Arc<EventSubscription<u64>> {
        Arc::clone(&self.wake_callbacks)
    }

    #[ffi_stream(item = ProfileSnapshot, mode = "batch")]
    pub fn snapshots_batch(&self) -> Arc<EventSubscription<ProfileSnapshot>> {
        Arc::clone(&self.snapshots_batch)
    }
}

// =============================================================================================
// ProfileDraftFfi — the draft handle class (probe C1a)
// =============================================================================================

pub struct ProfileDraftFfi {
    id: u64,
    slot: Arc<Mutex<DraftSlot>>,
}

/// Run `f` on the live draft, publish the draft snapshot, map a closed handle to `closed`.
fn with_draft<R, E>(
    slot: &Mutex<DraftSlot>,
    closed: E,
    f: impl FnOnce(&mut ProfileDraft) -> Result<R, E>,
) -> Result<R, E> {
    let mut slot = lock(slot);
    let out = match slot.draft.as_mut() {
        Some(d) => f(d),
        None => return Err(closed),
    };
    publish_slot(&mut slot);
    out
}

#[export]
impl ProfileDraftFfi {
    /// Stable logical identity (checkout sequence) — the replay precondition
    /// (ARCHITECTURE §9), never pointer identity.
    pub fn draft_id(&self) -> u64 {
        self.id
    }

    pub fn status(&self) -> FfiDraftStatus {
        match &lock(&self.slot).draft {
            Some(d) => match d.status() {
                DraftStatus::Live => FfiDraftStatus::Live,
                DraftStatus::Orphaned => FfiDraftStatus::Orphaned,
            },
            None => FfiDraftStatus::Consumed,
        }
    }

    pub fn base_version(&self) -> u64 {
        lock(&self.slot)
            .draft
            .as_ref()
            .map(ProfileDraft::base_version)
            .unwrap_or(0)
    }

    pub fn snapshot(&self) -> FfiDraftSnapshot {
        slot_snapshot(&lock(&self.slot))
    }

    // --- monomorphic setters: Result + typed payload-carrying error enums (probe C1b) ---

    pub fn try_set_username(&self, raw: String) -> Result<(), FfiUsernameError> {
        with_draft(&self.slot, FfiUsernameError::DraftClosed, |d| {
            d.try_set_username(raw).map_err(username_error_to_ffi)
        })
    }

    pub fn try_set_name(&self, raw: String) -> Result<(), FfiPersonNameError> {
        with_draft(&self.slot, FfiPersonNameError::DraftClosed, |d| {
            d.try_set_name(raw).map_err(person_name_error_to_ffi)
        })
    }

    pub fn try_set_email(&self, raw: String) -> Result<(), FfiEmailError> {
        with_draft(&self.slot, FfiEmailError::DraftClosed, |d| {
            d.try_set_email(raw).map_err(email_error_to_ffi)
        })
    }

    pub fn try_set_availability(
        &self,
        start: FfiDate,
        end: FfiDate,
    ) -> Result<(), FfiDateRangeError> {
        with_draft(&self.slot, FfiDateRangeError::DraftClosed, |d| {
            d.try_set_availability(start.into(), end.into())
                .map_err(date_range_error_to_ffi)
        })
    }

    // --- queries ---

    pub fn dirty_fields(&self) -> Vec<FfiProfileField> {
        lock(&self.slot)
            .draft
            .as_ref()
            .map(|d| d.dirty_fields().into_iter().map(field_to_ffi).collect())
            .unwrap_or_default()
    }

    pub fn conflicts(&self) -> Vec<FfiProfileField> {
        lock(&self.slot)
            .draft
            .as_ref()
            .map(|d| d.conflicts().into_iter().map(field_to_ffi).collect())
            .unwrap_or_default()
    }

    pub fn validate(&self) -> FfiValidationReport {
        match lock(&self.slot).draft.as_ref() {
            Some(d) => report_to_ffi(&d.validate()),
            None => FfiValidationReport {
                ok: false,
                field_errors: Vec::new(),
                rule_errors: vec![FfiRuleError {
                    rule: "draft_closed".to_string(),
                    pins: Vec::new(),
                    error: FfiErrorData {
                        key: "draft_closed".to_string(),
                        params: Vec::new(),
                    },
                }],
            },
        }
    }

    // --- conflict resolution ---

    pub fn resolve_keep_mine(&self, field: FfiProfileField) {
        let mut slot = lock(&self.slot);
        if let Some(d) = slot.draft.as_mut() {
            d.resolve_keep_mine(field_from_ffi(field));
        }
        publish_slot(&mut slot);
    }

    pub fn resolve_take_theirs(&self, field: FfiProfileField) {
        let mut slot = lock(&self.slot);
        if let Some(d) = slot.draft.as_mut() {
            d.resolve_take_theirs(field_from_ffi(field));
        }
        publish_slot(&mut slot);
    }

    // --- async uniqueness check (single-flight; the shell drives, the core orders) ---
    //
    // `CheckToken` cannot cross the FFI (opaque, no constructor), so the wrapper keys held
    // tokens by its own u64 ids. Returns 0 on a closed handle (ids start at 1).

    pub fn begin_username_check(&self) -> u64 {
        let mut slot = lock(&self.slot);
        let Some(d) = slot.draft.as_mut() else {
            return 0;
        };
        let token = d.begin_username_check();
        let id = slot.next_token;
        slot.next_token += 1;
        slot.tokens.push((id, token));
        publish_slot(&mut slot);
        id
    }

    /// Complete a check by wrapper token id. `unique = false` reports the username taken.
    /// Returns `false` for stale/unknown tokens (invariant I10 crossing the boundary).
    pub fn complete_username_check(&self, token: u64, unique: bool) -> bool {
        let mut slot = lock(&self.slot);
        let Some(pos) = slot.tokens.iter().position(|(id, _)| *id == token) else {
            return false;
        };
        let (_, core_token) = slot.tokens.remove(pos);
        let Some(d) = slot.draft.as_mut() else {
            return false;
        };
        let verdict = if unique {
            Ok(())
        } else {
            Err(ErrorData::new("username_taken"))
        };
        let accepted = d.complete_username_check(core_token, verdict);
        publish_slot(&mut slot);
        accepted
    }

    #[ffi_stream(item = FfiDraftSnapshot)]
    pub fn snapshots(&self) -> Arc<EventSubscription<FfiDraftSnapshot>> {
        Arc::clone(&lock(&self.slot).events)
    }
}

// =============================================================================================
// Probe C1d — #[export(single_threaded)] with a deliberately !Send interior
// =============================================================================================

pub struct SingleThreadedProbe {
    counter: Rc<std::cell::RefCell<u64>>,
}

#[export(single_threaded)]
impl SingleThreadedProbe {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        SingleThreadedProbe {
            counter: Rc::new(std::cell::RefCell::new(0)),
        }
    }

    pub fn increment(&self) -> u64 {
        let mut c = self.counter.borrow_mut();
        *c += 1;
        *c
    }
}
