//! `collection-core` — the collection-facet spike (step 28, M0).
//!
//! Every Bolted facet so far exports **one** canonical entity through a [`bolted_core::Store`].
//! This spike builds the first facet whose store owns **many** canonical rows: a notes inbox,
//! observed through **windows**. It is disposable (`spikes/collection/`), depends on the frozen
//! `bolted-core` unmodified, and banks evidence for ARCHITECTURE §9's windowed-collections item.
//!
//! ## M0 shape (this file)
//!
//! - [`CollectionStore`] owns canonical [`NoteRow`]s keyed by entity id ([`NoteId`]).
//! - [`CollectionStore::apply_canonical`] — upsert/delete via [`CanonicalChange`] — is the **only**
//!   collection mutation in M0 (the canonical-source path; row drafts arrive in M1).
//! - A collection [`version`](CollectionStore::version) moves on every canonical change and is the
//!   observer's **change tick**: an observer polls [`latest`](CollectionStore::latest) and compares
//!   `version` (the §8 pull idiom). No delta or event delivery of any kind (D37).
//! - Windows are per-observer core-side handles ([`WindowId`]) with an explicit
//!   [`close_window`](CollectionStore::close_window) as the only release path and a live-window
//!   count the store answers ([`live_window_count`](CollectionStore::live_window_count) — the C22
//!   discipline). The natural order is fixed in M0: `updated_at` descending, `id` ascending as the
//!   tiebreak (queries arrive in M2).
//!
//! ## M1 shape (drafts over the collection)
//!
//! [`checkout`](CollectionStore::checkout) hands back a [`RowDraftId`] naming a per-row
//! [`NoteRowDraft`] — a hand-written draft mirroring `fixture-note`'s `NoteDraft`, over the frozen
//! [`bolted_core::Draft`] / [`bolted_core::StoreDraft`] traits and [`bolted_core::Field`], all
//! reused **unmodified**. [`apply_canonical`](CollectionStore::apply_canonical) now returns its
//! rebase fan-out **as data** (`Vec<RowDraftId>` — never a callback), and
//! [`submit`](CollectionStore::submit) commits a validated draft back through the same
//! canonical-source path M0 built, so windows see the result for free.
//!
//! ## The structural answer (the headline evidence)
//!
//! A row-draft is **not** a per-row [`bolted_core::Store`]. `Store<D>` structurally owns exactly one
//! `canonical: Option<Entity>`; a collection's canonicals live once, in
//! [`CollectionStore::rows`](CollectionStore) (the window-projection source). Hosting drafts in
//! per-row `Store`s would hold each canonical row **twice** (the F7 "two facts" smell), fracture the
//! `DraftId` namespace across stores, and cannot supply a create-flow id. So the collection is a
//! **peer** of `Store<D>`: it re-implements `Store`'s thin draft-registry loop
//! ([`adopt`](CollectionStore::adopt) / `apply_canonical` fan-out / `submit`) with **one** change —
//! the canonical is looked up **by `RowId`** (`self.rows.get(&draft.id())`) instead of a single
//! `self.canonical`. Everything below that line — `Field`, the three-way merge, `commit_gates`,
//! orphaning, the `rebases` flag — is inherited byte-for-byte. `DraftId` itself is sealed (its inner
//! `u64` has no public constructor), so a peer store issues its own [`RowDraftId`], exactly as M0's
//! [`WindowId`] does: the id type *cannot* be borrowed, which is itself evidence of the peer split.
//!
//! ## What is reused unmodified
//!
//! [`Title`] is a spike-local [`bolted_core::Value`] — the tier-1 constrained value type, reused
//! byte-for-byte as `fixture-note` writes it. A canonical [`NoteRow`] is plain owned data;
//! [`NoteRowDraft`] carries an editable [`Field<Title>`](bolted_core::Field) plus its identity
//! ([`NoteId`]) and its input-provided `updated_at` (D35), and delegates every draft judgement to
//! the frozen traits.
#![forbid(unsafe_code)]

use bolted_core::{
    CommitError, Constraint, Draft, DraftStatus, ErrorData, Field, StoreDraft, SubmitError,
    ValidationReport, Value, commit_gates,
};
use std::collections::BTreeMap;
use std::ops::Range;

