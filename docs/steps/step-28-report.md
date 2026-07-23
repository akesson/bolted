# Step 28 — Report: the collection-facet spike

**Status: done.** All three milestones shipped; 17 W-rows (16 tests + the `#[ignore]`d W17
probe), every one watched red first in production code; **zero changes to any frozen crate**;
**no kill criteria hit**. Implementation ran as three Opus sub-agent milestones with
orchestrator review, independent re-verification (`mise run check` re-run after each
milestone; W17 re-run independently), and a commit between each. The spike is
disposal-eligible once the windowed-collections design session has folded its findings into
ARCHITECTURE (`rm -rf spikes/collection` + one workspace member line).

## Built

`spikes/collection/crates/collection-core` (workspace member; `spikes/collection/README.md`
carries the charter and disposal criteria):

- **M0 — the many-canonical store + windows.** `CollectionStore`: canonical `NoteRow`s in a
  `BTreeMap<NoteId, NoteRow>` + a version tick; `apply_canonical` (upsert/delete) as the only
  mutation; per-observer windows (`open_window` / `set_range` / `latest` / `close_window`,
  live-window count as the C22 analog); `WindowSnapshot { version, total_count, range
  (tail-clamped), rows }`, pull-based, snapshot-authoritative, **no delta or event delivery of
  any kind** (D37). Tier-1 `Value` layer (`Title`) reused byte-for-byte. W1–W6.
- **M1 — row drafts.** `NoteRowDraft`, a hand-written `Draft` + `StoreDraft` implementor
  mirroring `fixture-note`; a draft registry on `CollectionStore` (`checkout`, `checkout_new`,
  `submit`, `close`, the C22 count pair); rebase fan-out returned as data
  (`Vec<RowDraftId>`), never a callback. W7–W12.
- **M2 — the per-window query handle.** `Query { sort: UpdatedDesc | TitleAsc, filter:
  Option<substring-on-title> }` carried per window; `set_query` an ordinary input beside
  `set_range`; projection = filter → sort → clamp + slice. W13–W16 + the W17 perf probe
  (re-run: `cargo test -p collection-core --release -- --ignored --nocapture w17_perf`).

## Findings, keyed to the four v1.17 questions

### 1. The store's canonical shape — **a peer container, not a `Store<D>` wrapper**

The machinery inherits; the host does not.

- **Reused byte-for-byte, unmodified:** `Field<V>` (the entire three-way merge, value-based
  dirty, conflict/resolve, rebase), `Value`/`Constraint`/`ErrorData`, the `Draft` and
  `StoreDraft` traits (fully implemented), **`commit_gates`** — so C07's
  `Orphaned → Conflicted → Validation` precedence is *inherited*, not re-derived (W10 proved
  it by hand-rolling the gates in the wrong order and watching the red), `CommitError` /
  `SubmitError` / `DraftStatus` / `ValidationReport`.
- **Not reusable: `Store<D>` the struct.** It binds exactly one `canonical: Option<Entity>`.
  Per-row `Store`s were rejected on the evidence: every canonical row would be held twice
  (once for windows, once per store — the F7 "two facts" smell), the `DraftId` namespace
  would fracture across stores, and no store could supply a create-flow id. The collection
  re-implements `Store`'s thin registry loop (`adopt` / fan-out / `submit`) with **exactly
  one structural change**: the canonical is looked up by `RowId`
  (`self.rows.get(&draft.id())`) instead of the single `self.canonical`. Everything below
  that line — the `rebases` gate, rebase-on-adopt idempotence (C19), orphan-if-deleted
  (C11), submit-hands-the-draft-back — is `Store`'s logic unchanged.
- **The sharpest frozen-surface signal: `StoreDraft::from_canonical` is identity-blind in
  create-flow.** A single-entity store *is* the identity; a collection row needs its `RowId`
  before any canonical exists. The spike routes around it (`checkout_new(id, updated_at)`,
  a client-generated key as input — D35-consistent, one-function reversible); whether the
  frozen create-flow contract should carry identity is the design session's.

