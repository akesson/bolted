//! M0 window-semantics tests (W1–W6) and M1 row-draft tests (W7–W12). Each was watched red first
//! (see the step-28 report's watched-red ledger) before being restored to green.

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
    let w = store.open_window(Query::natural(), 0..2);

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
    let w = store.open_window(Query::natural(), 0..3);

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
    let w = store.open_window(Query::natural(), 1..10);

    let snap = store.latest(w).expect("live window");
    assert_eq!(snap.total_count, 3);
    assert_eq!(snap.range, 1..3, "clamped to the tail");
    assert_eq!(ids(&snap), vec![2, 1]);
}

/// W4 — two windows, independent ranges, one collection: both correct after one mutation.
#[test]
fn w4_two_windows_one_collection() {
    let mut store = seeded();
    let a = store.open_window(Query::natural(), 0..1);
    let b = store.open_window(Query::natural(), 1..3);

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
    let w = store.open_window(Query::natural(), 0..2);
    assert_eq!(store.live_window_count(), 1);

    store.close_window(w);
    assert_eq!(store.live_window_count(), 0, "the count says it is gone");
    assert!(store.latest(w).is_none(), "a closed handle is dead");

    // Idempotent: closing again is a no-op; a fresh handle can still be opened and closed.
    store.close_window(w);
    let w2 = store.open_window(Query::natural(), 0..1);
    store.close_window(w2);
    assert_eq!(store.live_window_count(), 0);
}