// =================================================================================================
// The spike-local value type: a `Title`-style constrained value, reusing `bolted_core::Value`.
// =================================================================================================

/// Trim; 1..=40 chars. Spike-local, hand-written exactly as `#[bolted::value]` (and `fixture-note`)
/// would emit it — the reuse this spike is meant to demonstrate.
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

// =================================================================================================
// Identity: the entity key. `RowId` is identity, never index (D-candidate: the entity key).
// =================================================================================================

/// The entity key of a note. This is the collection's `RowId` candidate — **identity, never
/// index**: it is stable across re-sorts and is what a shell diffs consecutive snapshots by.
///
/// `Copy`/`Ord`/`Hash`, monotonic in the caller's hands (M0 has no create-flow of its own; ids
/// arrive with the canonical row, exactly as a server key would).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteId(pub u64);

/// A canonical note: entity-key id, a constrained [`Title`], and an **input-provided** timestamp.
///
/// `updated_at` enters as data (D35 — no ambient nondeterminism); there is no `SystemTime::now()`
/// anywhere in this crate. Its unit is the caller's business; the store only orders by it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRow {
    pub id: NoteId,
    pub title: Title,
    pub updated_at: i64,
}

// =================================================================================================
// Windows: per-observer handles into the ordered collection.
// =================================================================================================

/// A window's identity within one [`CollectionStore`]. `Copy`, monotonically issued, **never
/// reused** — the same discipline `bolted_core::DraftId` applies to drafts, on the read side. A
/// stale id is permanently dead rather than dangerously recycled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(u64);

impl WindowId {
    /// The wire form, for symmetry with `DraftId::as_u64` (unused in M0; here for parity).
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// One open window's mutable state. In M0 that is only its requested range; the order is fixed
/// (M2 adds a per-window `Query` here).
struct WindowState {
    range: Range<u32>,
}

/// A read-only projection of one row inside a snapshot, carrying its stable [`NoteId`] (the
/// `RowId`). Deltas never cross the boundary (D37) — a shell diffs consecutive snapshots by this
/// id itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    pub id: NoteId,
    pub title: String,
    pub updated_at: i64,
}

/// What one window's observer reads: snapshot-authoritative, newest-wins, coalescing-legal (D37).
///
/// `version` is the collection version at read time (the change tick). `total_count` is the whole
/// collection's row count (M0 has no filter; its semantics under a filter is an M2 finding).
/// `range` is the caller's requested range **after tail-clamping**. `rows` are the projections in
/// that clamped range, in natural order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub version: u64,
    pub total_count: u32,
    pub range: Range<u32>,
    pub rows: Vec<RowView>,
}

// =================================================================================================
// The many-canonical store.
// =================================================================================================

/// A canonical-source change: the only collection mutation in M0.
///
/// `Upsert` inserts or replaces the row at its id; `Delete` removes the id if present. Both are the
/// canonical-source path (already-validated server truth arriving), the many-canonical analog of
/// `bolted_core::Store::{apply_canonical, delete_canonical}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalChange {
    Upsert(NoteRow),
    Delete(NoteId),
}

/// A store owning **many** canonical rows, observed through windows.
///
/// Like `bolted_core::Store`, it ships no lock and returns nothing it could call out to: an
/// observer *pulls* [`latest`](Self::latest). `BTreeMap` keeps id-order iteration deterministic,
/// which costs nothing at these sizes and makes the natural-order re-projection reproducible.
pub struct CollectionStore {
    rows: BTreeMap<NoteId, NoteRow>,
    version: u64,
    windows: BTreeMap<WindowId, WindowState>,
    next_window_id: u64,
    /// The draft registry — `Store`'s `drafts` map, made a peer: keyed by a collection-global
    /// [`RowDraftId`], each entry naming the row it edits. `Store<D>` cannot host this because it
    /// binds one canonical; the registry loop below is otherwise `Store`'s, unchanged.
    drafts: BTreeMap<RowDraftId, RowDraftEntry>,
    next_draft_id: u64,
}

