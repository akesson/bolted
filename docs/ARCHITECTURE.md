# Bolted — Architecture

**Status: FROZEN (v1.1, step 06; amended step 07).** Phase 1 validated this design against four
independent shells — pure Rust, Apple/ARC, Rust/wasm, Android/ART — and step 06 reconciled their
friction logs. Every question that Phase 1 could answer is answered, in §8, with the alternative it
beat. What remains **OPEN** in §9 is genuinely undecided and each item names the step that owns it.

**v1.1** carries step 07's two amendments, both owner-approved before implementation: **D14** fixes a
verified defect in `rebase` (C03 amended, C19 added), and **D15** adds `Store::adopt` and the draft
stash. A freeze is a commitment to a design, not a promise that the design was already correct — the
record of what changed, and why, is the point.

Frozen means: §1–§7 are the contract Phases 3–4 extract and generate against. Changing them is a
breaking change to Bolted, not an edit. The falsifiable claims live in
[CONFORMANCE.md](CONFORMANCE.md) as C01–C18, each with a test.

Read [VISION.md](VISION.md) first for scope and principles — especially the verification ladder,
which every decision below is justified against.

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

A "view" is any native surface, not just a window: a tray/menu-bar icon, a file-manager
extension, a widget, a CLI — each is just another (often tiny) observer of feature models
sending commands back. The main app window has no privileged status in the contract.

The contract a feature model exposes has exactly three verbs (CQRS-shaped):

| Verb | Surface | Semantics |
|------|---------|-----------|
| **observe** | read-only, always-valid current state | *how* it is delivered is per-target: a `snapshots()` stream across FFI, a direct read plus a change tick in a Rust shell (§8) |
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
core guarantees ordering correctness. The check's sub-state — unchecked / pending / passed /
failed — is part of the draft's observable snapshot (core-owned verdict state, not presentation
state, so this does not reintroduce the rejected visibility-policy enums of §8), letting a shell
show progress without owning check logic.

Two rules make a client-side verdict trustworthy, and neither is sufficient alone:

- **Verdicts are value-bound** (C13). Any change to the checked field's value — edit, rebase, or
  `take_theirs` — resets the check to unchecked. A completed verdict therefore always belongs to the
  value currently in the field.
- **An unrun check blocks a dirty field** (C16). If a check pins to a field, the field is dirty, and
  the check never ran, `commit` refuses with a rule violation pinned to that field. A *clean* field
  needs no check: it still holds the canonical value, which was verified when it was committed.

Without C13 a stale pass endorses a value it never saw; without C16 the shell can simply never ask,
and both spikes showed that not asking was the **default** path (step-01 F1/F2, step-03, step-04).

