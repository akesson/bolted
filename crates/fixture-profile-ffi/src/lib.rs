//! `fixture-profile-ffi` — the hand-written, "as-if-generated" BoltFFI wrapper around the
//! `fixture-profile` feature (step 02, the BoltFFI due-diligence probe). The ONLY crate importing
//! `boltffi`; `bolted-core` still never sees it.
//!
//! NOTE: unlike `bolted-core`/`fixture-profile`, this crate does NOT `#![forbid(unsafe_code)]` —
//! `#[export]` expands to `extern "C"` shims containing `unsafe`, so the forbid would reject
//! generated code. FINDING: the FFI boundary is exactly where the no-unsafe discipline stops.
//!
//! ## The store this wrapper does NOT own (step 08, D16)
//!
//! Step 02 re-owned the entire store loop here: `bolted_core::Store` was `Rc<RefCell<…>>`/`Weak`,
//! therefore not `Send`, therefore unusable behind the `Mutex` that BoltFFI's thread-shared classes
//! require. So this crate re-implemented checkout registration, the `apply_canonical` rebase
//! fan-out, and `submit`, straight against `Field`/`Draft`. Step 07's `restore` doubled that
//! duplication. The two copies had already drifted — `live_draft_count` meant different things on
//! each side of the boundary (C22).
//!
//! Since D16 the core store owns its drafts, hands out `DraftId`s, and holds no lock of its own, so
//! **this wrapper simply puts it behind a `Mutex`.** What is left here is what was always genuinely
//! FFI: stream producers, the foreign checker, DTO projection, and the discipline below.
//!
//! Wrapper invariant, enforced by discipline (the reentrancy tests punish violations):
//! **never emit a stream event or invoke a foreign callback while holding the `Mutex`.** Every
//! mutation locks, mutates, builds the snapshot, DROPS the lock, then pushes. The core makes this
//! *expressible* rather than merely possible: `apply_canonical` and `submit` return the ids they
//! moved, as data, so the emit list can be assembled under the lock and flushed after it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use boltffi::*;

use bolted_core::report::ErrorData as CoreErrorData;
use bolted_core::{
    CheckState, Constraint, Draft, DraftId, DraftStatus, Field, Stashable, StoreDraft, SubmitError,
    SyncState, ValidationReport, Validity, Value,
};
use fixture_profile::{
    DateRange, Email, PersonName, Profile, ProfileDraft, ProfileField, ProfileStore, Username,
};

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

/// Everything the one `Mutex` protects: the core store, plus the per-draft snapshot streams a draft
/// needs because it is a mini feature-model (§4). Canonical, versions, the draft registry, the
/// rebase bookkeeping and the submit path all live in `store` — none of it is written here.
struct FfiState {
    store: ProfileStore,
    producers: BTreeMap<DraftId, Arc<StreamProducer<ProfileSnapshot>>>,
    /// The store-level (canonical) snapshot stream. Held here so both the store handle and a draft
    /// submitting itself can emit onto it.
    store_producer: Arc<StreamProducer<ProfileSnapshot>>,
}

// =================================================================================================
// Exported class: ProfileStoreFfi
// =================================================================================================