impl Default for CollectionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CollectionStore {
    pub fn new() -> Self {
        CollectionStore {
            rows: BTreeMap::new(),
            version: 0,
            windows: BTreeMap::new(),
            next_window_id: 0,
            drafts: BTreeMap::new(),
            next_draft_id: 0,
        }
    }

    /// The collection version — the observer's change tick. Moves on every canonical change.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// How many rows the collection holds (the whole-collection count; no filter in M0).
    pub fn total_count(&self) -> u32 {
        self.rows.len() as u32
    }

    /// How many windows are open. The C22 analog on the read side: [`close_window`] is the only
    /// path that lowers it, and a window the store never issued was never counted.
    pub fn live_window_count(&self) -> usize {
        self.windows.len()
    }

    // ---- the one mutation --------------------------------------------------------------------

    /// Apply a canonical-source change (upsert or delete). Bumps [`version`](Self::version)
    /// **per canonical event**, unconditionally — a delete of an absent id is still an announced
    /// canonical event, exactly as `bolted_core::Store::apply_canonical` bumps unconditionally.
    /// The naive design touches no window here; every [`latest`](Self::latest) re-projects from the
    /// current rows (M2 measures that price).
    ///
    /// Returns the **fan-out as data** (`Vec<RowDraftId>`): the row-drafts this change moved, in id
    /// order, so a shell can emit one snapshot per affected draft after dropping any lock — never a
    /// callback (`bolted_core::Store::apply_canonical`'s discipline). An `Upsert` rebases every live
    /// draft on that row id; a `Delete` orphans them (C11). A draft on any *other* row, a create-flow
    /// draft (C12), and an orphan are untouched — the `rebases` gate is `Store`'s, verbatim.
    pub fn apply_canonical(&mut self, change: CanonicalChange) -> Vec<RowDraftId> {
        self.version += 1;
        let version = self.version;
        let mut affected = Vec::new();
        match change {
            CanonicalChange::Upsert(row) => {
                for (draft_id, entry) in self.drafts.iter_mut() {
                    if entry.rebases && entry.draft.id() == row.id {
                        entry.draft.rebase(&row, version);
                        affected.push(*draft_id);
                    }
                }
                self.rows.insert(row.id, row);
            }
            CanonicalChange::Delete(id) => {
                for (draft_id, entry) in self.drafts.iter_mut() {
                    if entry.rebases && entry.draft.id() == id {
                        entry.draft.orphan();
                        entry.rebases = false;
                        affected.push(*draft_id);
                    }
                }
                self.rows.remove(&id);
            }
        }
        affected
    }

    // ---- windows -----------------------------------------------------------------------------

