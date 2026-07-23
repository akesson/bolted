# Step 28 — Collection-facet spike: the first real collection facet

**Status: ready.** Authored by the windowed-collections exploration pass (ARCHITECTURE
**v1.17**, 2026-07-23). §9's windowed-collections item has always been gated on "the first
real collection facet" — this step builds it, as a disposable spike, and banks the evidence.
**The ruling does not happen here**: the step's output is a report whose findings are the
agenda for the windowed-collections design session. Runs independently of step 27 (disjoint
crates; both branch from main).

## What this is, and is not

Every facet so far exports one canonical entity. This spike builds a **notes inbox**: a facet
whose store owns *many* canonical rows, observed through **windows** — because collections
never cross the boundary whole. It is deliberately inbox-shaped (sync-driven mutation *and*
editable rows *and* two differently-ordered observers) because a read-only top-N would
validate almost nothing.

It is a **spike**, in the Phase-1/Phase-5 sense: hand-written, disposable
(`spikes/collection/`), zero changes to any frozen crate. The frozen `bolted-core` surface
(§1–§7, C01–C23) is **law to inherit, not clay to reshape** — if the collection needs frozen
machinery to change, that is a kill criterion, not a workaround.

## Fixed points (decided; do not re-litigate)

- **Snapshot-authoritative, watch-shaped per window (D37).** A window's observer reads
  `WindowSnapshot { version, total_count, range, rows }`; newest wins; coalescing is legal.
  **Deltas never cross the boundary** — no `VecDiff`, no insert/remove events. Shells diff
  consecutive snapshots by `RowId`; that is their concern, not the core's.
- **`RowId` is identity, never index.** Candidate (confirm or refute): the entity key.
- **Scroll anchoring is shell-side.** The core is honest and index-based: after an
  insert-above, the same range shows shifted content. Viewport compensation is native view
  state (D36 governs the scroll side; no frame-rate inputs cross into the core).
- **Editing a row is `checkout(row_id)`** — observe/command/draft is unchanged. Deletion
  under a live draft is **already ruled**: C11 orphans, C07 orders the refusal
  (`Orphaned → Conflicted → Validation`), C12 governs create-flow. The spike *inherits* these
  and proves the inheritance with positive controls; it never invents a structural-conflict
  taxonomy.
- **No ambient nondeterminism (D35).** Timestamps (`updated_at`) enter as inputs. No
  `SystemTime::now()` in spike-core code.
- **Windows are per-observer core-side handles** (the draft pattern on the read side), with
  an explicit `close` as the only release path and a live-window count answerable by the
  store (the C22 discipline).

## Starting hypothesis (the candidate to confirm, sharpen, or refute — not a spec)

```rust
// spike-local; names are sketches, final naming is smallest-reversible territory
fn open_window(&mut self, query: Query, range: Range<u32>) -> WindowId;
fn set_range(&mut self, w: WindowId, range: Range<u32>);   // ordinary input
fn set_query(&mut self, w: WindowId, query: Query);        // ordinary input (M2)
fn latest(&self, w: WindowId) -> WindowSnapshot<Row>;      // pull + change tick (§8 idiom)
fn close_window(&mut self, w: WindowId);

pub struct WindowSnapshot<Row> {
    version: u64,        // collection version
    total_count: u32,    // NOTE: its semantics under filter is an open finding to record
    range: Range<u32>,   // may be clamped by the core
    rows: Vec<Row>,      // read-only projections carrying stable RowIds
}
```

The v1.17 questions this spike owes evidence on:

1. **The store's canonical shape** — what a many-canonical store actually is, and how much
   of `bolted-core`'s per-entity machinery (fields, drafts, rebase, orphaning) it reuses
   *unmodified*.
2. **Sort/filter as per-window query state** — two observers, two orders, over one
   collection; `set_query` as an ordinary replay-visible input; stale-query behavior.
3. **The `total_count` shape** — known vs lower bound; and (found while authoring) its
   semantics under a filter: filtered count or collection count? Record, don't rule.
4. **Re-projection etiquette** — the per-window compute cost of the naive implementation,
   measured, with D37 coalescing as the safety valve.

## Scope

New, disposable: `spikes/collection/crates/collection-core` (add to workspace members with a
comment block naming disposal, like the os-integration block; `spikes/collection/README.md`
states the disposal criteria). Dependencies: `bolted-core` (unmodified) and, where it fits,
`fixture-note` as a *style* reference — the spike's entity is spike-local (default: `NoteRow`
with entity-key id, a `Title`-style constrained value, `updated_at` as an input-provided
timestamp). Pure Rust, host-only. Everything else in the repo: untouched.