pub struct ProfileStoreFfi {
    core: Arc<Mutex<FfiState>>,
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
            core: Arc::new(Mutex::new(FfiState {
                store: ProfileStore::new(None),
                producers: BTreeMap::new(),
                store_producer: Arc::new(StreamProducer::new(256)),
            })),
        }
    }

    /// Set/replace the canonical entity — seeds an initial profile, and simulates a background
    /// change. `Store::apply_canonical` does all of it and reports which drafts it moved.
    pub fn apply_canonical(&self, values: ProfileValues) -> Result<(), SubmitErrorFfi> {
        let profile =
            build_profile(&values).map_err(|report| SubmitErrorFfi::Validation { report })?;

        let emits = {
            let mut g = lock(&self.core);
            let rebased = g.store.apply_canonical(profile.clone());
            let mut emits = draft_emits(&g, &rebased);
            emits.push((
                g.store_producer.clone(),
                canonical_snapshot(&profile, g.store.version()),
            ));
            emits
        };
        flush(emits);
        Ok(())
    }

    /// Check out a draft. Existing-canonical checkouts register for live rebase; create-flow
    /// checkouts (no canonical) do not (conformance C12).
    pub fn checkout(&self) -> ProfileDraftFfi {
        let mut g = lock(&self.core);
        let id = g.store.checkout();
        let producer = register_producer(&mut g, id);
        ProfileDraftFfi {
            id,
            core: Arc::clone(&self.core),
            producer,
            checker: Mutex::new(None),
        }
    }

    /// Restore a draft the shell stashed before its process was killed (C21).
    ///
    /// The rebase inside `Store::restore` is the whole point: a field whose canonical moved while the
    /// process was dead comes back **conflicted**, not silently dirty over a base it never saw. The
    /// restored draft's async verdict is `Unchecked`, so C16 will demand a fresh check before it
    /// submits a dirty username — which is why the shell must render `username_check_required` as
    /// progress rather than as an error on the first frame after a restore.
    pub fn restore(&self, stash: ProfileStashFfi) -> ProfileDraftFfi {
        let mut g = lock(&self.core);
        let id = g.store.restore(&to_core_stash(&stash));
        let producer = register_producer(&mut g, id);
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

    /// How many drafts exist: checked out or restored, not yet submitted or closed. Used by the
    /// deinit-deregistration probe. **Not** the same question as [`Self::rebasing_draft_count`] —
    /// see C22, and the five steps these two spent disagreeing under one name.
    pub fn live_draft_count(&self) -> u32 {
        let g = lock(&self.core);
        g.store.draft_count() as u32
    }

    /// How many drafts the next canonical change would rebase: not a create-flow draft (C12), not an
    /// orphan (C11). The count the core always meant by `live_draft_count`, now asked for by name.
    pub fn rebasing_draft_count(&self) -> u32 {
        let g = lock(&self.core);
        g.store.rebasing_draft_count() as u32
    }

    /// The current canonical as a snapshot (all fields valid, in-sync), or `None` if unseeded.
    pub fn canonical(&self) -> Option<ProfileSnapshot> {
        let g = lock(&self.core);
        let version = g.store.version();
        g.store.canonical().map(|p| canonical_snapshot(p, version))
    }

    /// Handle round-trip identity probe (Feature 1). Takes a draft handle back as a parameter and
    /// returns its id. If BoltFFI passes the *same* Rust object across the boundary, then
    /// `store.same_draft(d) == d.id()`; if it re-wraps or cannot pass exported class instances as
    /// parameters at all, this method reveals it (see the step-02 report).
    pub fn same_draft(&self, other: &ProfileDraftFfi) -> u64 {
        other.id.as_u64()
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
    id: DraftId,
    core: Arc<Mutex<FfiState>>,
    producer: Arc<StreamProducer<ProfileSnapshot>>,
    checker: Mutex<Option<Box<dyn UniquenessChecker>>>,
}

#[export]
impl ProfileDraftFfi {
    // ---- identity / lifecycle (Feature 1 probes) ----

    /// Stable per-draft id. Lets a test compare identity across a round-trip.
    pub fn id(&self) -> u64 {
        self.id.as_u64()
    }

    /// `true` while the draft is present and un-submitted; `false` once submitted (C17) or closed
    /// (C18). Post-submit mutating calls are silent no-ops (see the step-02 report).
    pub fn is_live(&self) -> bool {
        let g = lock(&self.core);
        g.store.is_live(self.id)
    }

    // ---- setters (one per field; availability takes two args, never a tuple) ----

    pub fn try_set_username(&self, raw: String) -> Result<(), UsernameErrorFfi> {
        let (producer, snapshot, result) = {
            let mut g = lock(&self.core);
            let Some(draft) = g.store.draft_mut(self.id) else {
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
            let Some(draft) = g.store.draft_mut(self.id) else {
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
            let Some(draft) = g.store.draft_mut(self.id) else {
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
            let Some(draft) = g.store.draft_mut(self.id) else {
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
            g.store.draft_mut(self.id).map(|draft| {
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
            g.store.draft_mut(self.id).map(|draft| {
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
        match g.store.draft(self.id) {
            Some(draft) => project_report(&draft.validate()),
            None => ValidationReportFfi {
                field_errors: Vec::new(),
                rule_errors: Vec::new(),
            },
        }
    }

    /// Submit this draft: commit it and adopt the result as the new canonical, rebasing every other
    /// registered draft. On success the draft is released and the foreign handle becomes a tombstone
    /// (C17). On refusal the draft stays put under the same id: the edit session survives.
    ///
    /// The whole transaction is `Store::submit`. What is left here is the emit list, assembled under
    /// the lock from the ids the core returned and flushed after it. Step 02 wrote this loop by hand
    /// and worried, correctly, that it would drift from the core's.
    pub fn submit(&self) -> Result<(), SubmitErrorFfi> {
        let emits = {
            let mut g = lock(&self.core);
            let rebased = g.store.submit(self.id).map_err(submit_error_to_dto)?;
            let mut emits = draft_emits(&g, &rebased);
            let version = g.store.version();
            if let Some(entity) = g.store.canonical() {
                emits.push((
                    g.store_producer.clone(),
                    canonical_snapshot(entity, version),
                ));
            }
            emits
        };
        flush(emits);
        Ok(())
    }

    // ---- observation (Feature 2 probes) ----

    /// Flatten this draft to serializable data so the shell can persist it across process death
    /// (C20). A tombstoned draft has nothing to stash and yields the create-flow (all-`None`) shape,
    /// consistent with `snapshot()`.
    ///
    /// The shell calls this from wherever its platform says "you are about to be killed" —
    /// `onSaveInstanceState` / `SavedStateHandle` on Android. Restoring is `ProfileStoreFfi::restore`.
    pub fn stash(&self) -> ProfileStashFfi {
        let g = lock(&self.core);
        match g.store.draft(self.id) {
            Some(draft) => to_stash_ffi(&draft.stash()),
            None => to_stash_ffi(&ProfileDraft::from_canonical(None, 0).stash()),
        }
    }

    /// The draft's current state on demand — the recovery getter that makes drop-newest stream
    /// overflow non-fatal (a stalled subscriber can always re-read current state). Returns an
    /// all-unset snapshot for a tombstoned draft.
    pub fn snapshot(&self) -> ProfileSnapshot {
        let g = lock(&self.core);
        match g.store.draft(self.id) {
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
            let Some(draft) = g.store.draft_mut(self.id) else {
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
    /// Deinit-deregistration: when the foreign handle is released, ARC runs this `Drop`, which calls
    /// `Store::close` (so `apply_canonical` stops rebasing a zombie and `live_draft_count` falls).
    ///
    /// This is the *shell* calling `close`, not the framework doing it for free — exactly what C18
    /// now says. Kotlin's GC never runs it, which is why `AutoCloseable`/`onCleared()` are mandatory
    /// there (step 05, H1) and why `bolted-ffi` still owes a `Cleaner` backstop (§9).
    fn drop(&mut self) {
        let mut g = lock(&self.core);
        g.store.close(self.id);
        g.producers.remove(&self.id);
    }
}

// =================================================================================================
// Snapshot construction + projections (generic `Field<V>` → monomorphic per-value DTOs)
// =================================================================================================

/// Give a freshly registered draft its own snapshot stream (a draft is a mini feature-model, §4).
///
/// This is all that is left of the wrapper's former `adopt_locked`, which was a hand-written copy of
/// `Store::adopt` — checkout registration, the orphan/create-flow table, id issuance, the lot. It
/// had to match the core's table exactly or a shell's behaviour would depend on which side of the
/// FFI it sat, and it was enforced by nothing but a comment saying so.
fn register_producer(
    g: &mut MutexGuard<'_, FfiState>,
    id: DraftId,
) -> Arc<StreamProducer<ProfileSnapshot>> {
    let producer = Arc::new(StreamProducer::new(256));
    g.producers.insert(id, producer.clone());
    producer
}

/// One snapshot per draft the core just moved, ready to flush **after** the lock is dropped.
///
/// The core returns ids rather than calling back, so there is no way to accidentally emit under the
/// lock from inside a rebase fan-out. Step 02's hardest-won invariant became a property of the type
/// signature.
fn draft_emits(g: &MutexGuard<'_, FfiState>, ids: &[DraftId]) -> Emits {
    ids.iter()
        .filter_map(|id| {
            let draft = g.store.draft(*id)?;
            let producer = g.producers.get(id)?;
            Some((producer.clone(), build_draft_snapshot(draft)))
        })
        .collect()
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
        // The DTO keeps the full 3-way shape for shells; the core no longer stores the ancestor
        // twice, so it is read from the field itself (step-01 F7).
        SyncState::Conflicted { theirs } => UsernameFieldSync::Conflicted {
            base: f.base().map(|u| u.as_str().to_string()),
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
        // The DTO keeps the full 3-way shape for shells; the core no longer stores the ancestor
        // twice, so it is read from the field itself (step-01 F7).
        SyncState::Conflicted { theirs } => PersonNameFieldSync::Conflicted {
            base: f.base().map(|u| u.as_str().to_string()),
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
        // The DTO keeps the full 3-way shape for shells; the core no longer stores the ancestor
        // twice, so it is read from the field itself (step-01 F7).
        SyncState::Conflicted { theirs } => EmailFieldSync::Conflicted {
            base: f.base().map(|u| u.as_str().to_string()),
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
        SyncState::Conflicted { theirs } => AvailabilityFieldSync::Conflicted {
            base: f.base().map(to_plain_range),
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

/// Project the core's typed `SubmitError` onto the FFI enum — a one-to-one mapping now that the
/// core store raises `AlreadySubmitted` itself. Step 02 had to invent that variant wrapper-side,
/// because the foreign handle outlives the core draft; step 06 (D5) put it in the core; step 08
/// finally lets the wrapper stop translating between two taxonomies of the same failures.
fn submit_error_to_dto(e: SubmitError<ProfileField>) -> SubmitErrorFfi {
    match e {
        SubmitError::Validation(report) => SubmitErrorFfi::Validation {
            report: project_report(&report),
        },
        SubmitError::Conflicted { fields } => SubmitErrorFfi::Conflicted {
            fields: fields.into_iter().map(to_field_id).collect(),
        },
        SubmitError::Orphaned => SubmitErrorFfi::Orphaned,
        SubmitError::AlreadySubmitted => SubmitErrorFfi::AlreadySubmitted,
    }
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