    /// Open a window over `range` of the natural order. Returns its handle. The range is stored as
    /// requested and clamped only at read time (so the same window stays honest as the collection
    /// grows and shrinks under it).
    pub fn open_window(&mut self, range: Range<u32>) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        self.windows.insert(id, WindowState { range });
        id
    }

    /// Set a window's requested range (an ordinary input). No-op for an unknown/closed window.
    pub fn set_range(&mut self, w: WindowId, range: Range<u32>) {
        if let Some(state) = self.windows.get_mut(&w) {
            state.range = range;
        }
    }

    /// Release a window. Idempotent; closing an id the store never issued is a no-op. **The only
    /// release path** — there is no owner to drop (the `bolted_core::Store::close` discipline).
    pub fn close_window(&mut self, w: WindowId) {
        self.windows.remove(&w);
    }

    /// The window's current snapshot — pull-based, newest state only. `None` if `w` is not a live
    /// window (unknown or closed), which is how an observer learns its handle is dead.
    pub fn latest(&self, w: WindowId) -> Option<WindowSnapshot> {
        let state = self.windows.get(&w)?;
        let ordered = self.natural_order();
        let total = ordered.len() as u32;
        let range = clamp_range(&state.range, total);
        let rows = ordered[range.start as usize..range.end as usize]
            .iter()
            .map(|row| RowView {
                id: row.id,
                title: row.title.as_str().to_string(),
                updated_at: row.updated_at,
            })
            .collect();
        Some(WindowSnapshot {
            version: self.version,
            total_count: total,
            range,
            rows,
        })
    }

    // ---- row drafts (the many-canonical peer of `Store`'s draft registry) --------------------

    /// Check out a draft over the row `row_id`, or `None` if no such row exists. The draft is built
    /// from the current canonical row and registered for live rebase — `Store::checkout` for one row
    /// of many.
    pub fn checkout(&mut self, row_id: NoteId) -> Option<RowDraftId> {
        let base = self.rows.get(&row_id)?;
        let draft = NoteRowDraft::from_canonical(Some(base), self.version);
        Some(self.adopt(draft))
    }

    /// Begin a **create-flow** row: a draft with no base entity, its identity supplied by the caller
    /// (`id` — a client-generated key, D35's no-ambient-nondeterminism) and its initial `updated_at`
    /// as input. It never rebases and never orphans (C12); [`submit`](Self::submit) **inserts** it.
    ///
    /// Identity comes in as data because the frozen `StoreDraft::from_canonical(None, ..)` is
    /// identity-*blind* — a single-entity `Store` needs no id in create-flow (the store *is* the
    /// identity), but a collection row does, before any canonical exists. See the step-28 report.
    pub fn checkout_new(&mut self, id: NoteId, updated_at: i64) -> RowDraftId {
        self.adopt(NoteRowDraft::new_create(id, updated_at, self.version))
    }

    /// Register a freshly built draft, mirroring `bolted_core::Store::adopt` **exactly** — the only
    /// change is the canonical lookup: `self.rows.get(&draft.id())` (many, keyed by `RowId`) in place
    /// of `Store`'s single `self.canonical`. An entity-backed draft over a live row rebases onto it
    /// (C19 idempotence) and registers; over a now-absent row it orphans (C11); a create-flow draft
    /// is untouched and unregistered (C12).
    fn adopt(&mut self, mut draft: NoteRowDraft) -> RowDraftId {
        let rebases = match (draft.is_based(), self.rows.get(&draft.id())) {
            (true, Some(row)) => {
                draft.rebase(row, self.version);
                true
            }
            (true, None) => {
                draft.orphan();
                false
            }
            (false, _) => false,
        };
        let id = RowDraftId(self.next_draft_id);
        self.next_draft_id += 1;
        self.drafts.insert(id, RowDraftEntry { draft, rebases });
        id
    }

    /// Is `id` still a live draft? False once submitted or closed, and for an id never issued.
    pub fn is_live(&self, id: RowDraftId) -> bool {
        self.drafts.contains_key(&id)
    }

    /// Shared access to a row draft. `None` once it is gone.
    pub fn draft(&self, id: RowDraftId) -> Option<&NoteRowDraft> {
        self.drafts.get(&id).map(|e| &e.draft)
    }

    /// Mutable access to a row draft (setters, resolve). `None` once it is gone.
    pub fn draft_mut(&mut self, id: RowDraftId) -> Option<&mut NoteRowDraft> {
        self.drafts.get_mut(&id).map(|e| &mut e.draft)
    }

    /// How many row drafts exist (the C22 "a draft exists" count). Includes create-flow drafts and
    /// orphans.
    pub fn draft_count(&self) -> usize {
        self.drafts.len()
    }

    /// How many row drafts the next matching canonical change would rebase (the C22 "a draft rebases"
    /// count). A create-flow draft (C12) and an orphan (C11) are not counted.
    pub fn rebasing_draft_count(&self) -> usize {
        self.drafts.values().filter(|e| e.rebases).count()
    }

    /// Release a row draft. Idempotent; closing an id never issued is a no-op. The only release path.
    pub fn close(&mut self, id: RowDraftId) {
        self.drafts.remove(&id);
    }

    /// Submit a row draft transactionally — `bolted_core::Store::submit`, over many canonicals. On
    /// success the committed row is applied as a canonical `Upsert` (inserting a create-flow row, or
    /// landing an edit), every *other* draft on that row rebases onto it (their ids returned), and
    /// `id` is released — a second submit is `AlreadySubmitted` (C17). On refusal the draft goes back
    /// under the same id, its typed reason surfaced (C07 precedence via the frozen `commit_gates`).
    pub fn submit(&mut self, id: RowDraftId) -> Result<Vec<RowDraftId>, SubmitError<RowField>> {
        let Some(entry) = self.drafts.remove(&id) else {
            return Err(SubmitError::AlreadySubmitted);
        };
        match entry.draft.commit() {
            Ok(row) => Ok(self.apply_canonical(CanonicalChange::Upsert(row))),
            Err((draft, error)) => {
                self.drafts.insert(
                    id,
                    RowDraftEntry {
                        draft,
                        rebases: entry.rebases,
                    },
                );
                Err(error.into())
            }
        }
    }

    /// The fixed natural order: `updated_at` descending, `id` ascending as the tiebreak. Naive
    /// full sort on every read — the etiquette M2 prices.
    fn natural_order(&self) -> Vec<&NoteRow> {
        let mut rows: Vec<&NoteRow> = self.rows.values().collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        rows
    }
}

