# spikes/collection — the collection-facet spike (windowed collections)

Disposable probe for step 28. Every Bolted facet so far exports **one** canonical entity; this
spike builds the first facet whose store owns **many** canonical rows, observed through
**windows**. Charter, terrain, kill criteria and the W-ID test ledger:
`docs/steps/step-28-collection-facet.md`; the ARCHITECTURE §9 item it banks evidence for is the
windowed-collections bullet (v1.17, 2026-07-23).

## What this campaign falsifies

ARCHITECTURE §9 gated windowed collections on "the first real collection facet". This spike is
that facet, deliberately inbox-shaped (sync-driven mutation *and* editable rows *and* two
differently-ordered observers — a read-only top-N would validate almost nothing). It gathers the
evidence for the four v1.17 questions:

1. **The store's canonical shape** — what a many-canonical store actually is, and how much of
   `bolted-core`'s per-entity machinery it reuses *unmodified*.
2. **Sort/filter as per-window query state** — two observers, two orders, over one collection.
3. **The `total_count` shape** — known vs lower bound; its semantics under a filter.
4. **Re-projection etiquette** — the per-window compute cost of the naive implementation.

The frozen `bolted-core` surface (§1–§7, C01–C23) is **law to inherit, not clay to reshape**: if
the collection needs frozen machinery to change, that is a kill criterion (report it), never a
workaround.

## Layout

- `crates/collection-core` — the spike core. M0: `CollectionStore` owning many canonical
  `NoteRow`s keyed by entity id; `apply_canonical` (upsert/delete) as the only mutation; a
  collection `version` that moves on every change; `open_window`/`set_range`/`latest`/
  `close_window` over a fixed natural order (`updated_at` desc, id tiebreak). M1 adds row drafts
  (draft-machinery inheritance); M2 adds the per-window query handle. Pure Rust, host-only, zero
  FFI.

The spike crate is a workspace member so `mise run check` compiles, clippys and tests it like
everything else. `check` gains no new external requirement from this directory.

## Disposal criteria

Everything here exists to be learned from, then deleted: **one `rm -rf spikes/collection` plus
removing one line from the root `Cargo.toml` members list** must be a clean exit at any time.
Findings land in `docs/steps/step-28-report.md` and, after the design pass, in ARCHITECTURE —
never by this code becoming load-bearing. If anything under this directory acquires a dependent
outside it, that is a finding to report, not a state to accept.

Delete after: the windowed-collections design session has resolved §9's collection-observation
item into D-decisions, and any code worth keeping has been **re-derived** in framework crates
through a normal step (evidence first, extraction later).
