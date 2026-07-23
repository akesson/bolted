//! M0 window-semantics tests (W1–W6). Each was watched red first (see the step-28 report's
//! watched-red ledger) before being restored to green.

use super::*;

/// A canonical row. `expect` is fine here — test code, and a hard-coded literal that is provably
/// within `Title`'s constraints.
fn row(id: u64, title: &str, updated_at: i64) -> NoteRow {
    NoteRow {
        id: NoteId(id),
        title: Title::try_new(title.to_string()).expect("literal is a valid title"),
        updated_at,
    }
}

/// A store seeded with three rows whose natural order (`updated_at` desc, id asc) is `[3, 2, 1]`.
fn seeded() -> CollectionStore {
    let mut store = CollectionStore::new();
    store.apply_canonical(CanonicalChange::Upsert(row(1, "a", 100)));
    store.apply_canonical(CanonicalChange::Upsert(row(2, "b", 200)));
    store.apply_canonical(CanonicalChange::Upsert(row(3, "c", 300)));
    store
}

fn ids(snap: &WindowSnapshot) -> Vec<u64> {
    snap.rows.iter().map(|r| r.id.0).collect()
}

/// W1 — insert-above-window: the same range shows shifted content, `version`/`total_count` moved,
/// the snapshot stays honest (index-based, no scroll anchoring — that is shell-side).
#[test]
fn w1_insert_above_window_shifts_content() {
    let mut store = seeded();
    let w = store.open_window(0..2);

    let before = store.latest(w).expect("live window");
    assert_eq!(before.version, 3);
    assert_eq!(before.total_count, 3);
    assert_eq!(before.range, 0..2);
    assert_eq!(ids(&before), vec![3, 2]);

    // A newer row sorts above the whole window.
    store.apply_canonical(CanonicalChange::Upsert(row(4, "d", 400)));

    let after = store.latest(w).expect("live window");
    assert_eq!(after.version, 4, "version moved");
    assert_eq!(after.total_count, 4, "total_count moved");
    assert_eq!(after.range, 0..2, "same requested range");
    assert_eq!(ids(&after), vec![4, 3], "same range, shifted content");
}

/// W2 — delete-in-window: the deleted row leaves the snapshot, the count drops, the version moves.
#[test]
fn w2_delete_in_window() {
    let mut store = seeded();
    let w = store.open_window(0..3);

    let before = store.latest(w).expect("live window");
    assert_eq!(ids(&before), vec![3, 2, 1]);

    store.apply_canonical(CanonicalChange::Delete(NoteId(2)));

    let after = store.latest(w).expect("live window");
    assert_eq!(after.version, 4);
    assert_eq!(after.total_count, 2);
    assert_eq!(ids(&after), vec![3, 1], "id 2 is gone");
}

/// W3 — range clamped at the tail: a range past the end collapses to the collection's tail rather
/// than reading out of bounds.
#[test]
fn w3_range_clamped_at_tail() {
    let mut store = seeded();
    let w = store.open_window(1..10);

    let snap = store.latest(w).expect("live window");
    assert_eq!(snap.total_count, 3);
    assert_eq!(snap.range, 1..3, "clamped to the tail");
    assert_eq!(ids(&snap), vec![2, 1]);
}

/// W4 — two windows, independent ranges, one collection: both correct after one mutation.
#[test]
fn w4_two_windows_one_collection() {
    let mut store = seeded();
    let a = store.open_window(0..1);
    let b = store.open_window(1..3);

    assert_eq!(ids(&store.latest(a).expect("a")), vec![3]);
    assert_eq!(ids(&store.latest(b).expect("b")), vec![2, 1]);

    store.apply_canonical(CanonicalChange::Upsert(row(4, "d", 400)));

    // Natural order is now [4, 3, 2, 1]; each window re-projects its own range.
    let sa = store.latest(a).expect("a");
    let sb = store.latest(b).expect("b");
    assert_eq!(ids(&sa), vec![4], "window a: top 1");
    assert_eq!(ids(&sb), vec![3, 2], "window b: next two");
    assert_eq!(sa.version, sb.version, "one collection, one version");
    assert_eq!(sa.total_count, 4);
    assert_eq!(sb.total_count, 4);
}

/// W5 — `close_window` releases; the live-window count says so (the C22 analog), and `latest`
/// on a closed handle returns `None`.
#[test]
fn w5_close_window_releases() {
    let mut store = seeded();
    let w = store.open_window(0..2);
    assert_eq!(store.live_window_count(), 1);

    store.close_window(w);
    assert_eq!(store.live_window_count(), 0, "the count says it is gone");
    assert!(store.latest(w).is_none(), "a closed handle is dead");

    // Idempotent: closing again is a no-op; a fresh handle can still be opened and closed.
    store.close_window(w);
    let w2 = store.open_window(0..1);
    store.close_window(w2);
    assert_eq!(store.live_window_count(), 0);
}

/// W6 — coalescing-by-construction: two mutations, one read; only the newest state is visible, and
/// the intermediate version was never observable.
#[test]
fn w6_coalescing_by_construction() {
    let mut store = CollectionStore::new();
    let w = store.open_window(0..10);

    store.apply_canonical(CanonicalChange::Upsert(row(1, "a", 100)));
    store.apply_canonical(CanonicalChange::Upsert(row(2, "b", 200)));

    // A single pull after both mutations sees only the newest state.
    let snap = store.latest(w).expect("live window");
    assert_eq!(
        snap.version, 2,
        "the newest version, not the intermediate 1"
    );
    assert_eq!(snap.total_count, 2);
    assert_eq!(ids(&snap), vec![2, 1], "both mutations, coalesced");
}