// =================================================================================================
// Row drafts: the per-row edit session, over the frozen draft/field machinery unmodified.
// =================================================================================================

/// A row draft's identity within one [`CollectionStore`]. `Copy`, monotonically issued, **never
/// reused** — the discipline `bolted_core::DraftId` applies per `Store`, here made collection-global.
///
/// It is a spike-local type, not `bolted_core::DraftId`, for a concrete reason: `DraftId`'s inner
/// `u64` has no public constructor (it is a capability the issuing `Store` alone can mint). A peer
/// store therefore mints its own id — the sealed constructor is itself evidence that the registry is
/// a peer of `Store`, not a client of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowDraftId(u64);

impl RowDraftId {
    /// The wire form, for symmetry with `DraftId::as_u64` (unused in the spike; here for parity).
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// The one editable field of a row. A `NoteRow`'s `id` is identity (never edited) and its
/// `updated_at` is input-provided canonical metadata (never a user-edited field), so `Title` is the
/// whole field set — the deliberately-minimal analog of `fixture-note`'s `NoteField`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowField {
    Title,
}

/// One checked-out row draft and whether canonical changes move it — [`Entry`] from
/// `bolted_core::store`, spike-local. `rebases` is set by [`CollectionStore::adopt`] and cleared on
/// orphan (C11/C15), exactly as the frozen store does.
struct RowDraftEntry {
    draft: NoteRowDraft,
    rebases: bool,
}

/// A per-row draft, mirroring `fixture-note`'s `NoteDraft`: an editable [`Field<Title>`] over the
/// frozen draft machinery, plus the two facts a *collection* row carries that a single-entity draft
/// does not — its identity [`NoteId`] and its input-provided `updated_at` (D35).
///
/// `id` is not a [`Field`]: identity is never edited. `updated_at` is not a `Field` either: it is
/// canonical ordering metadata the user never types, so it is carried as data and **adopts theirs on
/// every rebase** (like a clean field), which is what lets an edit land at the row's *new* sort
/// position after a canonical move (W7).
pub struct NoteRowDraft {
    id: NoteId,
    title: Field<Title>,
    updated_at: i64,
    status: DraftStatus,
    base_version: u64,
}

impl NoteRowDraft {
    /// A create-flow draft with a caller-supplied identity and initial timestamp (both input, D35).
    /// No field carries a base, so [`is_based`](StoreDraft::is_based) is `false` — it never rebases
    /// and never orphans (C12).
    fn new_create(id: NoteId, updated_at: i64, base_version: u64) -> Self {
        NoteRowDraft {
            id,
            title: Field::new_unset(),
            updated_at,
            status: DraftStatus::Live,
            base_version,
        }
    }

    /// This draft's row identity (its `RowId`). Stable across rebases; how the store routes a
    /// canonical change to the drafts it affects.
    pub fn id(&self) -> NoteId {
        self.id
    }

    /// The current input-provided timestamp the draft would commit with.
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }

    /// Shared access to the title field (validity/sync queries), mirroring `NoteDraft`.
    pub fn title(&self) -> &Field<Title> {
        &self.title
    }

    /// Record a title input attempt. `Ok`/`Err` exactly as `Field::try_set` (frozen) reports it.
    pub fn try_set_title(&mut self, raw: String) -> Result<(), TitleError> {
        self.title.try_set(raw)
    }

    /// Provide a new timestamp as input (D35 — timestamps never come from an ambient clock).
    pub fn set_updated_at(&mut self, updated_at: i64) {
        self.updated_at = updated_at;
    }
}

