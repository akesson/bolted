> **What this is.** The full `docs/ARCHITECTURE.md` as it stood on the `design/core-evolution`
> branch (base `8ecc1c9`, last touched 2026-07-09) when the branch was rebased onto main's
> step-16 state. Main's `docs/ARCHITECTURE.md` was rewritten in parallel (v1.8) and is the
> authoritative document; **nothing here is current**. Preserved because these deltas exist
> nowhere in main's docs and merging them is a design-session decision, not rebase mechanics:
>
> - "Observation is watch-shaped, not queue-shaped" (the `Latest` contract)
> - "Large collections: windowed observation"
> - "The runtime: a synchronous reduce loop" (incl. the frame-loop/FFI-crossing rule)
> - "Rules as artifacts (nothing load-bearing lives only in docs)"
> - interaction-replay preconditions
> - the `facet` vocabulary sweep (see `docs/GLOSSARY.md` — the term is owner-approved)
> - the bolted-http S8 row / S9 freeze-gate item
>
> The branch's ROADMAP edits were dropped outright as superseded by main's roadmap; the
> branch's step-02 findings live at `spikes/profile-ffi-stall-probe/docs/`.
>
> **Per-delta merge triage against v1.8: [core-evolution-triage.md](core-evolution-triage.md)**
> — the design session's input; read it instead of diffing this snapshot by hand.

# Bolted — Architecture

**Status: design draft, pre-validation.** This document records the architecture as designed;
Phase 1 of [ROADMAP.md](ROADMAP.md) exists to validate it. Sections marked **OPEN** are
deliberately undecided. Read [VISION.md](VISION.md) first for scope and principles —
especially the verification ladder, which every decision below is justified against.

---

## 1. The shape: MVVM with an Elm core

- **Model** — an Elm-style core per **facet** (a domain-grouped reactive unit; see
  [GLOSSARY.md](GLOSSARY.md)): single typed state, internal messages, a pure `update`
  function, effects as data. Messages never cross the FFI.
- **Facet binding** (where MVVM would say ViewModel) — generated per platform from the
  facet's contract: thin, dumb glue binding it to `@Observable` (Swift) / `StateFlow`
  (Kotlin) / `INotifyPropertyChanged` (C#). Rust shells (web via Leptos/Dioxus/Silkenweb,
  Linux-native) consume the contract directly as a crate — no codegen.
- **View** — fully native, owned by the app, holds no business logic and **no constraint
  literals** (a max length appearing in shell code is a defect — greppable in CI).

**Facets are scoped by domain cohesion, not by view structure — Bolted ships no ViewModels.**
A screen may compose several facets; a tray icon may observe a sliver of one. A ViewModel, if
an app wants one at all, is the app's own view-scoped composition over facet bindings.

A "view" is any native surface, not just a window: a tray/menu-bar icon, a file-manager
extension, a widget, a CLI — each is just another (often tiny) observer of facets
sending commands back. The main app window has no privileged status in the contract.

The contract a facet exposes has exactly three verbs (CQRS-shaped):

| Verb | Surface | Semantics |
|------|---------|-----------|
| **observe** | `observe() -> Latest<FacetSnapshot>` | read-only, always-valid state; watch-shaped — only the newest snapshot is observable |
| **command** | `toggle_x() -> Result<(), CmdError>` | single-action mutation, validate-or-reject |
| **draft** | `checkout() -> FacetDraft` | multi-field edit session: checkout → edit → validate → submit |

Rule of thumb: a mutation that touches one field and needs no editing session is a command;
otherwise it's a draft.

