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
//! ## What is reused unmodified
//!
//! [`Title`] is a spike-local [`bolted_core::Value`] — the tier-1 constrained value type, reused
//! byte-for-byte as `fixture-note` writes it. M0 touches none of the draft/rebase/orphan machinery
//! (that is M1's inheritance question); a canonical `NoteRow` is plain owned data.
#![forbid(unsafe_code)]

use bolted_core::{Constraint, ErrorData, Value};
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
    pub fn apply_canonical(&mut self, change: CanonicalChange) {
        self.version += 1;
        match change {
            CanonicalChange::Upsert(row) => {
                self.rows.insert(row.id, row);
            }
            CanonicalChange::Delete(id) => {
                self.rows.remove(&id);
            }
        }
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

    /// The fixed natural order: `updated_at` descending, `id` ascending as the tiebreak. Naive
    /// full sort on every read — the etiquette M2 prices.
    fn natural_order(&self) -> Vec<&NoteRow> {
        let mut rows: Vec<&NoteRow> = self.rows.values().collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        rows
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
