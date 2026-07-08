//! `spike-profile-ffi` — the hand-written, "as-if-generated" BoltFFI wrapper around the
//! `spike-profile` feature (step 02, the BoltFFI due-diligence probe). The ONLY crate importing
//! `boltffi`; `bolted-core` still never sees it.
//!
//! NOTE: unlike `bolted-core`/`spike-profile`, this crate does NOT `#![forbid(unsafe_code)]` —
//! `#[export]` expands to `extern "C"` shims containing `unsafe`, so the forbid would reject
//! generated code. FINDING: the FFI boundary is exactly where the no-unsafe discipline stops.
//!
//! ## The re-owned store (Deliverable 1b)
//!
//! Step-01's `Store`/`DraftHandle` are `Rc<RefCell<…>>`/`Weak` — NOT `Send` — so a `Mutex` around
//! them will not compile, and BoltFFI classes are shared across foreign threads (`&self` only,
//! interior mutability required). But `spike_profile::ProfileDraft` is plain owned data and IS
//! `Send`. So the wrapper **re-owns the store loop**: it holds `HashMap<DraftId, ProfileDraft>` +
//! `canonical` + `version` behind ONE `Mutex` and re-implements checkout registration,
//! `apply_canonical` fan-out (rebase every live draft), and `submit` directly against
//! `bolted-core`'s `Field`/`Draft` — bypassing `Store` entirely. How much of `store.rs` had to be
//! re-owned is recorded in the step-02 report (evidence for ARCHITECTURE §9's store-concurrency
//! question).
//!
//! Wrapper invariant, enforced by discipline (the reentrancy tests punish violations):
//! **never emit a stream event or invoke a foreign callback while holding the `Mutex`.** Every
//! mutation locks, mutates, builds the snapshot, DROPS the lock, then pushes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use boltffi::*;

use bolted_core::report::ErrorData as CoreErrorData;
use bolted_core::{
    CheckState, Constraint, Draft, DraftStatus, Field, StoreDraft, SyncState, ValidationReport,
    Validity, Value,
};
use spike_profile::{DateRange, Email, PersonName, Profile, ProfileDraft, ProfileField, Username};

mod dto;
use dto::*;

/// Walking-skeleton probe (milestone 1). Kept so `SkeletonTests` stays green.
#[export]
pub fn ping(input: String) -> String {
    format!("pong: {input}")
}

// =================================================================================================
// Capability: the async username-uniqueness check, implemented on the FOREIGN side (Deliverable 1d)
// =================================================================================================

/// A BoltFFI callback trait: Swift implements it and hands it to a draft via
/// `set_uniqueness_checker`. `Send + Sync` because the draft stores it and calls it from whatever
/// thread drives the check. Synchronous (matches the deterministic single-flight begin/complete).
#[export]
pub trait UniquenessChecker: Send + Sync {
    fn check_unique(&self, username: String) -> UniquenessVerdictFfi;
}

// =================================================================================================
// Poison-safe locking (no `unwrap`/`expect`/`panic!` in library code, per CLAUDE.md)
// =================================================================================================

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A batch of stream emissions to flush AFTER the store lock is dropped.
type Emits = Vec<(Arc<StreamProducer<ProfileSnapshot>>, ProfileSnapshot)>;

fn flush(emits: Emits) {
    for (producer, snapshot) in emits {
        producer.push(snapshot);
    }
}

// =================================================================================================
// The re-owned store state
// =================================================================================================

struct DraftEntry {
    /// `None` once the draft has been submitted (consumed by `commit`). The entry lingers as a
    /// tombstone until the foreign handle is dropped, so post-submit calls can be detected.
    draft: Option<ProfileDraft>,
    /// Per-draft snapshot stream (a draft is a mini feature-model, §4).
    producer: Arc<StreamProducer<ProfileSnapshot>>,
    /// Whether this draft participates in live rebase. Mirrors step-01's `Store`: only checkouts of
    /// an existing canonical register; create-flow drafts never rebase (invariant I12).
    rebases: bool,
}

struct StoreCore {
    canonical: Option<Profile>,
    version: u64,
    drafts: HashMap<u64, DraftEntry>,
    next_id: u64,
    /// The store-level (canonical) snapshot stream. Held here so both the store handle and a draft
    /// submitting itself can emit onto it.
    store_producer: Arc<StreamProducer<ProfileSnapshot>>,
}

