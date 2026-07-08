# Bolted — Architecture

**Status: design draft, pre-validation.** This document records the architecture as designed;
Phase 1 of [ROADMAP.md](ROADMAP.md) exists to validate it. Sections marked **OPEN** are
deliberately undecided. Read [../VISION.md](../VISION.md) first for scope and principles —
especially the verification ladder, which every decision below is justified against.

---

## 1. The shape: MVVM with an Elm core

- **Model** — an Elm-style core per feature ("feature model"): single typed state, internal
  messages, a pure `update` function, effects as data. Messages never cross the FFI.
- **ViewModel** — generated per platform from the feature model's contract: thin, dumb glue
  binding the contract to `@Observable` (Swift) / `StateFlow` (Kotlin) /
  `INotifyPropertyChanged` (C#). Rust shells (web via Leptos/Dioxus/Silkenweb, Linux-native)
  consume the contract directly as a crate — no codegen.
- **View** — fully native, owned by the app, holds no business logic and **no constraint
  literals** (a max length appearing in shell code is a defect — greppable in CI).

The contract a feature model exposes has exactly three verbs (CQRS-shaped):

| Verb | Surface | Semantics |
|------|---------|-----------|
| **observe** | `snapshots() -> Stream<FeatureSnapshot>` | read-only, always-valid state, flows continuously |
| **command** | `toggle_x() -> Result<(), CmdError>` | single-action mutation, validate-or-reject |
| **draft** | `checkout() -> FeatureDraft` | multi-field edit session: checkout → edit → validate → submit |

Rule of thumb: a mutation that touches one field and needs no editing session is a command;
otherwise it's a draft.

**Canonical core state is never mid-edit.** All editing happens inside drafts. The Elm update
function never sees keystrokes — submit dispatches one message carrying the fully-validated
result (`Msg::ProfileSubmitted(ValidProfile)`), so the event log is a domain log ("profile
updated"), not a keystroke log. This is what makes replay/time-travel meaningful.

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
core guarantees ordering correctness.

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
- Drafts expose their own snapshot stream (they can change from underneath via rebase). A
  draft is thus a mini feature-model — same stream+operations shape, same generated binding
  machinery, reused.
- `checkout()` is live by default; a `checkout_frozen()` escape hatch may exist for flows that
  must not shift underfoot.
- Discard = drop. `is_dirty()` = diff vs base. Cancel and unsaved-changes warnings are free.

**Commit is the parse-don't-validate moment**: `commit(self) -> Result<Entity, ValidationReport>`
— a `Draft` goes in, an always-valid `Entity` comes out, or a report keyed by typed field IDs.
On success the core may normalize / server-round-trip; the shell receives final truth via the
ordinary snapshot stream (never its own input echoed back).

## 5. Manifestation: generics for behavior, macros for names, traits as contracts

Hard FFI constraint driving this: **generic methods cannot cross a language boundary** — the FFI
surface must be monomorphic with concrete names. Therefore:

- **Generic framework types** carry all logic (rung 1, written once): `Field<V>`, `Store<F>`,
  `ValidationReport<FieldId>`, single-flight machinery.
- **Derive/attr macros** do only mechanical name-stamping, delegating immediately to the
  generics: `#[bolted::value]` (newtype + `Value` impl + constraint metadata),
  `#[bolted::entity]` (snapshot + draft struct of `Field<V>`s + `FieldId` enum + monomorphic
  `try_set_name(...)` methods), `#[bolted::rules]`, `#[bolted::feature_model]` (composes down
  onto BoltFFI's `#[data]`/`#[export]`). Thin macros are a verification-ladder requirement:
  macro output is the least-verifiable code, so it must stay trivial.
- **Traits** are the contracts: `Value` (Raw / Error / try_new / into_raw / constraints),
  `Draft` (FieldId / conflicts / validate / commit), `Feature` (State / Msg / Caps / update).

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
```

**Crate layout** (physicalizes VISION's narrow-coupling promise):

```
bolted-core    all traits + generic types; sans-io; NEVER depends on boltffi
bolted-macros  the derives; output = thin delegation to bolted-core
bolted-ffi     the ONLY crate importing boltffi (the swappable seam)
bolted-check   build-time analyses (drift, coverage, constraint semver)
```

**Sans-io / runtime-agnostic core**: effects are data driven by the platform layer; no tokio in
`bolted-core`. This is what makes headless deterministic tests and wasm32 compatibility
structural rather than aspirational.

## 6. Platform notes

- **Text echo rule**: the native text control owns its text *while focused*; core `raw` is
  authoritative on blur/programmatic change. Sanitization runs on blur/commit, not keystroke
  (cursor survival). Validated in the Swift spike.
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

## 9. OPEN questions (do not resolve ad hoc — bring to a design session)

- Draft handle lifecycle in GC languages (`close()`? `use`-block?) — pending Step 5.
- One-shot effects (focus-first-invalid-field, toasts, **navigation**) — pattern undesigned;
  likely `Option<(Request, Generation)>` state + ack, but navigation deserves its own session.
- Draft stash/restore for process death — Phase 2.
- Focused-but-untouched field during rebase: updates live (rule stays pure); shells may soften
  visually — revisit after Swift spike if it feels wrong.
- Store concurrency model behind FFI (single-threaded actor vs `Arc<Mutex>`) — prototype uses
  the simplest thing; decide at Phase 3 extraction.

## 10. Prior art

- *Parse, don't validate* (Alexis King) — tier 1's philosophy; commit as the parse moment.
- **nutype** crate — the declaration mechanics for value types (sanitize/validate attrs).
- *Domain Modeling Made Functional* (Wlaschin) — value objects + Result workflows.
- **Crux** — closest relative (TEA over FFI); Bolted differs in the typed contract layer
  (drafts, fields, structured errors) vs serialized view-models and stringly events.
- Working-copy/checkout-commit (git, ORM unit-of-work) — the draft pattern; three-way merge
  with the trivial rule "unmodified takes theirs".