Inheritance proven with positive controls: delete-under-draft orphans (W9), C07 precedence
(W10), create-flow never rebases and submit *inserts* (W11), fan-out names exactly the
affected drafts (W12), base-version tracks rebase — C15 mirrored as the representative
frozen row (W8).

### 2. Sort/filter as per-window query state — **confirmed; single-flight moot in a sync core**

The exploration pair works as argued: a tray window (`0..5`, recency) and a main window
(by-title) over one collection, one mutation updating both (W13). `set_query` is an ordinary
input; the next `latest()` already projects through the new query, so *"a stale-query
snapshot is never observable"* holds **by construction** — there is no in-flight projection
to cancel in a pull-based sync core, and no single-flight machinery was built (W14, recorded
as a finding: the async-projection contract, with generation-stamping, is real but has no
consumer here). A filtered-out row remains a first-class collection citizen: checkout, edit,
and submit on a hidden row all land; the filtered window honestly keeps it out (W16).
Whether the query surface is a closed enum (macro-stampable) or a richer predicate was
deliberately not answered.

### 3. `total_count` — **implemented filtered-count; the fork is recorded, not ruled**

Implemented: `total_count` = the length of *this window's* projection (W15). For a single
field this is the only internally consistent choice — `range` clamps against and pages
through exactly the projected list, so "is there more below?" (`range.end < total_count`)
stays truthful. The recorded counter-argument: "how big is the collection" is a stable fact a
UI wants ("342 notes · 5 match"), which suggests the honest shape is **two** fields
(`collection_count` + `matched_count`), not one overloaded one. Known-vs-lower-bound
(paging) was untouched by design and remains open, unforeclosed.

### 4. Re-projection etiquette — **the naive price at spike scale: ~0.65–0.69 ms p50**

10,000 rows × 4 open windows (mixed queries) × 1,000 mutations, each mutation followed by a
`latest()` pull on all four windows, full re-sort from scratch each time. Apple M4 Pro,
release: **p50 ≈ 643–689 µs, p99 ≈ 741–808 µs per mutation** across three runs (two by the
implementer, one independent re-run). **Kill criterion 3 (~1 ms p50) not hit.** The number
says headroom, not that the shape scales: cost is O(windows × n log n) per pull, unshared
across windows and mutations, with D37 coalescing as the only relief (W6 shows one pull
coalescing any number of prior mutations for free).

## Deviations from the step doc

None of substance. `latest()` returns `Option` (a closed handle needs an honest dead
signal — the sketch had a bare snapshot); W17 is an `#[ignore]`d in-suite test rather than a
probe binary (single lib target, still compiled and clippy'd by `check`; run command on the
test).

## Decisions taken where the doc was silent (all smallest-reversible, all recorded)

1. `apply_canonical` takes one `CanonicalChange` enum (upsert/delete), not two methods.
2. Version bumps **per canonical event**, unconditionally — a delete of an absent id still
   ticks (mirrors frozen `Store`; "net-change-only" is a recorded alternative).
3. `checkout(row_id)` returns `Option` — an absent row has nothing to edit (a single-entity
   `checkout` is infallible only because it falls back to create-flow).
4. Create-flow identity = input-provided client key (`checkout_new(id, updated_at)`).
   Rejected: store-allocated provisional id (two identities for one row, needs
   reconciliation); canonical-assigned id with placeholder (unwindowable, undiffable).
5. `checkout_new` on a colliding id upsert-overwrites (spike simplification; a real create
   would likely reject).
6. `updated_at` is carried non-field data that adopts-theirs on every rebase, never
   conflicts — clean here, but an ordering key a *user* can edit would be a `Field` needing
   the three-way merge (recorded).
7. Filter = case-sensitive substring `contains`; `TitleAsc` = byte order; both id-tiebroken
   so every query is a total order (deterministic naive re-projection).