// =================================================================================================
// Exported class: ProfileStoreFfi
// =================================================================================================

pub struct ProfileStoreFfi {
    core: Arc<Mutex<StoreCore>>,
}

impl Default for ProfileStoreFfi {
    fn default() -> Self {
        Self::new()
    }
}

#[export]
impl ProfileStoreFfi {
    pub fn new() -> ProfileStoreFfi {
        ProfileStoreFfi {
            core: Arc::new(Mutex::new(StoreCore {
                canonical: None,
                version: 0,
                drafts: HashMap::new(),
                next_id: 0,
                store_producer: Arc::new(StreamProducer::new(256)),
            })),
        }
    }

    /// Set/replace the canonical entity — seeds an initial profile, and simulates a background
    /// change: bumps version, rebases every live (rebasing) draft, adopts the new canonical.
    pub fn apply_canonical(&self, values: ProfileValues) -> Result<(), SubmitErrorFfi> {
        let profile =
            build_profile(&values).map_err(|report| SubmitErrorFfi::Validation { report })?;

        let emits = {
            let mut g = lock(&self.core);
            g.version += 1;
            let version = g.version;
            let mut emits: Emits = Vec::new();
            for entry in g.drafts.values_mut() {
                if !entry.rebases {
                    continue;
                }
                if let Some(draft) = entry.draft.as_mut() {
                    draft.rebase(&profile);
                    emits.push((entry.producer.clone(), build_draft_snapshot(draft)));
                }
            }
            g.canonical = Some(profile.clone());
            emits.push((
                g.store_producer.clone(),
                canonical_snapshot(&profile, version),
            ));
            emits
        };
        flush(emits);
        Ok(())
    }

    /// Check out a draft. Existing-canonical checkouts register for live rebase; create-flow
    /// checkouts (no canonical) do not (invariant I12).
    pub fn checkout(&self) -> ProfileDraftFfi {
        let mut g = lock(&self.core);
        let id = g.next_id;
        g.next_id += 1;
        let base = g.canonical.clone();
        let base_version = g.version;
        let rebases = base.is_some();
        let draft = ProfileDraft::from_canonical(base.as_ref(), base_version);
        let producer = Arc::new(StreamProducer::new(256));
        g.drafts.insert(
            id,
            DraftEntry {
                draft: Some(draft),
                producer: producer.clone(),
                rebases,
            },
        );
        ProfileDraftFfi {
            id,
            core: Arc::clone(&self.core),
            producer,
            checker: Mutex::new(None),
        }
    }

    /// Declared constraints for a field, projected from `ProfileField::constraints()`. Pure
    /// metadata (no store state), so it takes no lock. The app derives `maxLength`, character
    /// counters and required markers from THIS alone — no numeric constraint literal in Swift.
    pub fn constraints(&self, field: ProfileFieldId) -> Vec<ConstraintFfi> {
        to_core_field(field)
            .constraints()
            .into_iter()
            .map(to_constraint_ffi)
            .collect()
    }

    /// Live (present, un-submitted) draft count — for the deinit-deregistration probe.
    pub fn live_draft_count(&self) -> u32 {
        let g = lock(&self.core);
        g.drafts.values().filter(|e| e.draft.is_some()).count() as u32
    }

    /// The current canonical as a snapshot (all fields valid, in-sync), or `None` if unseeded.
    pub fn canonical(&self) -> Option<ProfileSnapshot> {
        let g = lock(&self.core);
        g.canonical
            .as_ref()
            .map(|p| canonical_snapshot(p, g.version))
    }

    /// Handle round-trip identity probe (Feature 1). Takes a draft handle back as a parameter and
    /// returns its id. If BoltFFI passes the *same* Rust object across the boundary, then
    /// `store.same_draft(d) == d.id()`; if it re-wraps or cannot pass exported class instances as
    /// parameters at all, this method reveals it (see the step-02 report).
    pub fn same_draft(&self, other: &ProfileDraftFfi) -> u64 {
        other.id
    }