Rules as always: edition 2024; clippy `-D warnings`; no `unwrap`/`expect`/`panic!` in
non-test code; build/test **only** via `mise run check` / `mise run test`, run synchronously
in the foreground. Every semantic claim's test is watched red first (break the code or the
assertion deliberately, see the red, restore green — record each in the report).

## Milestones (one Opus sub-agent each; Fable reviews between)

### M0 — the many-canonical store + window semantics

`CollectionStore` owning canonical rows keyed by entity id; `apply_canonical`
upsert/delete (the canonical-source path — the only collection mutation in M0); a collection
`version` that moves on every change; `open_window`/`set_range`/`latest`/`close_window` with
a fixed natural order (`updated_at` desc, id tiebreak — queries arrive in M2).

Tests (W-IDs are for the report): **W1** insert-above-window — same range, shifted content,
`version`/`total_count` moved, snapshot honest. **W2** delete-in-window. **W3** range clamped
at the tail. **W4** two windows, independent ranges, one collection — both correct after one
mutation. **W5** `close_window` releases; the live-window count says so (C22 analog).
**W6** coalescing-by-construction — two mutations, one read, only the newest state visible.

### M1 — drafts over the collection (inheritance, proven, not invented)

`checkout(row_id)` reusing `bolted-core`'s draft machinery per row; rebase fan-out stays
returned data (`Vec<DraftId>`).

Tests: **W7** edit under sort movement — canonical upsert moves the row's index, the draft is
unmoved, submit lands, the next snapshot shows the row at its new position with the same
`RowId`. **W8** rebase/conflict on a row draft behaves exactly as the frozen per-entity rules
say (pick one representative C-row and mirror it). **W9** delete-under-draft → `Orphaned`,
submit refusal typed. **W10** precedence positive control — a draft both conflicted *and*
orphaned refuses `Orphaned` first (C07). **W11** create-flow (C12) — a no-base draft is never
rebased, submit **inserts** a new row, and the windows see it. **W12** rebase fan-out — one
`apply_canonical` upsert, two open row-drafts, the returned set names exactly the affected
drafts.

### M2 — the per-window query handle + the price of naivety

`Query { sort: UpdatedDesc | TitleAsc, filter: Option<substring-on-title> }` per window;
`set_query` an ordinary input.

Tests: **W13** the exploration pair — a "tray" window (`0..5`, recency) and a "main" window
(by-title, `0..50`) over one collection; one mutation updates both correctly. **W14** query
change — next snapshot is for the new query, range re-clamped; a stale-query snapshot is
never observable after `set_query` returns. If sync-only construction makes single-flight
machinery moot, **record that as a finding** (the async case is the design session's
problem), don't build machinery without a consumer. **W15** filter narrows —
`total_count` semantics under filter: implement one choice, record it as an open question
with the argument for each side. **W16** filtered-out checked-out row — a draft on a row the
filter hides keeps working; submit lands; the row stays out of the filtered window.
**W17 (perf probe, reported not asserted)** 10k rows, 4 open windows (mixed queries), 1k
mutations: p50/p99 per-mutation cost of naive full re-projection, host machine, numbers in
the report.

## Kill criteria (real: stop and report, do not work around)

1. **Watch-shape breaks.** Any scenario above provably requires delta delivery (an observer
   is incorrect under legal coalescing). This contradicts D37's premise for collections —
   the design session must hear it before anything else is built.
2. **Inheritance breaks.** Reusing the draft/rebase/orphan machinery over many canonicals
   requires modifying any frozen crate's surface. That is the structural question answered
   in the worst way — bank the exact friction, stop.
3. **The naive price is absurd.** W17's p50 exceeds ~1 ms per mutation at spike scale.
   Don't optimize into an ad-hoc design — the etiquette question needs the honest number.

## Non-goals

Paging/hydration (rows the store doesn't hold), FFI crossing and bindings, shells/UI, macro
stamping, the wire topology, persistence, text-search beyond substring, collection *ordering*
as a user-editable property (manual reorder is a command-verb question for later), and any
ARCHITECTURE/GLOSSARY edit — §9 stays untouched until the design session.

## Report (`docs/steps/step-28-report.md`) + ROADMAP row update

Built / deviations / friction log / open questions, as always — plus a **§Findings keyed to
the four v1.17 questions** (store shape, query handle, `total_count`, etiquette — each:
what the code says, what remains genuinely open), the W-ID ledger with its watched-red
record, W17's numbers, and a proposed agenda for the windowed-collections design session.