/// W6 — coalescing-by-construction: two mutations, one read; only the newest state is visible, and
/// the intermediate version was never observable.
#[test]
fn w6_coalescing_by_construction() {
    let mut store = CollectionStore::new();
    let w = store.open_window(Query::natural(), 0..10);

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

// =================================================================================================
// M1 — row drafts over the collection (W7–W12). Inheritance of the frozen draft/rebase/orphan
// machinery, proven with positive controls; never a re-invented conflict taxonomy.
// =================================================================================================

/// W7 — edit under sort movement: a canonical upsert moves the row's *index*, the draft is unmoved
/// (its edit survives the rebase), submit lands, and the next snapshot shows the **same `RowId`** at
/// its new position with the edit applied.
#[test]
fn w7_edit_under_sort_movement() {
    let mut store = seeded(); // [3@300, 2@200, 1@100] -> order [3, 2, 1]
    let w = store.open_window(Query::natural(), 0..3);
    let d = store.checkout(NoteId(1)).expect("row 1 exists");

    // Edit the draft's title (dirty).
    store
        .draft_mut(d)
        .expect("live")
        .try_set_title("z".to_string())
        .expect("valid");

    // A canonical upsert moves row 1 to the top (updated_at 100 -> 400); its title is unchanged.
    let affected = store.apply_canonical(CanonicalChange::Upsert(row(1, "a", 400)));
    assert_eq!(affected, vec![d], "the fan-out named the draft on row 1");

    // The draft is unmoved: the edit survived (theirs 'a' == base 'a' -> keep mine, still dirty).
    assert_eq!(
        store.draft(d).expect("live").dirty_fields(),
        vec![RowField::Title],
        "the edit survived the rebase"
    );

    // The row's INDEX moved (now first) though the canonical title is still 'a'.
    let mid = store.latest(w).expect("live window");
    assert_eq!(ids(&mid), vec![1, 3, 2], "same RowId, new (top) position");

    // Submit lands the edited title; there is no other draft on row 1, so the fan-out is empty.
    let out = store.submit(d).expect("submit lands");
    assert!(out.is_empty(), "no other draft on row 1");
    assert!(!store.is_live(d), "the submitted draft is released (C17)");

    // Next snapshot: row 1 at the top, title now 'z', same RowId.
    let after = store.latest(w).expect("live window");
    assert_eq!(ids(&after), vec![1, 3, 2]);
    assert_eq!(after.rows[0].id, NoteId(1));
    assert_eq!(
        after.rows[0].title.as_str(),
        "z",
        "the edit landed at the new position"
    );
}

/// W8 — the representative frozen conformance row: **C15** (the base version tracks the rebase, and
/// an orphan's stamp stops moving). Chosen because it is exactly what a *re-implemented* registry
/// loop is most likely to get wrong — it exercises the fan-out passing the right version, the
/// `rebases` gate skipping unaffected drafts, and orphan terminality — all inherited, not invented.
#[test]
fn w8_base_version_tracks_rebase_c15() {
    let mut store = seeded(); // version 3
    let d = store.checkout(NoteId(1)).expect("row 1");
    assert_eq!(
        store.draft(d).expect("live").base_version(),
        3,
        "checkout stamps the store version"
    );

    // A canonical change that rebases the draft advances its stamp to the store version.
    store.apply_canonical(CanonicalChange::Upsert(row(1, "a2", 150)));
    assert_eq!(store.version(), 4);
    assert_eq!(
        store.draft(d).expect("live").base_version(),
        4,
        "base_version tracks the rebase (C15)"
    );

    // A change to ANOTHER row does not re-stamp this draft (it was not rebased).
    store.apply_canonical(CanonicalChange::Upsert(row(2, "b2", 250)));
    assert_eq!(store.version(), 5);
    assert_eq!(
        store.draft(d).expect("live").base_version(),
        4,
        "an unaffected draft does not re-stamp"
    );

    // Deletion orphans; an orphan is based on no canonical and its stamp stops moving (C15).
    store.apply_canonical(CanonicalChange::Delete(NoteId(1)));
    assert_eq!(
        store.draft(d).expect("live").status(),
        DraftStatus::Orphaned
    );
    assert_eq!(
        store.draft(d).expect("live").base_version(),
        4,
        "the orphan's stamp froze at its last rebase"
    );

    // A later canonical event neither resurrects nor re-stamps it.
    store.apply_canonical(CanonicalChange::Upsert(row(1, "a3", 999)));
    assert_eq!(
        store.draft(d).expect("live").base_version(),
        4,
        "the orphan stamp stays frozen"
    );
    assert_eq!(
        store.draft(d).expect("live").status(),
        DraftStatus::Orphaned
    );
}

/// W9 — delete-under-draft orphans (C11), and submitting the orphan is a **typed** refusal that
/// hands the edit session back (F3).
#[test]
fn w9_delete_under_draft_orphans() {
    let mut store = seeded();
    let d = store.checkout(NoteId(2)).expect("row 2");
    store
        .draft_mut(d)
        .expect("live")
        .try_set_title("edit".to_string())
        .expect("valid");

    let affected = store.apply_canonical(CanonicalChange::Delete(NoteId(2)));
    assert_eq!(affected, vec![d], "the delete fan-out named the draft");
    assert_eq!(
        store.draft(d).expect("live").status(),
        DraftStatus::Orphaned
    );
    assert!(store.is_live(d), "orphaned but still a live handle (C11)");

    // Submit is a typed `Orphaned` refusal, never a silent failure or a resurrection.
    assert_eq!(store.submit(d), Err(SubmitError::Orphaned));
    assert!(
        store.is_live(d),
        "a refused submit keeps the edit session (F3)"
    );
}

/// W10 — precedence positive control (C07): a draft that is **both** conflicted and orphaned refuses
/// `Orphaned` first, because the collection's `submit` delegates to the frozen `commit_gates`.
#[test]
fn w10_precedence_orphaned_before_conflicted() {
    let mut store = seeded();
    let d = store.checkout(NoteId(1)).expect("row 1");

    // Make the title conflicted: edit it, then a canonical upsert carrying a DIFFERENT title.
    store
        .draft_mut(d)
        .expect("live")
        .try_set_title("mine".to_string())
        .expect("valid");
    store.apply_canonical(CanonicalChange::Upsert(row(1, "theirs", 100)));
    assert_eq!(
        store.draft(d).expect("live").conflicts(),
        vec![RowField::Title],
        "the field is now conflicted"
    );

    // Now delete the row: the draft is BOTH conflicted and orphaned.
    store.apply_canonical(CanonicalChange::Delete(NoteId(1)));
    let draft = store.draft(d).expect("live");
    assert_eq!(draft.status(), DraftStatus::Orphaned);
    assert_eq!(
        draft.conflicts(),
        vec![RowField::Title],
        "still conflicted under the orphan"
    );

    // C07: the refusal is `Orphaned` first, not `Conflicted`.
    assert_eq!(
        store.submit(d),
        Err(SubmitError::Orphaned),
        "Orphaned outranks Conflicted (C07)"
    );
}

/// W11 — create-flow (C12): a no-base draft is never moved by any canonical change and commits
/// normally, **inserting** a new row the windows then see. Identity is caller-supplied (a
/// client-generated key, D35) — the smallest reversible choice; see the report.
#[test]
fn w11_create_flow_inserts_c12() {
    let mut store = seeded(); // rows 1, 2, 3
    let w = store.open_window(Query::natural(), 0..10);

    // A create-flow row: the caller supplies the identity and the initial timestamp (both input).
    let d = store.checkout_new(NoteId(99), 500);
    assert_eq!(store.draft_count(), 1);
    assert_eq!(
        store.rebasing_draft_count(),
        0,
        "a create-flow draft never rebases (C12)"
    );

    // A canonical change to another row leaves it untouched — it is not in the fan-out.
    let a = store.apply_canonical(CanonicalChange::Upsert(row(2, "b2", 250)));
    assert!(a.is_empty(), "the create-flow draft is not rebased (C12)");
    assert_eq!(store.rebasing_draft_count(), 0);

    // Fill and submit: it inserts row 99; the window sees it with the typed title.
    store
        .draft_mut(d)
        .expect("live")
        .try_set_title("new note".to_string())
        .expect("valid");
    let out = store.submit(d).expect("create-flow commits normally (C12)");
    assert!(out.is_empty(), "no other draft on row 99");
    assert!(!store.is_live(d));

    let snap = store.latest(w).expect("live window");
    let created = snap
        .rows
        .iter()
        .find(|r| r.id == NoteId(99))
        .expect("row 99 is visible in the window");
    assert_eq!(created.title.as_str(), "new note", "the created row landed");
}

/// W12 — rebase fan-out names exactly the affected drafts: one upsert, two open drafts on the same
/// row (both named, in id order), and a third draft on another row (never named).
#[test]
fn w12_rebase_fan_out_names_exactly() {
    let mut store = seeded(); // rows 1, 2, 3
    let a = store.checkout(NoteId(1)).expect("row 1");
    let b = store.checkout(NoteId(1)).expect("row 1 again"); // a second edit session on row 1
    let c = store.checkout(NoteId(3)).expect("row 3"); // the control: must NOT be named

    let affected = store.apply_canonical(CanonicalChange::Upsert(row(1, "a2", 120)));
    assert_eq!(
        affected,
        vec![a, b],
        "exactly the two drafts on row 1, in id order"
    );
    assert!(
        !affected.contains(&c),
        "the draft on row 3 is not in the fan-out"
    );

    // And a change to row 3 names only c.
    let affected3 = store.apply_canonical(CanonicalChange::Upsert(row(3, "c2", 320)));
    assert_eq!(affected3, vec![c], "exactly the draft on row 3");
}

// =================================================================================================
// M2 — the per-window query handle (W13–W16) and the naive-re-projection perf probe (W17).
// `Query { sort, filter }` is per-window state; `set_query` is an ordinary input; `latest`
// projects filter-first, then sort, then clamp+slice, each window through its own query.
// =================================================================================================

/// A store whose rows' *recency* order and *title* order deliberately disagree, so the two windows
/// in W13 cannot be accidentally correct. Recency (`updated_at` desc): `[3, 2, 1]`
/// (cherry, apple, banana). Title asc: `[2, 1, 3]` (apple, banana, cherry).
fn seeded_named() -> CollectionStore {
    let mut store = CollectionStore::new();
    store.apply_canonical(CanonicalChange::Upsert(row(1, "banana", 100)));
    store.apply_canonical(CanonicalChange::Upsert(row(2, "apple", 200)));
    store.apply_canonical(CanonicalChange::Upsert(row(3, "cherry", 300)));
    store
}

/// W13 — the exploration pair: a "tray" window (recency, `0..5`) and a "main" window (by title,
/// `0..50`) over **one** collection. One mutation updates **both** correctly, each through its own
/// query, off the same collection version.
#[test]
fn w13_exploration_pair_two_orders_one_collection() {
    let mut store = seeded_named();
    let tray = store.open_window(
        Query {
            sort: Sort::UpdatedDesc,
            filter: None,
        },
        0..5,
    );
    let main = store.open_window(
        Query {
            sort: Sort::TitleAsc,
            filter: None,
        },
        0..50,
    );

    assert_eq!(
        ids(&store.latest(tray).expect("tray")),
        vec![3, 2, 1],
        "recency order"
    );
    assert_eq!(
        ids(&store.latest(main).expect("main")),
        vec![2, 1, 3],
        "title order"
    );

    // One mutation: a brand-new row that lands at the top of recency but the *second* slot by title.
    store.apply_canonical(CanonicalChange::Upsert(row(4, "avocado", 400)));

    let t = store.latest(tray).expect("tray");
    let m = store.latest(main).expect("main");
    assert_eq!(ids(&t), vec![4, 3, 2, 1], "recency: newest on top");
    assert_eq!(
        ids(&m),
        vec![2, 4, 1, 3],
        "title: apple, avocado, banana, cherry"
    );
    assert_eq!(t.version, m.version, "one collection, one version");
    assert_eq!(t.total_count, 4);
    assert_eq!(m.total_count, 4);
}

/// W14 — query change: after `set_query` returns, the very next snapshot is for the **new** query,
/// with the range re-clamped against the new projection. A stale-query snapshot is never observable
/// (construction is synchronous — see the report's single-flight finding).
#[test]
fn w14_query_change_next_snapshot_is_new_query() {
    let mut store = seeded_named(); // recency [3,2,1], titles apple(2)/banana(1)/cherry(3)
    let w = store.open_window(
        Query {
            sort: Sort::UpdatedDesc,
            filter: None,
        },
        0..3,
    );

    let before = store.latest(w).expect("live");
    assert_eq!(ids(&before), vec![3, 2, 1], "old query: recency, all three");
    assert_eq!(before.range, 0..3);
    assert_eq!(before.total_count, 3);

    // Switch to title order AND a filter that narrows to the two rows containing 'a'.
    // (banana, apple contain 'a'; cherry does not.)
    store.set_query(
        w,
        Query {
            sort: Sort::TitleAsc,
            filter: Some("a".to_string()),
        },
    );

    // The very next pull is entirely the new query: title order, filtered, range re-clamped.
    let after = store.latest(w).expect("live");
    assert_eq!(
        ids(&after),
        vec![2, 1],
        "new query: title order, only 'a' rows (apple, banana)"
    );
    assert_eq!(after.total_count, 2, "filtered projection length");
    assert_eq!(
        after.range,
        0..2,
        "0..3 re-clamped against the narrower projection"
    );
    assert_ne!(ids(&before), ids(&after), "no stale-query snapshot lingers");
}

/// W15 — filter narrows: `total_count` under a filter is the **filtered** count (the implemented
/// choice), not the whole-collection count. The recorded fork (whole-collection count) is in the
/// report; this test pins the choice the code makes.
#[test]
fn w15_filter_narrows_total_count_is_filtered() {
    let mut store = CollectionStore::new();
    store.apply_canonical(CanonicalChange::Upsert(row(1, "apple", 100)));
    store.apply_canonical(CanonicalChange::Upsert(row(2, "apricot", 200)));
    store.apply_canonical(CanonicalChange::Upsert(row(3, "banana", 300)));
    store.apply_canonical(CanonicalChange::Upsert(row(4, "cherry", 400)));

    // Whole collection is 4 rows; two titles contain "ap".
    assert_eq!(
        store.total_count(),
        4,
        "the collection itself holds four rows"
    );

    let w = store.open_window(
        Query {
            sort: Sort::UpdatedDesc,
            filter: Some("ap".to_string()),
        },
        0..50,
    );
    let snap = store.latest(w).expect("live");

    assert_eq!(
        ids(&snap),
        vec![2, 1],
        "apricot@200, apple@100 — the two 'ap' rows, recency order"
    );
    assert_eq!(
        snap.total_count, 2,
        "total_count is the FILTERED count (implemented choice), not the collection's 4"
    );
    assert_eq!(snap.range, 0..2, "range clamps against the filtered length");
}

/// W16 — a filtered-out, checked-out row: a draft on a row the window's filter hides keeps working,
/// its submit lands, and the row **stays out** of the filtered window (the filter is a window
/// concern; checkout/submit are collection concerns and never consult a window's filter). A second,
/// unfiltered window proves the edit really landed.
#[test]
fn w16_filtered_out_checked_out_row() {
    let mut store = CollectionStore::new();
    store.apply_canonical(CanonicalChange::Upsert(row(1, "apple", 100)));
    store.apply_canonical(CanonicalChange::Upsert(row(2, "banana", 200)));
    store.apply_canonical(CanonicalChange::Upsert(row(3, "cherry", 300)));

    // A window filtered to titles containing "apple" — it shows only row 1; rows 2 and 3 are hidden.
    let filtered = store.open_window(
        Query {
            sort: Sort::UpdatedDesc,
            filter: Some("apple".to_string()),
        },
        0..50,
    );
    let unfiltered = store.open_window(
        Query {
            sort: Sort::UpdatedDesc,
            filter: None,
        },
        0..50,
    );
    assert_eq!(
        ids(&store.latest(filtered).expect("live")),
        vec![1],
        "only 'apple' is visible"
    );

    // Check out row 2 (banana) — hidden by the filter, but checkout is a collection concern.
    let d = store
        .checkout(NoteId(2))
        .expect("row 2 exists in the collection, filter notwithstanding");
    store
        .draft_mut(d)
        .expect("live")
        .try_set_title("blueberry".to_string())
        .expect("valid, and still contains no 'apple'");

    // Submit lands the edit through the canonical path.
    let out = store.submit(d).expect("submit lands");
    assert!(out.is_empty(), "no other draft on row 2");
    assert!(!store.is_live(d), "the draft is released (C17)");

    // The filtered window STILL shows only row 1 — the edited-but-still-hidden row stays out.
    assert_eq!(
        ids(&store.latest(filtered).expect("live")),
        vec![1],
        "the edited row is still filtered out"
    );
    // The unfiltered window proves the edit truly landed on the canonical row.
    let full = store.latest(unfiltered).expect("live");
    let edited = full
        .rows
        .iter()
        .find(|r| r.id == NoteId(2))
        .expect("row 2 present unfiltered");
    assert_eq!(
        edited.title.as_str(),
        "blueberry",
        "the submit landed on the canonical row"
    );
}

/// W17 — the naive-re-projection perf probe. **Not an assertion** (`#[ignore]`d): 10k rows, 4 open
/// windows with mixed queries, 1k mutations, measuring the per-mutation cost of naive full
/// re-projection (one `apply_canonical` + a `latest` pull on all four windows). Reports p50/p99.
///
/// Run on demand (release, so the numbers reflect real cost), capturing stdout:
///
/// ```text
/// cargo test -p collection-core --release -- --ignored --nocapture w17_perf
/// ```
///
/// It is `#[ignore]`d so `mise run test` / `mise run check` compile and clippy-lint it (keeping it
/// honest) without paying its cost on every suite run. `Instant` is a measurement harness, not core
/// code (D35 binds the core, not the probe).
#[test]
#[ignore = "W17 perf probe — run on demand: cargo test -p collection-core --release -- --ignored --nocapture w17_perf"]
// The workspace bans `Instant::now` (ambient time breaks replay). This is the one place it is
// legitimate: a measurement harness, not core code — D35 binds the core, not the probe.
#[allow(clippy::disallowed_methods)]
fn w17_perf_probe_naive_reprojection() {
    use std::time::Instant;

    const ROWS: u64 = 10_000;
    const MUTATIONS: u64 = 1_000;

    let mut store = CollectionStore::new();
    for i in 0..ROWS {
        let title = format!("note {:05}", i);
        let updated_at = ((i.wrapping_mul(2_654_435_761)) % 1_000_000) as i64;
        store.apply_canonical(CanonicalChange::Upsert(NoteRow {
            id: NoteId(i),
            title: Title::try_new(title).expect("literal-shaped title is valid"),
            updated_at,
        }));
    }

    // Four windows, mixed queries: two full sorts (no filter) and two filtered projections.
    let windows = [
        store.open_window(
            Query {
                sort: Sort::UpdatedDesc,
                filter: None,
            },
            0..50,
        ),
        store.open_window(
            Query {
                sort: Sort::TitleAsc,
                filter: None,
            },
            0..50,
        ),
        store.open_window(
            Query {
                sort: Sort::UpdatedDesc,
                filter: Some("23".to_string()),
            },
            0..50,
        ),
        store.open_window(
            Query {
                sort: Sort::TitleAsc,
                filter: Some("note 09".to_string()),
            },
            0..50,
        ),
    ];

    let mut timings: Vec<std::time::Duration> = Vec::with_capacity(MUTATIONS as usize);
    for m in 0..MUTATIONS {
        let id = m % ROWS;
        let mutated = NoteRow {
            id: NoteId(id),
            title: Title::try_new(format!("note {:05}", id)).expect("valid"),
            updated_at: 1_000_000 + m as i64,
        };
        let start = Instant::now();
        store.apply_canonical(CanonicalChange::Upsert(mutated));
        for &w in &windows {
            let snap = store.latest(w).expect("live window");
            std::hint::black_box(&snap);
        }
        timings.push(start.elapsed());
    }

    timings.sort();
    let p50 = timings[timings.len() / 2];
    let p99 = timings[(timings.len() * 99 / 100).min(timings.len() - 1)];
    let total: std::time::Duration = timings.iter().sum();
    println!(
        "W17 naive re-projection: {ROWS} rows, {} windows, {} mutations (apply + {}x latest each)",
        windows.len(),
        timings.len(),
        windows.len(),
    );
    println!(
        "  p50 = {p50:?}/mutation   p99 = {p99:?}/mutation   mean = {:?}",
        total / timings.len() as u32
    );
}