    /// Store-level snapshot stream: a fresh canonical snapshot on every `apply_canonical`/submit.
    #[ffi_stream(item = ProfileSnapshot)]
    pub fn snapshots(&self) -> Arc<EventSubscription<ProfileSnapshot>> {
        let producer = {
            let g = lock(&self.core);
            g.store_producer.clone()
        };
        producer.subscribe()
    }
}

// =================================================================================================
// Exported class: ProfileDraftFfi
// =================================================================================================

pub struct ProfileDraftFfi {
    id: u64,
    core: Arc<Mutex<StoreCore>>,
    producer: Arc<StreamProducer<ProfileSnapshot>>,
    checker: Mutex<Option<Box<dyn UniquenessChecker>>>,
}

#[export]
impl ProfileDraftFfi {
    // ---- identity / lifecycle (Feature 1 probes) ----

    /// Stable per-draft id. Lets a test compare identity across a round-trip.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// `true` while the draft is present and un-submitted; `false` once submitted (tombstone) or
    /// after the entry is gone. Post-submit mutating calls are silent no-ops (see the report).
    pub fn is_live(&self) -> bool {
        let g = lock(&self.core);
        g.drafts
            .get(&self.id)
            .and_then(|e| e.draft.as_ref())
            .is_some()
    }

    // ---- setters (one per field; availability takes two args, never a tuple) ----

    pub fn try_set_username(&self, raw: String) -> Result<(), UsernameErrorFfi> {
        let (producer, snapshot, result) = {
            let mut g = lock(&self.core);
            let Some(draft) = live_draft_mut(&mut g, self.id) else {
                return Ok(()); // tombstone: no-op
            };
            let result = draft.try_set_username(raw).map_err(UsernameErrorFfi::from);
            let snapshot = build_draft_snapshot(draft);
            (self.producer.clone(), snapshot, result)
        };
        producer.push(snapshot);
        result
    }

    pub fn try_set_name(&self, raw: String) -> Result<(), PersonNameErrorFfi> {
        let (producer, snapshot, result) = {
            let mut g = lock(&self.core);
            let Some(draft) = live_draft_mut(&mut g, self.id) else {
                return Ok(());
            };
            let result = draft.try_set_name(raw).map_err(PersonNameErrorFfi::from);
            let snapshot = build_draft_snapshot(draft);
            (self.producer.clone(), snapshot, result)
        };
        producer.push(snapshot);
        result
    }

    pub fn try_set_email(&self, raw: String) -> Result<(), EmailErrorFfi> {
        let (producer, snapshot, result) = {
            let mut g = lock(&self.core);
            let Some(draft) = live_draft_mut(&mut g, self.id) else {
                return Ok(());
            };
            let result = draft.try_set_email(raw).map_err(EmailErrorFfi::from);
            let snapshot = build_draft_snapshot(draft);
            (self.producer.clone(), snapshot, result)
        };
        producer.push(snapshot);
        result
    }

    pub fn try_set_availability(
        &self,
        start: PlainDate,
        end: PlainDate,
    ) -> Result<(), DateRangeErrorFfi> {
        let (producer, snapshot, result) = {
            let mut g = lock(&self.core);
            let Some(draft) = live_draft_mut(&mut g, self.id) else {
                return Ok(());
            };
            let result = draft
                .try_set_availability(to_core_date(start), to_core_date(end))
                .map_err(DateRangeErrorFfi::from);
            let snapshot = build_draft_snapshot(draft);
            (self.producer.clone(), snapshot, result)
        };
        producer.push(snapshot);
        result
    }

    // ---- conflict resolution ----

    pub fn resolve_keep_mine(&self, field: ProfileFieldId) {
        self.resolve(field, true);
    }

    pub fn resolve_take_theirs(&self, field: ProfileFieldId) {
        self.resolve(field, false);
    }

    // ---- capability wiring + async check drive ----

    pub fn set_uniqueness_checker(&self, checker: Box<dyn UniquenessChecker>) {
        *lock(&self.checker) = Some(checker);
    }