8. The W17 probe carries a scoped `#[allow(clippy::disallowed_methods)]` for
   `Instant::now` — the workspace ambient-time ban is D35's lint, and D35 binds the core,
   not a measurement harness.

## W-ID ledger (each red produced by breaking production code, never the assertion)

| W | Claim | Injected defect → observed red |
|---|---|---|
| W1 | insert-above shifts content, snapshot honest | sort ascending → wrong ids |
| W2 | delete-in-window | `Delete` no-op → count/rows stale |
| W3 | tail clamp | end-clamp dropped → slice panic |
| W4 | two windows independent | window-id collision → wrong ranges |
| W5 | close releases (C22 analog) | `close_window` no-op → live count 1 |
| W6 | coalescing by construction | version tick frozen → version 0 |
| W7 | edit under sort movement | rebase drops `updated_at` → stale position |
| W8 | C15 base-version tracks rebase | `base_version` not stamped → 3 ≠ 4 |
| W9 | deletion orphans (C11) | orphan call removed → status `Live` |
| W10 | C07 precedence inherited | gates hand-rolled conflicted-first → `Err(Conflicted)` |
| W11 | create-flow never rebases (C12) | registered as rebasing → count 1 ≠ 0 |
| W12 | fan-out exactness | `break` after first match → one id of two |
| W13 | two orders, one collection | `TitleAsc` arm ignores sort → wrong order |
| W14 | next snapshot is the new query | `set_query` no-op → old projection |
| W15 | `total_count` filtered | whole-collection count → 4 ≠ 2 |
| W16 | hidden row stays first-class | filter ignored → hidden rows visible |

W15/W16's breaks legitimately reddened filter-sharing neighbors; every target went red for
its own claim. Verification gates: `mise run check` exit 0 after every milestone (re-run by
the orchestrator, not trusted from reports); `collection-core` 16 passed / 1 ignored.

## Friction log

1. **`StoreDraft::from_canonical(None, ..)` has no identity channel** — the one place the
   frozen surface visibly assumes single-entity (see finding 1).
2. **`Store`'s registry loop is duplicated** — cheap (a method each) but real; whether a
   generic many-canonical registry belongs in `bolted-core` is an extract-later question.
3. **`bolted_core::DraftId`'s constructor is sealed**, so the peer store mints `RowDraftId`
   locally (as M0's `WindowId`). Arguably correct — a capability token — and itself evidence
   of the peer split: the id type *cannot* be borrowed.
4. **The workspace `Instant::now` clippy ban caught the perf probe** — D35's lint working as
   designed; resolved with a scoped, commented allow.

## Open questions → proposed agenda for the windowed-collections design session

1. **The store-shape ruling.** Bless the peer container (many canonicals, by-`RowId`
   registry loop)? Does a generic registry move into `bolted-core`, or does each collection
   stay hand-written until a second collection facet exists (the D20 discipline)?
2. **Create-flow identity.** Should `StoreDraft`'s create-flow contract carry `RowId`?
   Client-key-as-input worked; the frozen assumption is now named.
3. **The `total_count` shape.** One field (filtered count — internally consistent) vs two
   (`collection_count` + `matched_count` — honest for UI affordances); and known
   vs lower-bound for the unbuilt paging case.
4. **The async-projection contract.** A sync pull core needs no single-flight (proven); the
   moment projection moves off the pull path, generation-stamping becomes load-bearing.
   Design it only with its consumer.
5. **Etiquette.** Does ~0.65 ms p50 at 10k×4 license naive-plus-coalescing, and what is the
   trigger for incremental projection? (The wire/daemon topology multiplies observers.)
6. **Version semantics.** Per-canonical-event tick (frozen-store parity) vs net-change-only.
7. **The query surface.** Closed enum (macro-stampable, conformance-testable) vs open
   predicate — and case-folding for text filters.
8. **The `Row` projection.** `RowView` was hand-shaped (id, title, updated_at); what a
   generated projection is — and what the declaration for it looks like — was outside the
   spike.