impl Draft for NoteRowDraft {
    type Entity = NoteRow;
    type FieldId = RowField;

    fn status(&self) -> DraftStatus {
        self.status
    }

    fn base_version(&self) -> u64 {
        self.base_version
    }

    fn dirty_fields(&self) -> Vec<RowField> {
        let mut out = Vec::new();
        if self.title.is_dirty() {
            out.push(RowField::Title);
        }
        out
    }

    fn conflicts(&self) -> Vec<RowField> {
        let mut out = Vec::new();
        if self.title.is_conflicted() {
            out.push(RowField::Title);
        }
        out
    }

    fn validate(&self) -> ValidationReport<RowField> {
        let mut report = ValidationReport::new();
        if let Some(e) = self.title.required_error() {
            report.field_errors.push((RowField::Title, e));
        }
        report
    }

    fn resolve_keep_mine(&mut self, field: RowField) {
        match field {
            RowField::Title => self.title.resolve_keep_mine(),
        }
    }

    fn resolve_take_theirs(&mut self, field: RowField) {
        match field {
            RowField::Title => self.title.resolve_take_theirs(),
        }
    }

    /// The parse moment. The refusal precedence (`Orphaned → Conflicted → Validation`, C07) is the
    /// frozen [`commit_gates`], called verbatim — the collection **inherits** the ordering rather
    /// than re-deriving it. On success the draft yields a full `NoteRow`, carrying its identity and
    /// input-provided timestamp through unchanged.
    fn commit(self) -> Result<NoteRow, (Self, CommitError<RowField>)> {
        if let Some(err) = commit_gates(&self) {
            return Err((self, err));
        }
        match self.title.value().cloned() {
            Some(title) => Ok(NoteRow {
                id: self.id,
                title,
                updated_at: self.updated_at,
            }),
            None => {
                // Unreachable once the gates pass (a live, unconflicted, valid draft has a title),
                // but library code never panics: re-report as a validation refusal.
                let report = self.validate();
                Err((self, CommitError::Validation(report)))
            }
        }
    }
}

impl StoreDraft for NoteRowDraft {
    /// Build a checkout of `base`. The `None` branch is **identity-blind** — a degenerate create-flow
    /// draft with a zero id — because this frozen signature carries no `RowId`. The collection never
    /// routes create-flow through here; it uses [`CollectionStore::checkout_new`], which supplies the
    /// id as data. Recorded as the M1 friction on `StoreDraft`'s create-flow contract.
    fn from_canonical(base: Option<&NoteRow>, base_version: u64) -> Self {
        match base {
            Some(row) => NoteRowDraft {
                id: row.id,
                title: Field::from_base(row.title.clone()),
                updated_at: row.updated_at,
                status: DraftStatus::Live,
                base_version,
            },
            None => NoteRowDraft::new_create(NoteId(0), 0, base_version),
        }
    }

    /// Rebase onto a newer canonical row. The [`Field`] does the three-way merge (C19); `updated_at`
    /// adopts theirs (canonical metadata, never conflicted); `base_version` tracks the store (C15).
    /// An orphan is terminal — it does not rebase (C11).
    fn rebase(&mut self, entity: &NoteRow, version: u64) {
        if matches!(self.status, DraftStatus::Orphaned) {
            return;
        }
        self.title.rebase(entity.title.clone());
        self.updated_at = entity.updated_at;
        self.base_version = version;
    }

    fn orphan(&mut self) {
        self.status = DraftStatus::Orphaned;
    }

    /// Entity-backed iff any field retains a base. A create-flow draft's title is unset (no base), so
    /// this is `false` and the store never rebases or orphans it (C12) — the id it carries is *its
    /// own* future identity, not a base it was checked out from.
    fn is_based(&self) -> bool {
        self.title.base().is_some()
    }
}

/// Clamp a requested range to `[0, total]`, tail-first: an out-of-range start collapses to an
/// empty range at the tail rather than panicking or wrapping.
fn clamp_range(range: &Range<u32>, total: u32) -> Range<u32> {
    let start = range.start.min(total);
    let end = range.end.clamp(start, total);
    start..end
}

#[cfg(test)]
mod tests;