    /// Drive one single-flight uniqueness check: begin (emit Pending-effect snapshot), call the
    /// foreign checker with NO lock held (reentrancy-safe), complete (emit result snapshot).
    /// Returns `false` if no checker is set or the draft is a tombstone. The check's effect is
    /// observable via `validate()` (a pending/failed check is a `username_unique` rule violation).
    pub fn run_username_check(&self) -> bool {
        // Take the checker OUT of its mutex for the whole operation, so we never hold the checker
        // lock across the outcall (a Swift checker may reentrantly touch this draft).
        let Some(checker) = lock(&self.checker).take() else {
            return false;
        };

        // Phase A (locked): begin, read the username text, build the pending snapshot.
        let begun = {
            let mut g = lock(&self.core);
            live_draft_mut(&mut g, self.id).map(|draft| {
                let username = current_username_text(draft);
                let token = draft.begin_username_check();
                (token, username, build_draft_snapshot(draft))
            })
        };
        let Some((token, username, pending)) = begun else {
            *lock(&self.checker) = Some(checker); // restore before bailing on a tombstone
            return false;
        };
        self.producer.push(pending); // emit Pending-effect (outside the lock)

        // Phase B (NO locks held): call the foreign checker.
        let verdict = checker.check_unique(username);
        *lock(&self.checker) = Some(checker); // restore
        let core_verdict: Result<(), CoreErrorData> = match verdict {
            UniquenessVerdictFfi::Unique => Ok(()),
            UniquenessVerdictFfi::Taken => Err(CoreErrorData::new("username_taken")),
        };

        // Phase C (locked): complete (the core discards a superseded token), build snapshot.
        let done = {
            let mut g = lock(&self.core);
            live_draft_mut(&mut g, self.id).map(|draft| {
                let _superseded = draft.complete_username_check(token, core_verdict);
                build_draft_snapshot(draft)
            })
        };
        if let Some(snapshot) = done {
            self.producer.push(snapshot);
        }
        true
    }

    // ---- validation + submit ----

    pub fn validate(&self) -> ValidationReportFfi {
        let g = lock(&self.core);
        match g.drafts.get(&self.id).and_then(|e| e.draft.as_ref()) {
            Some(draft) => project_report(&draft.validate()),
            None => ValidationReportFfi {
                field_errors: Vec::new(),
                rule_errors: Vec::new(),
            },
        }
    }