**Capability callbacks are synchronous.** Asynchrony is expressed as a shell-driven `begin`/`complete`
effect pair over a `CheckToken`, never as an async trait method — that is what keeps the core sans-io
and what let the browser shell drive a check from `spawn_local` with no executor in the core (§8).

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
sync:     InSync | Conflicted { theirs: V }
base:     Option<V>                        // the common ancestor; does not move while conflicted
```

- `try_set(raw)` always records the attempt: `Ok` → `Valid(v)`, `Err` → `Invalid{raw, error}`
  (returns the verdict either way). `Invalid` blocks submit — this closes the
  **stale-value-submit bug** (edit "Alice" → invalid text → submit must NOT silently send
  "Alice").
- **Dirty is value-based, not touch-based**: dirty ⇔ current value ≠ base value. Editing a
  field back to its original value makes it clean again (revert-for-free). `Invalid` is
  always dirty.
- The two dimensions are independent **state**, not independent **transitions**. `rebase` already
  moved validity; symmetrically, a `try_set` that lands exactly on `theirs` clears the conflict
  (C14). Two edits that agree are not a conflict, whichever arrived first — C04 makes the identical
  judgement when the canonical change arrives second.
- `{base, yours, theirs}` — the 3-way merge data — is read from the field: `base()`, the validity,
  and `theirs()`. The ancestor is stored **once**; duplicating it into the `Conflicted` variant meant
  two copies of one fact to keep consistent (step-01 F7).

## 4. Drafts: core-side handles with live rebase

Drafts live **core-side**; shells hold handles (FFI class objects / plain Rust references on
Rust shells). Rationale: validation and derived values (`computed_total()`) run in Rust during
editing; a detached value-copy would fork logic.

**Live rebase.** A draft stays subscribed to canonical changes on its base entity. On change, every
field of the draft is rebased — so the *first* question each field asks is whether its own canonical
value moved at all. Per field: `theirs == base` → nobody else touched it, keep yours, clear any
conflict, `InSync` (C19); not dirty → silently adopt theirs, update base, stay `InSync`; dirty but
yours == theirs → adopt, clean (convergent edit); dirty otherwise → `Conflicted { theirs }` (yours
preserved). Rebase is therefore idempotent. Rebase re-runs validation and derived values. Resolution
is framework API:
`resolve_keep_mine()` (rebase base to theirs, keep your value, stay dirty) /
`resolve_take_theirs()` (adopt, clean). `{base, yours, theirs}` is exposed so an app *can*
build its own merge UI; **field-level keep/take is the framework's ceiling — no text/CRDT
merging, ever** (perimeter).

- Canonical entity deleted while a draft is open → whole-draft status `Orphaned`; submit on
  orphaned is a typed outcome the app decides (fail / convert-to-create).
- Because drafts live in the core and the store serializes state changes, **there is no
  conflict window at submit** within one device: submit refuses while any field is
  `Conflicted`, and that's never a surprise (the UI already showed it). `SubmitError::Conflicted`
  survives only for the outer core↔server loop — the same pattern telescoped
  (shell↔core mirrors core↔server: snapshot down, transactional submit up, reconcile).
- Across FFI, drafts expose their own snapshot stream (they can change from underneath via rebase);
  a draft is then a mini feature-model, same stream+operations shape, same generated binding
  machinery, reused. A **Rust shell does not want the stream** and does not get one — see §8.
- Every snapshot carries the store `version` it is based on. A draft's stamp advances with each
  rebase (C15), so a consumer can reconcile a `snapshot()` read against a future-only subscription.
- `checkout()` is live by default; a `checkout_frozen()` escape hatch may exist for flows that
  must not shift underfoot.
- `is_dirty()` = diff vs base. Cancel and unsaved-changes warnings are free.

**Stash and restore** (C20/C21). A core-side draft dies with the process, and on Android the process
dies whenever the OS says so. A draft therefore flattens to raw, serializable data — per field, the
last input attempt and the ancestor it was made over — and restores through the store's single draft
entry point, `adopt(D::from_stash(..))`, which rebases it onto whatever canonical says *now*.
`checkout()` is `adopt` of a freshly-built draft.

Two things are deliberately absent from the stash, and their absence is the design:

- **`sync`.** A conflict names a canonical value the server may no longer hold. It re-derives on the
  restoring rebase, against fresh canonical, and so names the right value.
- **The async verdict.** It endorses a value against a server state that may have moved. A restored
  checked field is unchecked, and C16 then refuses to submit it while dirty. C13 + C16 make restore
  safe without an invariant of their own.

What *is* stashed is the ancestor, and it carries every prior resolution with it: a `keep_mine`d field
has `base == old theirs`, so if canonical still holds that value the restored field lands dirty and
`InSync` — the user's decision stands, unlitigated. The stash is also the framework's first
**untrusted input**: an ancestor that no longer parses means the constraints changed between app
versions (`bolted-check`'s constraint-semver snapshots, VISION).

**Commit is the parse-don't-validate moment**: `commit(self) -> Result<Entity, (Self, CommitError)>`
— a `Draft` goes in, an always-valid `Entity` comes out, or the draft comes *back* with a typed
reason (`Validation` / `Conflicted` / `Orphaned`). On success the core may normalize /
server-round-trip; the shell receives final truth via the ordinary observe path, never its own input
echoed back. A refused commit must never destroy the edit session (step-01 F3).

**The handle is a lifecycle object.** `submit` borrows it; on success the draft is consumed and the
handle becomes an inert **tombstone** (`is_live()` false, no draft access, a second submit is
`AlreadySubmitted`), and on refusal the draft goes straight back (C17). `close()` releases the draft,
is idempotent, and stops the store rebasing it (C18).

This is not symmetric across languages, and pretending otherwise would be a lie:

| | release path | use after release |
|---|---|---|
| Rust | `Drop`; `close()` is a convenience | `None` from every borrow |
| Swift / ARC | `deinit` runs Rust `Drop` automatically | impossible (ARC), plus the tombstone |
| Kotlin / C# | **`close()` only** — the GC never frees the Rust draft | **must** be a typed error (step 10) |

Step 05 measured this on ART: an abandoned Kotlin handle is collected while the Rust draft stays
registered forever, an unreachable zombie the store keeps rebasing. So the contract names an explicit
release everywhere, `use { }` / `IDisposable` are the idiomatic forms, and BoltFFI's raw-pointer
handles make use-after-close silent UB today — which `bolted-ffi` must close (§9).

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
pub trait Value: Clone + PartialEq + Send + Sync + 'static {   // and NOT Copy — see §8
    type Raw:   Clone + PartialEq + Debug + Send + Sync + 'static;
    type Error: Clone + PartialEq + Debug + Send + Sync + 'static + Into<ErrorData>;
    fn try_new(raw: Self::Raw) -> Result<Self, Self::Error>;
    fn into_raw(self) -> Self::Raw;
    fn constraints() -> &'static [Constraint];
}

pub trait Draft {
    type Entity;
    type FieldId: Copy + Eq + std::fmt::Debug;
    fn status(&self) -> DraftStatus;
    fn base_version(&self) -> u64;
    fn dirty_fields(&self) -> Vec<Self::FieldId>;
    fn conflicts(&self) -> Vec<Self::FieldId>;
    fn validate(&self) -> ValidationReport<Self::FieldId>;
    fn commit(self) -> Result<Self::Entity, (Self, CommitError<Self::FieldId>)> where Self: Sized;
}

pub enum CommitError<FieldId> { Validation(ValidationReport<FieldId>), Conflicted { fields: Vec<FieldId> }, Orphaned }
pub enum SubmitError<FieldId> { Validation(..),                        Conflicted { .. },                  Orphaned, AlreadySubmitted }
```