**Canonical core state is never mid-edit.** All editing happens inside drafts. The Elm update
function never sees keystrokes — submit dispatches one message carrying the fully-validated
result (`Msg::ProfileSubmitted(ValidProfile)`), so the event log is a domain log ("profile
updated"), not a keystroke log. This is what makes replay/time-travel meaningful.

### Observation is watch-shaped, not queue-shaped

`Latest<T>` (cf. `tokio::sync::watch`) has one operation: read/await the newest value. That
single choice turns several rules into types instead of docs: coalescing is always legal
(intermediate snapshots are *unobservable*, so no consumer can ever depend on seeing them),
renderers decouple from emission rate (a 120 Hz frame loop polls at its own cadence — neither
side has to keep up with the other), and delta protocols over the boundary are unrepresentable.
This is safe only because every snapshot is self-contained — the property the rest of the
design already guarantees.

### Large collections: windowed observation

Collections never cross the boundary whole; the only generated accessor is a window:

```rust
fn open_window(&self, range: Range<u32>) -> CollectionWindow<Row>;   // per-observer handle

pub struct WindowSnapshot<Row> {
    version: u64,        // collection version
    total_count: u32,    // scrollbar geometry without shipping the vec
    range: Range<u32>,   // may be clamped by the core
    rows: Vec<Row>,      // read-only projections with stable RowIds
}
```

`CollectionWindow` is the draft pattern reused on the read side: a core-side handle with its
own `Latest<WindowSnapshot>`, one per observer (the tray's top-5 and the main window's
`23..45` are different windows onto one collection), subject to the same GC-lifecycle question
(§9). `set_range` is an ordinary input (scroll position enters the replay log). Rows carry
stable `RowId`s — identity, not indexes — so declarative shells (SwiftUI `ForEach`, Compose
keys, `DiffUtil`) diff consecutive snapshots natively for insert/remove/move animations;
deltas never cross the FFI (§8). Editing a row is `checkout(row_id)` — observe/command/draft
is unchanged.

## 2. Validation: three tiers

- **Tier 1 — value types.** No bare `String`/`i64` in contracts. Every field is a constrained
  newtype (nutype-style declaration): `PersonName` = trim + 1..=30 chars. Constraints are
  *declared*, which enables two things: `try_new` enforcement in Rust, and **constraint
  metadata exported to shells** (max length, required, pattern) so UIs derive affordances
  (field `maxLength`, counters, required markers) from the same single source of truth.
  Many "cross-field" rules dissolve into tier 1 as **composite value objects**: end-after-start
  is `DateRange::try_new(start, end)`, one value, one field, one grouped setter.
- **Tier 2 — draft rules.** Explicit functions on the draft for genuinely relational rules
  (allocations sum to 100%): `#[rule(pins(email))] fn corporate_email(&self) -> Result<(), RuleError>`.
  Rules declare which fields their errors pin to; pinning a nonexistent field is a compile
  error. Called explicitly by the UI (set several fields, then validate).
- **Tier 3 — submit.** `submit()` re-validates **everything**, every time. This floor is
  non-negotiable and is what makes all UI-side flexibility safe: no orchestration choice by a
  shell can push invalid data past the boundary.

**Async validation** (e.g. username uniqueness) is an effect from the draft with **single-flight
semantics owned by the core**: a new check cancels the in-flight one; stale completions are
discarded by sequence number. Shells choose *when* to trigger (debounce is shell taste); the
core guarantees ordering correctness. Verdicts are **value-bound**: any change to the checked
field's value — edit *or* rebase — resets the check to unchecked, so a completed verdict never
endorses a value it wasn't computed for (step-01 friction F1; invariant 13, §8).

**Validation policy belongs to the UI; verdicts belong to the core.** Shells decide when to call
`try_set` / rules / async checks and what to display when. The litmus test: shells may add
*when*, never *what* — if implementing a UI behavior requires restating a constraint, it's
forbidden. (A core-side error-visibility policy layer was considered and deliberately deferred —
see §8.)

**Errors are data, never strings.** Every error is a key + structured params
(`TooLong { max: 30, actual: 45 }`); shells localize. Ties into the future i18n battery.

## 3. Field: validity × sync

Each draft field is `Field<V: Value>` with **two independent dimensions**:

```
validity: Unset | Valid(V) | Invalid { raw: V::Raw, error: V::Error }
sync:     InSync | Conflicted { base: Option<V>, theirs: V }
```

- `try_set(raw)` always records the attempt: `Ok` → `Valid(v)`, `Err` → `Invalid{raw, error}`
  (returns the verdict either way). `Invalid` blocks submit — this closes the
  **stale-value-submit bug** (edit "Alice" → invalid text → submit must NOT silently send
  "Alice").
- **Dirty is value-based, not touch-based**: dirty ⇔ current value ≠ base value. Editing a
  field back to its original value makes it clean again (revert-for-free). `Invalid` is
  always dirty.

## 4. Drafts: core-side handles with live rebase

Drafts live **core-side**; shells hold handles (FFI class objects / plain Rust references on
Rust shells). Rationale: validation and derived values (`computed_total()`) run in Rust during
editing; a detached value-copy would fork logic.

**Live rebase.** A draft stays subscribed to canonical changes on its base entity. On change,
per field: not dirty → silently adopt theirs, update base, stay `InSync`; dirty → enter
`Conflicted { base, theirs }` (yours preserved); dirty but yours == theirs → adopt, clean
(convergent edit). Rebase re-runs validation and derived values. Resolution is framework API:
`resolve_keep_mine()` (rebase base to theirs, keep your value, stay dirty) /
`resolve_take_theirs()` (adopt, clean). `{base, yours, theirs}` is exposed so an app *can*
build its own merge UI; **field-level keep/take is the framework's ceiling — no text/CRDT
merging, ever** (perimeter).

- Canonical entity deleted while a draft is open → whole-draft status `Orphaned`; submit on
  orphaned is a typed outcome the app decides (fail / convert-to-create).
- Because drafts live in the core and the store serializes state changes, **there is no
  conflict window at submit** within one device: submit refuses while any field is
  `Conflicted`, and that's never a surprise (the UI already showed it). `SubmitError::Conflict`
  survives only for the outer core↔server loop — the same pattern telescoped
  (shell↔core mirrors core↔server: snapshot down, transactional submit up, reconcile).
- Drafts expose their own snapshots (the same watch-shaped `Latest` as features — they can
  change from underneath via rebase). A draft is thus a mini facet — same
  observe+operations shape, same generated binding machinery, reused.
- `checkout()` is live by default; a `checkout_frozen()` escape hatch may exist for flows that
  must not shift underfoot.
- Discard = drop. `is_dirty()` = diff vs base. Cancel and unsaved-changes warnings are free.

**Commit is the parse-don't-validate moment**: `commit(self) -> Result<Entity, ValidationReport>`
— a `Draft` goes in, an always-valid `Entity` comes out, or a report keyed by typed field IDs.
On success the core may normalize / server-round-trip; the shell receives final truth via the
ordinary snapshot stream (never its own input echoed back). A refused submit must never destroy
the edit session: store-level `submit` consumes the draft only on the success path and returns
the handle alongside the error otherwise (step-01 friction F3; §8).

## 5. Manifestation: generics for behavior, macros for names, traits as contracts

Hard FFI constraint driving this: **generic methods cannot cross a language boundary** — the FFI
surface must be monomorphic with concrete names. Therefore:

- **Generic framework types** carry all logic (rung 1, written once): `Field<V>`, `Store<F>`,
  `ValidationReport<FieldId>`, single-flight machinery.
- **Derive/attr macros** do only mechanical name-stamping, delegating immediately to the
  generics: `#[bolted::value]` (newtype + `Value` impl + constraint metadata),
  `#[bolted::entity]` (snapshot + draft struct of `Field<V>`s + `FieldId` enum + monomorphic
  `try_set_name(...)` methods), `#[bolted::rules]`, `#[bolted::facet]` (composes down
  onto BoltFFI's `#[data]`/`#[export]`). Thin macros are a verification-ladder requirement:
  macro output is the least-verifiable code, so it must stay trivial.
- **Traits** are the contracts: `Value` (Raw / Error / try_new / into_raw / constraints),
  `Draft` (FieldId / conflicts / validate / commit), `Facet` (State / Msg / Effect /
  Snapshot / update).

Key trait sketches (authoritative signatures live in the step docs / code):

```rust
pub trait Value: Clone + PartialEq + Send + Sync + 'static {
    type Raw:   Clone + PartialEq + Send + Sync + 'static;
    type Error: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static;
    fn try_new(raw: Self::Raw) -> Result<Self, Self::Error>;
    fn into_raw(self) -> Self::Raw;
    fn constraints() -> &'static [Constraint];
}

pub trait Draft {
    type Entity;
    type FieldId: Copy + Eq + std::fmt::Debug;
    fn dirty_fields(&self) -> Vec<Self::FieldId>;
    fn conflicts(&self) -> Vec<Self::FieldId>;
    fn validate(&self) -> ValidationReport<Self::FieldId>;
    fn commit(self) -> Result<Self::Entity, ValidationReport<Self::FieldId>>;
}

pub trait Facet {
    type State;
    type Msg;                          // internal, never exported over FFI
    type Effect;                       // data — executed by the driver, never the core
    type Snapshot: Clone + PartialEq;
    fn update(state: &mut Self::State, msg: Self::Msg, ctx: &Ctx) -> Vec<Self::Effect>;
    fn snapshot(state: &Self::State) -> Self::Snapshot;
}
```

### The runtime: a synchronous reduce loop

The Elm part of the core is a function plus a value-diff — no async, no reactive graph:

```rust
impl<F: Facet> FacetCore<F> {
    pub fn dispatch(&mut self, msg: F::Msg, ctx: &Ctx) -> Vec<F::Effect> {
        let effects = F::update(&mut self.state, msg, ctx);
        let snap = F::snapshot(&self.state);
        if snap != self.last { self.last = snap.clone(); self.publish(&snap); }
        effects
    }
}
```

Snapshot emission is recompute-and-compare — value-based, like dirty and convergent rebase:
one philosophy throughout. Fine-grained reactivity (`reactive-graph`, `futures-signals`) was
rejected (§8). Everything async lives in the **driver**: `bolted-ffi` (native) or the shell
itself (Rust web) executes effects on a real runtime and feeds completions back as plain
inputs — the single-flight token pattern is the template for all of it. The core's complete
input set is four kinds — commands, draft calls, effect completions, canonical pushes — the
total order that interaction replay (§9) records. When a facet needs time or fresh
identities, they arrive through the `Ctx` argument, stamped by the driver at dispatch and
recorded as part of the input (shape **OPEN**, §9).

**Crate layout** (physicalizes VISION's narrow-coupling promise):

```
bolted-core    all traits + generic types; sans-io; NEVER depends on boltffi
bolted-macros  the derives; output = thin delegation to bolted-core
bolted-ffi     the ONLY crate importing boltffi (the swappable seam)
bolted-check   build-time analyses (drift, coverage, constraint semver)
```

**Sans-io / runtime-agnostic core**: effects are data driven by the platform layer; no tokio in
`bolted-core`. This is what makes headless deterministic tests and wasm32 compatibility
structural rather than aspirational. The rule extends to **all ambient nondeterminism**: no
clocks, no randomness, no ID generation inside the core — timestamps and identities arrive as
inputs (command arguments or effect results). Core state evolution is therefore a pure
function of its input sequence, which is what keeps interaction replay (§9) possible; slated
to be promoted to invariant 14 at the design freeze (13 is taken by value-bound verdicts).
Enforced today by the workspace `clippy.toml` deny-list (`SystemTime::now` / `Instant::now`)
riding the existing `-D warnings` gate; `bolted-check` widens this later.

### Rules as artifacts (nothing load-bearing lives only in docs)

VISION's founding rule applied to the design itself: every rule above is carried by an
artifact on the verification ladder; prose is commentary, never the enforcement.

| Rule | Carried by | Rung |
|------|-----------|------|
| Coalescing legal; no frame coupling | `Latest<T>` — intermediates unobservable | 1 |
| Collections never ship whole | `CollectionWindow` is the only accessor | 1 |
| Entity always valid | constructible only via `commit` | 1 |
| `Msg` never crosses FFI | macros generate no binding for it | 1–2 |
| No ambient time/randomness in core | `Ctx` as the only source + clippy deny-list | 1 + 3 |
| Echo rule; frame loop off the FFI | shipped components over an opaque `FieldBinding` — generated glue exposes no raw `Binding<String>` | 2 |
| Windowing etiquette (overscan, thresholds) | `BoltedList`-style adapters | 2 |
| No constraint literals in shells | metadata-driven components + `bolted-check` grep | 2 + 3 |

## 6. Platform notes

- **Text echo rule**: the native text control owns its text *while focused*; core `raw` is
  authoritative on blur/programmatic change. Sanitization runs on blur/commit, not keystroke
  (cursor survival). Validated in the Swift spike.
- **The frame loop never crosses the FFI** (the echo rule generalized). Rendering runs at
  frame rate from natively held state; the core hears *events*, not frames. Continuous
  gestures (slider, drag-reorder) keep the value native while live and commit at boundaries —
  TCA's best-documented pitfall (per-frame reducer round-trips) designed out. Scroll uses
  window overscan + threshold refetch, never per-frame `set_range`. Core-driven churn is
  conflated at the driver (`Latest` semantics, e.g. a buffer-1 `AsyncStream`); sparse
  snapshots animate via native implicit animation. 120 Hz/ProMotion is thus a shell-rendering
  concern by construction — still measured in steps 03/05.
- **GC languages (Kotlin, C#)**: no deterministic destruction → core-side draft handles leak
  unless the API has an explicit lifecycle. Likely outcome: explicit `close()` everywhere for
  symmetry. **OPEN** until the Android probe (Step 5).
- **JNI is the performance worst case**, not Swift: the per-keystroke `try_set` bet must be
  measured on Android, not inferred from Swift.
- **Process death (Android)**: core-side drafts die with the process → drafts must be
  serializable with a stash/restore hook. Design in Phase 2.
- **Rust shells** (web, Linux-native): consume `bolted-core` + feature crates directly; zero
  FFI; the web target also enforces `wasm32-unknown-unknown` discipline on the whole core.

## 7. Invariants (the conformance suite seed)

These are the design's falsifiable claims; they exist as tests from Step 1 onward and later
become per-language contract tests:

1. `Value::try_new(v.into_raw()) == Ok(v)` for every valid `v` (roundtrip).
2. A non-dirty field always equals canonical after rebase (`InSync`).
3. A dirty field is never silently overwritten by rebase (yours preserved, `Conflicted`).
4. Convergent rebase (yours == theirs) lands clean and `InSync`.
5. Setting a field back to its base value clears dirty.
6. A failed `try_set` blocks submit (no stale-value submit).
7. `commit` succeeds ⇔ all fields `Valid`, none `Conflicted`, no rule violations; the
   committed entity equals the field values.
8. Rebase re-runs tier-2 validation.
9. `resolve_keep_mine`: value=yours, base=theirs, dirty, `InSync`. `resolve_take_theirs`:
   value=theirs, clean, `InSync`.
10. Stale async completions (old sequence) are ignored; latest wins.
11. Canonical deletion ⇒ draft `Orphaned`; submit on orphaned is a typed error.
12. Create-flow drafts (no base) never rebase and commit normally.
13. A completed async-check verdict does not survive a change to the checked field's value:
    edit or rebase of the pinned field resets the check to unchecked. *(Added after step 01
    — F1; the test lands with the fix, scheduled with the step-03 implementation session.)*

## 8. Resolved decisions (with the losing alternative)

| Decision | Rejected alternative | Why |
|----------|---------------------|-----|
| Lean contract; UI orchestrates validation timing | Core-side visibility-policy enums (`touched`, `visible_errors`) | Presentation-adjacent state in core violated prefer-simple; retrofittable later as an optional battery ("FormKit") on top of the lean surface — the reverse migration would be breaking |
| Drafts core-side (handles) | Detached value-struct copied to shell | Validation/derived values must not fork; leans on BoltFFI cheap calls (the core bet) |
| Live rebase default | Frozen drafts, conflicts at submit | Silent staleness is a data-loss bug, not a UX taste; machinery is once-in-framework, dormant when canonical is quiet |
| Value-based dirty | Touch-based | Revert-for-free; dirty stays a pure function of data |
| Field-level keep/take conflict ceiling | Text/CRDT merge | Perimeter: rung-4 complexity, different product |
| Sans-io core | Async runtime in core | Deterministic tests, wasm32, Elm effect model |
| Errors as key+params | Message strings from core | Localization is shell/platform territory |
| Snapshot-per-change streams (feature + draft) | Per-property notifications | Simpler surface, native UIs diff anyway, enables replay |
| Value-bound async verdicts: pinned-field change (edit or rebase) resets the check to unchecked | Verdict carries the value it validated, compared at `validate()`; or shell re-triggers on change | A stale `Done(Ok)` endorsing a value it never saw is a correctness bug (step-01 F1); reset-on-change keeps the state model minimal (unchecked means unchecked); shell-side re-trigger is exactly the runtime glue Bolted exists to remove |
| Failed submit returns the draft handle with the error | Submit consumes the handle on every outcome | Losing the user's edits on a rejected submit is data loss (step-01 F3); pre-checks run under a borrow, so only the success path needs ownership |
| Watch-shaped observation (`Latest<T>`) | Queue-shaped `Stream` of snapshots | Coalescing legal by type — consumers can't depend on intermediates; frame-rate decoupling; delta protocols unrepresentable |
| Synchronous reduce loop; snapshots recomputed + value-diffed | Reactive graph inside the core (`reactive-graph`, `futures-signals`) | Wrong scale (dozens of fields, not a DOM); zero-dep rule; scheduler semantics muddy replay; a signal can't cross FFI anyway — shells that want signals mirror snapshots into their own |
| Windowed collections, snapshot-authoritative | FFI delta protocol (`VecDiff`-style added/removed/inserted) | Deltas die on coalescing/reordering (rung-4 fragility; BoltFFI stream semantics undocumented); the window already makes payloads small; keyed rows let native UIs diff/animate; replay compares snapshots trivially |
| Frame loop never crosses FFI | Per-frame gesture/scroll round-trips through the core | Betting the 8.3 ms ProMotion budget on FFI + encode + diff is the wrong shape even where it fits; the core hears event boundaries |
| `bolted-http`: sans-io contract crate + Bolted-shipped shell-side adapters (URLSession in Swift, OkHttp in Kotlin, WinRT in C#; Rust adapters only for Linux/web) | One Rust client binding the native stacks directly (objc2/windows-rs/JNI, nyquest-style) | Android has no credible Rust path to OkHttp/Cronet, so shell-side is the uniform default; adapters are rung-2 shipped components (the effect-side `BoltedTextField`), living in the framework's maintenance envelope with a per-adapter conformance suite; the contract is placement-blind, so Rust-side Darwin/Windows bindings remain a recorded retreat if step-02 callback measurements come back ugly (full design: `crates/bolted-http/docs/`) |

## 9. OPEN questions (do not resolve ad hoc — bring to a design session)

- Draft handle lifecycle in GC languages (`close()`? `use`-block?) — pending Step 5.
- One-shot effects (focus-first-invalid-field, toasts, **navigation**) — pattern undesigned;
  likely `Option<(Request, Generation)>` state + ack, but navigation deserves its own session.
- Draft stash/restore for process death — Phase 2.
- Commit policy for never-run async checks (step-01 F2): unchecked currently *passes*, so a
  draft that never triggered its uniqueness check commits client-side unverified (tier-3
  server re-check is the backstop). Require-checked before commit, auto-trigger on validate,
  or accept-with-backstop — decide at the freeze with step-03 evidence.
- Focused-but-untouched field during rebase: updates live (rule stays pure); shells may soften
  visually — revisit after Swift spike if it feels wrong.
- Store↔draft wiring (step-01 Q1): live-rebase driving (`from_canonical`/`rebase`/`orphan`)
  sits on a `StoreDraft` subtrait in the prototype, keeping the public `Draft` contract (the
  FFI surface) exactly as §5 — promote into `Draft`, or keep the split? Decide at the freeze
  with step-02 FFI evidence.
- `Value::Error: Into<ErrorData>` as a trait bound (step-01 Q2)? The spike's external bridge
  (`From<XError> for ErrorData` + a bounded helper) was cleaner than per-field code and points
  at promoting it into `Value`.
- `Constraint::Required` channel (step-01 Q3/D3): a value type can't know whether its field is
  `Option<_>`, so the spike prepends `Required` at the field/entity layer — same enum, or a
  separate field-metadata channel?
- `commit` error taxonomy (step-01 Q4/F5): `commit` re-encodes conflicts/orphan as synthetic
  rule violations while store `submit` has typed `Conflicted`/`Orphaned` variants — two
  taxonomies for the same failures; unify (e.g. `CommitError { Validation | Conflict |
  Orphan }`)?
- `Copy` value objects vs uniform generated `.clone()` (step-01 F4): clippy `clone_on_copy`
  under `-D warnings` forbids cloning a `Copy` field, so codegen must either track `Copy`-ness,
  blanket-`allow` generated modules, or forbid `Copy` on value objects (spike's
  recommendation). Decide at the freeze, binds step 09.
- Conflicted field edited to equal `theirs` stays `Conflicted` (step-01 F6): resolution is an
  explicit user act in the prototype — confirm or auto-converge, with step-03 UI evidence.
- `SyncState::Conflicted { base, theirs }` duplicates `Field.base` (step-01 F7): they are
  always equal while conflicted; keep the self-contained 3-way shape or drop `base` from the
  variant — decide at the freeze/extraction.
- `Ctx` shape: what does `update` receive — `now()`? `fresh_id()`? seeded rng? — and how the
  driver stamps it and the replay log records it. No current feature needs it; design when one
  does, at the latest at the freeze.
- The opaque `FieldBinding` + shipped component kit (`BoltedTextField` / `BoltedSlider` /
  `BoltedList`): exact shape is step-03 territory — hand-written there as the as-if-generated
  reference, frozen with its evidence.
- Collection row projection: what a `Row` is, where `RowId` comes from (entity key?), whether
  `open_window` takes sort/filter parameters. The window/`Latest` contract (§1) is the fixed
  part; the projection is designed when the first real collection feature lands.
- Store concurrency model behind FFI (single-threaded actor vs `Arc<Mutex>`) — prototype uses
  the simplest thing; decide at Phase 3 extraction.
- `bolted-http` contract freeze: the portable request effect and capability split are
  sketched in `crates/bolted-http/docs/architecture.md` but frozen only after step-02
  evidence — response streaming in/out of the portable core (BoltFFI stream semantics),
  cookie capability shape, and whether Android's declarative `<pin-set>` binds OkHttp/Cronet.
  `BackgroundTransfer` is a separate optional effect family whose precondition — effects as
  durable, serializable data with stable identities — is shared with interaction replay
  (below) and draft stash/restore; nothing in Phases 1–2 may foreclose it.
- Process topology for OS-integration surfaces (VISION: daemons, tray, file-manager
  extensions): where the core runs (embedded vs daemon), how sandboxed extension processes
  reach it (the contract over IPC?), single-instance ownership. Undesigned — needs its own
  spike after Phase 2; nothing in Phases 1–2 depends on it.
- **Interaction replay (protected possibility, unscheduled).** The contract boundary is a
  natural record seam: every mutation enters the core as a typed, serializable call (commands,
  draft setters, `apply_canonical`, async-check completions), so logging those calls and
  re-driving the log against a fresh core yields deterministic session replay — bug reports
  that attach a replayable log, time-travel debugging, UI-less integration tests, and the
  strongest cross-platform conformance test available (same log ⇒ identical snapshot sequences
  on every platform). Since macros generate the boundary, recording could be stamped in as a
  free battery. Not designed and not scheduled, but three preconditions must survive other
  decisions: (1) the no-ambient-nondeterminism rule (§5); (2) draft handles need stable
  logical identities (e.g. checkout sequence number), never pointer identity; (3) inputs need
  a total order — a standing argument for the serialized/actor option in the store-concurrency
  decision above. Replay reproduces core state, not pixels: native view state (focus, IME
  composition, scroll) never crosses the boundary and is out of scope by design.

## 10. Prior art

- *Parse, don't validate* (Alexis King) — tier 1's philosophy; commit as the parse moment.
- **nutype** crate — the declaration mechanics for value types (sanitize/validate attrs).
- *Domain Modeling Made Functional* (Wlaschin) — value objects + Result workflows.
- **Crux** — closest relative (TEA over FFI); Bolted differs in the typed contract layer
  (drafts, fields, structured errors) vs serialized view-models and stringly events.
- Working-copy/checkout-commit (git, ORM unit-of-work) — the draft pattern; three-way merge
  with the trivial rule "unmodified takes theirs".