    /// Submit this draft: re-validate everything, then (if clean) commit and adopt the result as
    /// the new canonical, rebasing every other live draft. The draft is consumed either way; the
    /// foreign handle becomes a tombstone. Mirrors `bolted_core::Store::submit`.
    pub fn submit(&self) -> Result<(), SubmitErrorFfi> {
        let outcome = {
            let mut g = lock(&self.core);

            // Pre-check without consuming.
            // Refused pre-checks propagate straight out of `submit`: nothing consumed, no emit.
            match g.drafts.get(&self.id).and_then(|e| e.draft.as_ref()) {
                None => return Err(SubmitErrorFfi::AlreadySubmitted),
                Some(draft) => pre_submit_check(draft)?,
            }

            // Passed: move the draft out (single-owner move) and commit.
            let taken = g
                .drafts
                .get_mut(&self.id)
                .and_then(|entry| entry.draft.take());
            let Some(draft) = taken else {
                return Err(SubmitErrorFfi::AlreadySubmitted);
            };

            match draft.commit() {
                Ok(entity) => {
                    g.version += 1;
                    let version = g.version;
                    let mut emits: Emits = Vec::new();
                    for (other_id, entry) in g.drafts.iter_mut() {
                        if *other_id == self.id || !entry.rebases {
                            continue;
                        }
                        if let Some(other) = entry.draft.as_mut() {
                            other.rebase(&entity);
                            emits.push((entry.producer.clone(), build_draft_snapshot(other)));
                        }
                    }
                    g.canonical = Some(entity.clone());
                    emits.push((
                        g.store_producer.clone(),
                        canonical_snapshot(&entity, version),
                    ));
                    Ok(emits)
                }
                // Unreachable (validated above with no intervening rebase); defensively report.
                Err(report) => Err(SubmitErrorFfi::Validation {
                    report: project_report(&report),
                }),
            }
        };
        match outcome {
            Ok(emits) => {
                flush(emits);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    // ---- observation (Feature 2 probes) ----

    /// The draft's current state on demand — the recovery getter that makes drop-newest stream
    /// overflow non-fatal (a stalled subscriber can always re-read current state). Returns an
    /// all-unset snapshot for a tombstoned draft.
    pub fn snapshot(&self) -> ProfileSnapshot {
        let g = lock(&self.core);
        match g.drafts.get(&self.id).and_then(|e| e.draft.as_ref()) {
            Some(draft) => build_draft_snapshot(draft),
            None => build_draft_snapshot(&ProfileDraft::from_canonical(None, 0)),
        }
    }

    /// The draft's snapshot stream (default 256-slot ring per subscriber).
    #[ffi_stream(item = ProfileSnapshot)]
    pub fn snapshots(&self) -> Arc<EventSubscription<ProfileSnapshot>> {
        self.producer.subscribe()
    }

    /// A deliberately tiny (4-slot) subscription for the overflow / drop-newest probe.
    #[ffi_stream(item = ProfileSnapshot)]
    pub fn snapshots_small(&self) -> Arc<EventSubscription<ProfileSnapshot>> {
        self.producer.subscribe_with_capacity(4)
    }

    // ---- private helpers ----

    fn resolve(&self, field: ProfileFieldId, keep_mine: bool) {
        let (producer, snapshot) = {
            let mut g = lock(&self.core);
            let Some(draft) = live_draft_mut(&mut g, self.id) else {
                return;
            };
            let core_field = to_core_field(field);
            if keep_mine {
                draft.resolve_keep_mine(core_field);
            } else {
                draft.resolve_take_theirs(core_field);
            }
            (self.producer.clone(), build_draft_snapshot(draft))
        };
        producer.push(snapshot);
    }
}

impl Drop for ProfileDraftFfi {
    /// Deinit-deregistration: when the foreign handle is released, ARC runs this `Drop`, which
    /// prunes the draft from the store's registry (so `apply_canonical` stops rebasing a zombie and
    /// `live_draft_count` falls). This is the probe evidence for the §9 `close()` question.
    fn drop(&mut self) {
        let mut g = lock(&self.core);
        g.drafts.remove(&self.id);
    }
}

// =================================================================================================
// Snapshot construction + projections (generic `Field<V>` → monomorphic per-value DTOs)
// =================================================================================================

fn live_draft_mut(core: &mut StoreCore, id: u64) -> Option<&mut ProfileDraft> {
    core.drafts.get_mut(&id).and_then(|e| e.draft.as_mut())
}

fn build_draft_snapshot(draft: &ProfileDraft) -> ProfileSnapshot {
    ProfileSnapshot {
        username: project_username(&draft.username),
        name: project_name(&draft.name),
        email: project_email(&draft.email),
        availability: project_availability(&draft.availability),
        username_check: project_check(draft.username_check_state()),
        any_dirty: !draft.dirty_fields().is_empty(),
        conflicts: draft.conflicts().into_iter().map(to_field_id).collect(),
        status: to_status(draft.status()),
        version: draft.base_version(),
    }
}

/// Project the core check sub-state into its FFI shape (step-02 finding 7).
fn project_check(state: &CheckState<Result<(), CoreErrorData>>) -> UsernameCheckFfi {
    match state {
        CheckState::Idle => UsernameCheckFfi::Unchecked,
        CheckState::Pending { .. } => UsernameCheckFfi::Pending,
        CheckState::Done { verdict: Ok(()) } => UsernameCheckFfi::Passed,
        CheckState::Done { verdict: Err(e) } => UsernameCheckFfi::Failed {
            error: ErrorData::from(e.clone()),
        },
    }
}

fn to_constraint_ffi(c: Constraint) -> ConstraintFfi {
    match c {
        Constraint::Required => ConstraintFfi::Required,
        Constraint::LenChars { min, max } => ConstraintFfi::LenChars { min, max },
        Constraint::Custom(key) => ConstraintFfi::Custom {
            key: key.to_string(),
        },
    }
}

fn canonical_snapshot(profile: &Profile, version: u64) -> ProfileSnapshot {
    ProfileSnapshot {
        username: UsernameFieldState {
            validity: UsernameValidity::Valid {
                value: profile.username.as_str().to_string(),
            },
            sync: UsernameFieldSync::InSync,
            dirty: false,
        },
        name: PersonNameFieldState {
            validity: PersonNameValidity::Valid {
                value: profile.name.as_str().to_string(),
            },
            sync: PersonNameFieldSync::InSync,
            dirty: false,
        },
        email: EmailFieldState {
            validity: EmailValidity::Valid {
                value: profile.email.as_str().to_string(),
            },
            sync: EmailFieldSync::InSync,
            dirty: false,
        },
        availability: AvailabilityFieldState {
            validity: AvailabilityValidity::Valid {
                value: to_plain_range(&profile.availability),
            },
            sync: AvailabilityFieldSync::InSync,
            dirty: false,
        },
        // Canonical is committed state — there is no in-flight draft check to report.
        username_check: UsernameCheckFfi::Unchecked,
        any_dirty: false,
        conflicts: Vec::new(),
        status: DraftStatusFfi::Live,
        version,
    }
}

fn err_to_dto<E: Into<CoreErrorData>>(e: E) -> ErrorData {
    ErrorData::from(e.into())
}

fn project_username(f: &Field<Username>) -> UsernameFieldState {
    let validity = match f.validity() {
        Validity::Unset => UsernameValidity::Unset,
        Validity::Valid(v) => UsernameValidity::Valid {
            value: v.as_str().to_string(),
        },
        Validity::Invalid { raw, error } => UsernameValidity::Invalid {
            raw: raw.clone(),
            error: err_to_dto(error.clone()),
        },
    };
    let sync = match f.sync() {
        SyncState::InSync => UsernameFieldSync::InSync,
        SyncState::Conflicted { base, theirs } => UsernameFieldSync::Conflicted {
            base: base.as_ref().map(|u| u.as_str().to_string()),
            theirs: theirs.as_str().to_string(),
        },
    };
    UsernameFieldState {
        validity,
        sync,
        dirty: f.is_dirty(),
    }
}

fn project_name(f: &Field<PersonName>) -> PersonNameFieldState {
    let validity = match f.validity() {
        Validity::Unset => PersonNameValidity::Unset,
        Validity::Valid(v) => PersonNameValidity::Valid {
            value: v.as_str().to_string(),
        },
        Validity::Invalid { raw, error } => PersonNameValidity::Invalid {
            raw: raw.clone(),
            error: err_to_dto(error.clone()),
        },
    };
    let sync = match f.sync() {
        SyncState::InSync => PersonNameFieldSync::InSync,
        SyncState::Conflicted { base, theirs } => PersonNameFieldSync::Conflicted {
            base: base.as_ref().map(|u| u.as_str().to_string()),
            theirs: theirs.as_str().to_string(),
        },
    };
    PersonNameFieldState {
        validity,
        sync,
        dirty: f.is_dirty(),
    }
}

fn project_email(f: &Field<Email>) -> EmailFieldState {
    let validity = match f.validity() {
        Validity::Unset => EmailValidity::Unset,
        Validity::Valid(v) => EmailValidity::Valid {
            value: v.as_str().to_string(),
        },
        Validity::Invalid { raw, error } => EmailValidity::Invalid {
            raw: raw.clone(),
            error: err_to_dto(error.clone()),
        },
    };
    let sync = match f.sync() {
        SyncState::InSync => EmailFieldSync::InSync,
        SyncState::Conflicted { base, theirs } => EmailFieldSync::Conflicted {
            base: base.as_ref().map(|u| u.as_str().to_string()),
            theirs: theirs.as_str().to_string(),
        },
    };
    EmailFieldState {
        validity,
        sync,
        dirty: f.is_dirty(),
    }
}

fn project_availability(f: &Field<DateRange>) -> AvailabilityFieldState {
    let validity = match f.validity() {
        Validity::Unset => AvailabilityValidity::Unset,
        Validity::Valid(v) => AvailabilityValidity::Valid {
            value: to_plain_range(v),
        },
        Validity::Invalid { raw, error } => AvailabilityValidity::Invalid {
            raw: PlainDateRange {
                start: to_plain_date(raw.0),
                end: to_plain_date(raw.1),
            },
            error: err_to_dto(error.clone()),
        },
    };
    let sync = match f.sync() {
        SyncState::InSync => AvailabilityFieldSync::InSync,
        SyncState::Conflicted { base, theirs } => AvailabilityFieldSync::Conflicted {
            base: base.as_ref().map(to_plain_range),
            theirs: to_plain_range(theirs),
        },
    };
    AvailabilityFieldState {
        validity,
        sync,
        dirty: f.is_dirty(),
    }
}

fn project_report(r: &ValidationReport<ProfileField>) -> ValidationReportFfi {
    ValidationReportFfi {
        field_errors: r
            .field_errors
            .iter()
            .map(|(field, error)| FieldErrorFfi {
                field: to_field_id(*field),
                error: ErrorData::from(error.clone()),
            })
            .collect(),
        rule_errors: r
            .rule_errors
            .iter()
            .map(|v| RuleViolationFfi {
                rule: v.rule.to_string(),
                pins: v.pins.iter().map(|f| to_field_id(*f)).collect(),
                error: ErrorData::from(v.error.clone()),
            })
            .collect(),
    }
}

fn pre_submit_check(draft: &ProfileDraft) -> Result<(), SubmitErrorFfi> {
    match draft.status() {
        DraftStatus::Orphaned => return Err(SubmitErrorFfi::Orphaned),
        DraftStatus::Live => {}
    }
    let conflicts = draft.conflicts();
    if !conflicts.is_empty() {
        return Err(SubmitErrorFfi::Conflicted {
            fields: conflicts.into_iter().map(to_field_id).collect(),
        });
    }
    let report = draft.validate();
    if !report.is_ok() {
        return Err(SubmitErrorFfi::Validation {
            report: project_report(&report),
        });
    }
    Ok(())
}

fn build_profile(v: &ProfileValues) -> Result<Profile, ValidationReportFfi> {
    let username = Username::try_new(v.username.clone());
    let name = PersonName::try_new(v.name.clone());
    let email = Email::try_new(v.email.clone());
    let availability = DateRange::try_new((
        to_core_date(v.availability.start),
        to_core_date(v.availability.end),
    ));

    match (username, name, email, availability) {
        (Ok(username), Ok(name), Ok(email), Ok(availability)) => Ok(Profile {
            username,
            name,
            email,
            availability,
        }),
        (username, name, email, availability) => {
            let mut field_errors = Vec::new();
            if let Err(e) = username {
                field_errors.push(FieldErrorFfi {
                    field: ProfileFieldId::Username,
                    error: err_to_dto(e),
                });
            }
            if let Err(e) = name {
                field_errors.push(FieldErrorFfi {
                    field: ProfileFieldId::Name,
                    error: err_to_dto(e),
                });
            }
            if let Err(e) = email {
                field_errors.push(FieldErrorFfi {
                    field: ProfileFieldId::Email,
                    error: err_to_dto(e),
                });
            }
            if let Err(e) = availability {
                field_errors.push(FieldErrorFfi {
                    field: ProfileFieldId::Availability,
                    error: err_to_dto(e),
                });
            }
            Err(ValidationReportFfi {
                field_errors,
                rule_errors: Vec::new(),
            })
        }
    }
}

fn current_username_text(draft: &ProfileDraft) -> String {
    match draft.username.validity() {
        Validity::Valid(v) => v.as_str().to_string(),
        Validity::Invalid { raw, .. } => raw.clone(),
        Validity::Unset => String::new(),
    }
}

fn to_status(status: DraftStatus) -> DraftStatusFfi {
    match status {
        DraftStatus::Live => DraftStatusFfi::Live,
        DraftStatus::Orphaned => DraftStatusFfi::Orphaned,
    }
}

fn to_field_id(f: ProfileField) -> ProfileFieldId {
    match f {
        ProfileField::Username => ProfileFieldId::Username,
        ProfileField::Name => ProfileFieldId::Name,
        ProfileField::Email => ProfileFieldId::Email,
        ProfileField::Availability => ProfileFieldId::Availability,
    }
}

fn to_core_field(f: ProfileFieldId) -> ProfileField {
    match f {
        ProfileFieldId::Username => ProfileField::Username,
        ProfileFieldId::Name => ProfileField::Name,
        ProfileFieldId::Email => ProfileField::Email,
        ProfileFieldId::Availability => ProfileField::Availability,
    }
}