`Draft` is the FFI surface and stays minimal. The store-facing plumbing it needs to drive live rebase
— `from_canonical` / `rebase(entity, version)` / `orphan` — sits on a `StoreDraft: Draft` subtrait
that no shell ever calls (§8). `AlreadySubmitted` is the one failure a *handle* can have that a draft
cannot, which is why the two enums differ by exactly that variant.

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

- **Text echo rule**: the native text control owns its text while focused **and typed into**; core
  `raw` is authoritative on blur / programmatic change. Sanitization runs on blur/commit, not
  keystroke (cursor survival). A focused field the user has *not* touched holds nothing worth
  protecting and adopts a rebase live — otherwise two views over the same store visibly disagree
  with nothing on screen to explain it (step-04).

  *The predicate is `touched`, not `dirty`.* Typing `"  alice  "` over the base value `"alice"`
  leaves the field **clean** (the core trims, so the value never moved) while the control holds live
  keystrokes; repainting it would eat the spaces and jump the caret. `touched` is shell-local
  presentation state about a text control — not the core-side `touched` flag §8 rejects.
- **GC languages (Kotlin, C#)**: no deterministic destruction, so `close()` is the only release path
  and forgetting it leaks a Rust draft the store keeps rebasing. Measured on ART in step 05; see §4.
- **JNI is the performance worst case**, not Swift. Measured (step 05, emulator): a per-keystroke
  `try_set` + `snapshot` round-trip costs **12–13 µs** against a 1.0 ms bar, ~1.5–2× Apple's on the
  same host. The per-keystroke bet holds; no shell-side write buffer is needed. *Re-check on physical
  hardware in step 07 — an emulator on an arm64 host is the right VM and the wrong CPU.*
- **Process death (Android)**: core-side drafts die with the process → drafts must be
  serializable with a stash/restore hook. Undesigned; owned by step 07 (§9).
- **Rust shells** (web, Linux-native): consume `bolted-core` + feature crates directly; zero
  FFI; the web target also enforces `wasm32-unknown-unknown` discipline on the whole core.

## 7. Invariants — the conformance suite

The design's falsifiable claims, C01–C21, are stated normatively in **[CONFORMANCE.md](CONFORMANCE.md)**
and exist as named tests (`c01_*` … `c21_*`) in `crates/spike-profile/tests/conformance.rs`. A drift
test parses the document and fails the build if an ID has no test or a test has no ID — the mapping is
verified by the build, not by review (VISION rung 3).

In one line each: **C01** value roundtrip · **C02** a clean field follows canonical · **C03** a dirty
field whose canonical moved is never silently overwritten · **C04** convergent rebase is clean ·
**C05** revert-for-free · **C06** no stale-value submit · **C07** commit is the parse moment, each
refusal typed · **C08** rebase re-runs tier 2 · **C09** resolution semantics · **C10** latest check
wins · **C11** deletion orphans · **C12** create-flow never rebases · **C13** verdicts are value-bound ·
**C14** auto-converge on edit · **C15** the base version tracks the rebase · **C16** an unrun check
blocks a dirty field · **C17** submit tombstones the handle · **C18** release is explicit and
idempotent · **C19** rebase is a three-way merge, and idempotent · **C20** a draft stashes to raw data
and restores from it · **C21** restore is a rebase.

Step 08 makes the suite generic over a feature; step 10 emits it as per-language contract tests.

## 8. Resolved decisions (with the losing alternative)

Decisions from steps 01–05 and the step-06 freeze. Every row cost something; the third column says
what, and why it was worth it.

| Decision | Rejected alternative | Why |
|----------|---------------------|-----|
| Lean contract; UI orchestrates validation timing | Core-side visibility-policy enums (`touched`, `visible_errors`) | Presentation-adjacent state in core violated prefer-simple; retrofittable later as an optional battery ("FormKit") on top of the lean surface — the reverse migration would be breaking |
| Drafts core-side (handles) | Detached value-struct copied to shell | Validation/derived values must not fork; leans on BoltFFI cheap calls (the core bet), now measured at 12–13 µs per keystroke on the worst platform |
| Live rebase default | Frozen drafts, conflicts at submit | Silent staleness is a data-loss bug, not a UX taste; machinery is once-in-framework, dormant when canonical is quiet |
| Value-based dirty | Touch-based | Revert-for-free; dirty stays a pure function of data |
| Field-level keep/take conflict ceiling | Text/CRDT merge | Perimeter: rung-4 complexity, different product |
| Sans-io core | Async runtime in core | Deterministic tests, wasm32, Elm effect model. Proven: the browser shell drove the async check from `spawn_local` and the core produced only a `CheckToken` |
| Errors as key+params | Message strings from core | Localization is shell/platform territory. Confirmed structurally on two codegen backends, so this is a BoltFFI property, not a Swift-generator one |
| Snapshot-per-change streams (feature + draft) | Per-property notifications | Simpler surface, native UIs diff anyway, enables replay |
| Value-bound async verdicts: pinned-field change (edit or rebase) resets the check to unchecked (C13) | Verdict carries the value it validated, compared at `validate()`; or shell re-triggers on change | A stale `Done(Ok)` endorsing a value it never saw is a correctness bug (step-01 F1); reset-on-change keeps the state model minimal (unchecked means unchecked); shell-side re-trigger is exactly the runtime glue Bolted exists to remove |
| **D1** — `Value::Error: Into<ErrorData>` is a trait bound | An external `From` bridge per feature | Two crates independently restated the bound in a `where` clause to write the same three-line match (step-01 Q2, step-04 friction 3). It is part of the contract: a tier-1 error that cannot become report data is not a tier-1 error |
| **D2 (C14)** — `try_set` landing on `theirs` auto-converges | Resolution is always an explicit user act | C04 already makes this judgement when the rebase arrives second; making the outcome depend on event order is indefensible. The running web shell showed the old behaviour as a banner whose "Keep mine" and "Take theirs" buttons did visibly the same thing (step-04 F6 verdict) |
| **D3** — `Conflicted { theirs }`; the ancestor is `Field::base()` | `Conflicted { base, theirs }`, self-contained 3-way data | They are provably always equal while conflicted (step-01 F7). Two copies of one fact is two facts to keep consistent. Invisible at the FFI boundary: the DTO still projects `{base, theirs}` |
| **D4** — `commit(self) -> Result<Entity, (Self, CommitError)>`, typed refusals | `Result<Entity, ValidationReport>`, encoding conflict/orphan as synthetic rule violations | Two divergent taxonomies for one set of failures (step-01 F5). Returning `Self` also lets the store hand the draft back without a pre-check pass that duplicates `commit`'s own gates — which deleted the unreachable branch step-03 apologised for, in the core *and* in the FFI wrapper |
| **D5 (C17/C18)** — the handle is a lifecycle object: `submit(&mut handle)`, tombstone on success, `is_live()`, `close()` | Consume the handle by value on every outcome; `close()` only where GC forces it | The FFI had already invented all of this (step 02's post-submit tombstone and its FFI-only `AlreadySubmitted`); the core API was the one lying. It also deletes the scratch-`checkout()` a `!Clone` handle in a struct field forced on every Rust shell (step-04 friction 1). Cost, measured: +1.8 % lines in the web controller |
| **D6 (C16)** — an unrun check blocks a **dirty** pinned field | Accept, with the tier-3 server re-check as backstop; or block whenever unchecked | Accepting means shipping a client-side "unique" that was never computed, on the *default* path — both spikes submitted `admin` unverified (step-03, step-04 F2). Blocking always would make a user who edited only their email wait on a uniqueness lookup for a username nobody touched |
| **D7 (C15)** — `rebase(entity, version)`; `base_version()` on `Draft` | Drop `version` from draft snapshots | The stamp was written once at checkout and never again, so the version-guarded reconcile step 02 shipped for the subscribe race **could never fire on a draft stream** — dead code since the day it was written (found in step 05, verified in step 06). Making the stamp true is a one-line trait change; dropping it would admit `observe` cannot be reconciled on a draft |
| **D8** — value objects must not be `Copy` | Track `Copy`-ness in codegen, or blanket-`allow(clippy::clone_on_copy)` | Generated checkout/rebase clones every field uniformly; a `Copy` field makes that a hard clippy error under `-D warnings` (step-01 F4). Rust cannot express a negative bound, so `#[bolted::value]` will not emit `Copy` and `bolted-check` flags it |
| **D9** — the echo rule protects a focused **touched** control; a focused untouched field adopts a rebase live | Stale-until-blur (the control owns its text, full stop) | Two views over the same store visibly disagreed with nothing to explain it, and fine-grained reactivity made it *easier* to notice (step-04). Note the predicate is `touched`, not `dirty`: sanitization can make a field clean while the control holds live keystrokes (§6) |
| **D10** — capability callbacks are synchronous; asynchrony is a shell-driven `begin`/`complete` effect pair | An async trait method across FFI | A synchronous checker sufficed on all four shells and kept the core sans-io. An async callback would put an executor on the Rust side of the boundary, which §5 forbids. The cost is that `Pending` is never observable to a `snapshot()` caller between FFI calls — only on the stream (step-02 finding 7, step-05 friction 7) |
| **D11** — `observe` is a contract verb; the snapshot **stream** is an FFI-boundary mechanism | `snapshots()` as a universal contract member | A Rust shell reads the contract directly and drives reactivity from an explicit tick: race-free (synchronous reads of the same memory), forks nothing, and needs no version-stamped reconcile. Across FFI a stream is unavoidable — there is no shared memory to read (step-04 headline 3) |
| **D12** — keep the `Draft` / `StoreDraft` split | Promote `from_canonical`/`rebase`/`orphan` into `Draft` | `Draft` is the FFI surface and shells never call the plumbing. Four shells, zero friction (step-01 Q1) |
| **D13** — `Constraint::Required` stays in the same enum, prepended at the field layer | A separate field-metadata channel | A value type cannot know whether its field is `Option<_>`, but shells want one uniform list to derive affordances from. Three shells did exactly that with no constraint literal leaking (step-01 Q3) |
| Failed submit returns the draft (D4/D5) | Submit consumes the handle on every outcome | Losing the user's edits on a rejected submit is data loss (step-01 F3) |
| **D14 (C19)** — `rebase` compares `theirs` against `base` first: an unmoved canonical never conflicts | Guard in the generated `Draft::rebase`, skipping fields whose canonical equals their base | Post-freeze amendment (step 07). The store rebases every field on every canonical change, so a dirty `name` conflicted whenever the server moved `email` — offering "take theirs" over the user's own ancestor. Putting the guard in the core fixes it once and makes `checkout() == adopt(from_canonical(..))`; putting it in generated code means every generator must re-derive it, and none of them would clear a conflict when canonical moves *back* to the ancestor |
| **D15 (C20/C21)** — the draft stash is `{base_version, status, per-field (raw, base)}`; restore is `Store::adopt(D::from_stash(..))`, which rebases onto fresh canonical | Stash `sync` too, or replay `try_set` onto a fresh checkout with no ancestor | Step 07. `theirs` from before a process death names a canonical value the server may no longer hold — restoring it restores a lie, and it re-derives for free on the next rebase. Replaying without the ancestor is worse: a field the server moved while we were dead returns *dirty*, not *conflicted*, and submit silently overwrites the server. The verdict is not stashed either, and C13 + C16 then make the restored draft safe with no new invariant |

## 9. OPEN questions (do not resolve ad hoc — bring to a design session)

Each names the step that owns it. Nothing below blocks Phase 3.

- **Use-after-close must become a typed error** — *step 10 (`bolted-ffi`), and arguably a BoltFFI
  upstream fix.* BoltFFI handles are raw pointers and generated instance methods never consult the
  `closed` flag, so on Kotlin a use-after-close returns stale data and, after allocator churn,
  **silently aliases another live draft** (step 05, H2 — no crash). VISION's ladder forbids framework
  mechanics that can only fail at runtime; a typed `DraftClosed` is the floor. Should `bolted-ffi`
  also emit a `java.lang.ref.Cleaner` backstop so a forgotten `close()` leaks memory until GC rather
  than forever?
- **Should the store hold drafts weakly?** — *step 08.* With `close()` mandatory and no backstop, a
  forgotten handle grows the registry without bound and every canonical change rebases every zombie.
- **Store concurrency model behind FFI** — *step 08.* Evidence from both sides: the browser wants
  `Rc<RefCell>` and used `Store` unmodified; the FFI wants `Send + Sync` and bypassed `Store`
  entirely. One type cannot serve both — parameterise the handle (`Rc` vs `Arc`) or ship two. Step 02's
  three constraints hold and are not up for debate: **`Send` state behind one lock**, **id-keyed
  handles, not `Rc` clones**, and **never emit or call out under the lock**.
- **Stash schema evolution** — *step 10 (`bolted-ffi`) and Phase 4 (`bolted-check`).* The stash is the
  framework's first **untrusted input**: bytes the OS kept while the process was dead, possibly
  written by an *older version of the app*. C01 says a value's raw form roundtrips — so an ancestor
  that no longer parses means the constraints were tightened between releases. Step 07 degrades that
  field to create-flow (no ancestor), which is safe but silent. Should the whole stash be refused?
  Should `bolted-check`'s constraint-semver snapshots make a tightening a *build* error when a stash
  format version does not also change? Introduced by D15, and it is the one question stash/restore
  opened rather than closed.
- **One-shot effects** (focus-first-invalid-field, toasts, **navigation**) — *its own session.*
  Likely `Option<(Request, Generation)>` state + ack, but navigation deserves the session.
- **Process topology for OS-integration surfaces** (daemons, tray, file-manager extensions) — *its own
  spike, after Phase 2.* Where the core runs (embedded vs daemon), how sandboxed extension processes
  reach it (the contract over IPC?), single-instance ownership. Nothing in Phases 1–3 depends on it.
- **A real `Pending` across FFI** — *step 10.* With a synchronous checker, `begin` + `complete` are
  atomic inside one call, so `Pending` is only ever seen on the stream, never by a `snapshot()` caller.
  A spinner bound to a `snapshot()` read needs the split `begin`/`complete` exposed across the
  boundary (which D10 says is the right shape anyway).
- **Codegen dedup by raw type** — *step 09.* Three of the spike's four field-state families are
  structurally identical (`String` raw). Per-value-type stamping is what a macro naturally does; is the
  dedup worth the complexity?

## 10. Prior art

- *Parse, don't validate* (Alexis King) — tier 1's philosophy; commit as the parse moment.
- **nutype** crate — the declaration mechanics for value types (sanitize/validate attrs).
- *Domain Modeling Made Functional* (Wlaschin) — value objects + Result workflows.
- **Crux** — closest relative (TEA over FFI); Bolted differs in the typed contract layer
  (drafts, fields, structured errors) vs serialized view-models and stringly events.
- Working-copy/checkout-commit (git, ORM unit-of-work) — the draft pattern; three-way merge
  with the trivial rule "unmodified takes theirs".
