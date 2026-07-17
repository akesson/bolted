# Bolted — Architecture

**Status: FROZEN (v1.11, step 06; amended steps 07, 08, 09, 10 and the step-12/13/15/16 design passes, the topology design pass, the step-21 planning pass, and the core-evolution design session).** Phase 1 validated this design against
four independent shells — pure Rust, Apple/ARC, Rust/wasm, Android/ART — and step 06 reconciled their
friction logs. Every question that Phase 1 could answer is answered, in §8, with the alternative it
beat. What remains **OPEN** in §9 is genuinely undecided and each item names the step that owns it.

**v1.1** carries step 07's two amendments, both owner-approved before implementation: **D14** fixes a
verified defect in `rebase` (C03 amended, C19 added), and **D15** adds `Store::adopt` and the draft
stash. **v1.2** carries step 08's, which §9 had scheduled for it: **D16** makes the store id-keyed and
lock-free (C17/C18 amended, C22 added) and **D17** moves the resolvers onto `Draft` and adds
`Stashable`. **v1.3** carries step 09's: **D18** gives the async check a contract (`Checked`), and C07
is amended to state the precedence of its own refusals — a rule both spikes had implemented since step
01 and no test had ever checked. **v1.4** carries step 10's four: **D22** makes the FFI layer generated
source, committed and drift-checked; **D23** gives a released draft a typed refusal; **D24** shares one
field-state family per raw type; **D25** makes the declaration parsed once. Step 10 also **corrects §5**:
a proc macro can never emit an FFI surface, because BoltFFI's bindgen reads source text and never sees
expanded Rust. **v1.5** carries the step-12 design pass's two, resolving the §9 questions that blocked
that step: **D26** declines the `Cleaner` backstop — leak-freedom becomes a per-language contract test
over C22's live-draft count, and the use-after-close half stays an upstream filing; **D27** makes the
draft stash a versioned, parse-don't-validate envelope — wholesale refusal only at the parse gate,
per-field salvage inside it, and constraint tightening a build-time event. **v1.6** carries the
step-13 design pass's one: **D28** extends D22 across the language boundary — foreign-language
artifacts (the stash codec, the per-language contract tests) are committed generated source, emitted
by `bolted-ffi-gen` over the one parsed declaration (D25) and byte-compared by the drift check inside
`mise run check`. **v1.7** carries the step-15 planning pass's one, forced by step 14's runtime
evidence: §4/§6's "the GC never frees the Rust draft" is **Kotlin-only** — the C# backend's generated
handle owns a finalizer that reaches the store-side close — and **D26 is amended in place**: its
revisit condition arrived, the decline stands sharpened, and the per-language leak-freedom test is now
*required* to assert its baseline before any GC, so a finalizer can never green a forgotten release.
**v1.8** carries the step-16 planning pass's one: **D29** discharges §9's largest open claim by
rewriting §1 to the store-owned shape the spikes actually shipped — the unwritten
`Feature (State/Msg/Caps/update)` trait is struck and the never-built `command` verb is demoted to §9
— which opens Phase 4. **v1.9** carries the topology design pass's four, closing Phase 5's probe
campaign (steps 18–20): **D30** blesses the daemon-owned store as the second topology — one store,
one owner, every surface attaches; **D31** makes the wire a generated, values-only artifact (priced,
not yet built); **D32** hands daemon lifecycle to the OS and names the steady state — on while any
surface lives; **D33** graduates the `command` verb as a scratch-draft transaction. **v1.10** carries the step-21
planning pass's one: **D34** resolves VISION's "capability coverage" promise **by construction** —
the generated draft entry points take each declared capability as an explicit optional parameter,
the settable slot is deleted, and a forgotten capability becomes a platform compile error while a
`nil` stays a declared, C16-floored absence. **v1.11** carries the core-evolution design session's
four, adjudicating the `design/core-evolution` branch's triage
([core-evolution-triage.md](design/core-evolution-triage.md)): **D35** states the
no-ambient-nondeterminism rule §5 had enforced but never said; **D36** generalizes D9's echo rule —
the frame loop never crosses the FFI; **D37** gives `observe` its semantics — watch-shaped,
coalescing legal on every target, the old `[Pending, Passed]` delivery downgraded to a driver
fact; **D38** adopts the `bolted-http` shape (sans-io contract crate, shell-side adapters). §9
gains the restored interaction-replay item and the parked windowed-collections and `bolted-http`
freeze-gate items. A
freeze is a commitment to a design, not a promise that the design was already correct — the record of
what changed, and why, is the point.

Frozen means: §1–§7 are the contract Phases 3–4 extract and generate against. Changing them is a
breaking change to Bolted, not an edit. The falsifiable claims live in
[CONFORMANCE.md](CONFORMANCE.md) as C01–C23, each with a test.

Read [VISION.md](VISION.md) first for scope and principles — especially the verification ladder,
which every decision below is justified against.

---

## 1. The shape: MVVM over a store-owned core

- **Model** — a store-owned core per feature. A `Store<D>` owns the feature's single canonical,
  always-valid entity and every open draft, keyed by id and lock-free (D16). Canonical state is
  never mid-edit: all editing happens inside draft sessions, and the store's answer to a mutation is
  **data** — the rebase fan-out is a returned `Vec<DraftId>`, an async check is a `CheckToken`
  begin/complete pair (D10/D18), never an in-band callback. Nothing crosses the FFI but these typed
  verbs and the values they carry; there are no messages, no `update` loop. *(§1 once framed this as
  an Elm core with `State`/`Msg`/a pure `update` fn; six spikes shipped none of that and drove
  `Store` + `Draft` directly — D29 rewrote the framing to match the code. Effects-as-data, the good
  half of the Elm framing, survives; see §8's sans-io row.)*
- **ViewModel** — generated per platform from the feature's contract: thin, dumb glue
  binding the contract to `@Observable` (Swift) / `StateFlow` (Kotlin) /
  `INotifyPropertyChanged` (C#). Rust shells (web via Leptos/Dioxus/Silkenweb, Linux-native)
  consume the contract directly as a crate — no codegen.
- **View** — fully native, owned by the app, holds no business logic and **no constraint
  literals** (a max length appearing in shell code is a defect — greppable in CI).

A "view" is any native surface, not just a window: a tray/menu-bar icon, a file-manager
extension, a widget, a CLI — each is just another (often tiny) observer of a feature's state,
driving it back through the same verbs. The main app window has no privileged status in the contract.

Surfaces need not share a process (D30). The default topology embeds the store in the app process —
everything Phases 1–4 built. The moment a product needs surfaces the OS spawns on its own schedule
(a daemon, a file-manager extension, a tray that outlives the window), the store moves into an
OS-managed daemon and **every** surface — the main window included — attaches as a wire client: the
contract crosses the boundary values-only over a generated wire (D31), and the OS owns the daemon's
lifecycle (D32). One store, one owner, always — two live stores over one feature would be two
canonicals, and reconciling them is a merge protocol beyond the field-level keep/take ceiling (§4),
i.e. the perimeter.

The contract a feature exposes has two shipped verbs, plus a third — `command` — designed but not
yet built (D33, see below):

| Verb | Surface | Semantics |
|------|---------|-----------|
| **observe** | read-only, always-valid current state | **watch-shaped** (D37): a consumer is guaranteed the newest state, never every intermediate — coalescing is legal on every target. *How* it is delivered is per-target: a `snapshots()` stream across FFI, a direct read plus a change tick in a Rust shell (§8), the push client over the wire (D31) |
| **draft** | `checkout() -> FeatureDraft` | multi-field edit session: checkout → edit → validate → submit |

A **third verb, `command`** — a session-less single-action mutation (`toggle_paused()`), for
surfaces that structurally cannot host an edit session: a file-manager context menu, a tray toggle.
D29 had demoted it to §9 after six spikes implemented it zero times; the OS-integration campaign
then produced the first real one (steps 18–19), and D33 graduates it with the shape the spike
proved: **a command is a scratch-draft transaction** — checkout → mutate → commit (which
re-validates everything) → close — so tier 3's floor binds session-less mutations by construction,
never by discipline. Its refusals are `CommitError`; `apply_canonical` remains store plumbing for
the canonical-source path (server/sync) and is never a shell-reachable mutation. Macro/DSL stamping
and core packaging wait for the first framework consumer (D33).

**Canonical core state is never mid-edit.** All editing happens inside drafts; `submit` applies one
fully-validated result to the store in a single transition (§4), so the canonical entity only ever
moves between valid states, never through a keystroke. A feature that records its submits therefore
gets a domain log ("profile updated"), not a keystroke log — the precondition for meaningful
replay/time-travel.

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
show progress without owning check logic. The sub-state is state, not an event: a spinner binds to
`pending` *read from the latest snapshot*, never to delivery of the transition — observation is
watch-shaped (D37), so a fast check may legally never show a subscriber its `Pending`, while a long
one shows it at every read for as long as it is in flight.

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

**Capabilities are supplied at checkout, explicitly** (D34). Across FFI, each declared capability is
an optional parameter of the generated `checkout`/`restore` — there is no settable slot and no silent
default, so a shell that forgets a capability does not compile, and a surface that structurally lacks
one (a sandboxed extension without the needed OS access) declares that with `nil`, reviewable at the
call site. The runtime floor for a declared absence is C16 unchanged: the check never runs, and a
dirty pinned field refuses to submit. In-process Rust shells drive the `Checked` protocol directly
and have no generated seam; C16 is their floor too.

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
  a draft is then a mini feature, same stream+operations shape, same generated binding
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

**The store owns the drafts; a handle is a `DraftId`** — `Copy`, monotonically issued, never reused
(D16). `submit(id)` consumes the draft on success, after which the id is not live (`is_live()` false,
no draft access, a second submit is `AlreadySubmitted`); on refusal the draft goes straight back under
the same id (C17). `close(id)` releases the draft, is idempotent, and stops the store rebasing it
(C18).

**`close` is the only release path, on every platform.** An id is not an owner, so nothing reaps a
draft that a shell simply forgets. This used to be asymmetric, and pretending the asymmetry was
harmless was the lie:

| | release path | use after release |
|---|---|---|
| Rust | `close(id)` — an id is not an owner | `None` from `draft(id)` |
| Swift / ARC | `deinit` runs the wrapper's `Drop`, which calls `close` | impossible (ARC), plus the dead id |
| Kotlin | `close()` only — the GC never frees the Rust draft | **must** be a typed error (step 10); today silent UB, upstream's to fix |
| C# | `Dispose()` is the contract; the generated finalizer *may* reach the store-side close at GC's discretion — a safety net, never a release path (v1.7, step 14) | `ObjectDisposedException` — a typed refusal before any native call (step 14) |

Step 05 measured this on ART: an abandoned Kotlin handle is collected while the Rust draft stays
registered forever, an unreachable zombie the store keeps rebasing. Step 14 measured the C# row: a
forgotten, undisposed draft *is* reclaimed — its finalizer reaches the store-side close, proven
against a still-referenced control draft — which is why that row no longer reads like Kotlin's. The
contract does not soften: non-deterministic reclamation is not a release path, and D26's leak-freedom
test asserts its baseline before any GC, so a finalizer cannot green a forgotten `Dispose`. Before D16 the Rust reference
implementation reaped that zombie on `Drop`, so a lifecycle bug written against it surfaced for the
first time on Android. Now the contract reads identically everywhere, `use { }` / `IDisposable` are
the idiomatic wrappers, and BoltFFI's raw-pointer handles make use-after-close silent UB today —
which `bolted-ffi` must close (§9). Note that an *id* has no such hazard: a stale one is simply not
live, which is the mechanism step 10 needs.

## 5. Manifestation: generics for behavior, macros for names, traits as contracts

Hard FFI constraint driving this: **generic methods cannot cross a language boundary** — the FFI
surface must be monomorphic with concrete names. Therefore:

- **Generic framework types** carry all logic (rung 1, written once): `Field<V>`, `Store<F>`,
  `ValidationReport<FieldId>`, single-flight machinery.
- **Derive/attr macros** do only mechanical name-stamping, delegating immediately to the
  generics: `#[bolted::value]` (newtype + `Value` impl + constraint metadata),
  `#[bolted::entity]` (snapshot + draft struct of `Field<V>`s + `FieldId` enum + monomorphic
  `try_set_name(...)` methods), `#[bolted::rules]`. Thin macros are a verification-ladder
  requirement: macro output is the least-verifiable code, so it must stay trivial.
- **The FFI layer is generated *source*, not macro output** (D22), and it is the one thing a macro
  *cannot* do: BoltFFI's bindgen reads the crate's source files with `syn` and never sees expanded
  Rust, so a macro-emitted `#[data]` is silently omitted from the bindings. See §6.
- **Traits** are the contracts: `Value` (Raw / Error / try_new / into_raw / constraints) and
  `Draft` (FieldId / conflicts / validate / commit) with its subtraits `Stashable`, `Checked`,
  `StoreDraft`. There is no `Feature` trait — the Elm-style `Feature (State / Msg / Caps / update)`
  this list once sketched was struck by D29, and the name now belongs to `bolted_decl::Feature`, the
  declaration model (D25).

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
    fn resolve_keep_mine(&mut self, field: Self::FieldId);      // D17
    fn resolve_take_theirs(&mut self, field: Self::FieldId);    // D17
    fn commit(self) -> Result<Self::Entity, (Self, CommitError<Self::FieldId>)> where Self: Sized;
}

pub trait Stashable: Draft {                                    // D17 — optional; process death
    type Stash: Clone + PartialEq + Debug;
    fn stash(&self) -> Self::Stash;
    fn from_stash(stash: &Self::Stash) -> Self where Self: Sized;
}

pub trait Checked: Draft {                                      // D18 — optional; async checks
    type CheckId: Copy + Eq + Debug;
    fn begin_check(&mut self, check: Self::CheckId) -> CheckToken;
    fn complete_check(&mut self, check: Self::CheckId, token: CheckToken,
                      verdict: Result<(), ErrorData>) -> bool;
    fn check_state(&self, check: Self::CheckId) -> &CheckState<Result<(), ErrorData>>;
    fn check_pins(check: Self::CheckId) -> Self::FieldId;       // C13's "value-bound" names this field
}

pub enum CommitError<FieldId> { Validation(ValidationReport<FieldId>), Conflicted { fields: Vec<FieldId> }, Orphaned }
pub enum SubmitError<FieldId> { Validation(..),                        Conflicted { .. },                  Orphaned, AlreadySubmitted }
```

**The store ships no lock** (D16). `Store<D>` owns its drafts, so it is `Send` whenever `D` is, and the
shell chooses the sharing discipline: a Rust shell holds it by value, `bolted-ffi` holds it behind one
`Mutex`. Mutations return their fan-out as data (`Vec<DraftId>`) rather than calling out to a
subscriber — sans-io, applied to the store, and what lets a shell obey "never emit or call out under
the lock" without the core knowing that locks or streams exist.

`Draft` is the FFI surface and stays minimal, but *minimal* means what shells call, not what is
convenient: the resolvers were inherent methods on the concrete draft until step 08, invisible to
anything generic, though every shell called them across the boundary (D17). The same was true of
`begin`/`complete`/`state` for an async check until step 09 (D18). The store-facing plumbing —
`from_canonical` / `rebase(entity, version)` / `orphan` / `is_based` — sits on a `StoreDraft: Draft`
subtrait that no shell ever calls (§8, D12). `AlreadySubmitted` is the one failure an *id* can have
that a draft cannot, which is why the two enums differ by exactly that variant.

**Thin macros, in practice.** Writing `bolted-macros` (step 09) is what put teeth in "generics carry
behavior". Three judgements were about to be emitted per feature, and each moved down into the core
instead: `Field::required_error` (the `Unset` → `required` decision, D13), `commit_gates` (C07's three
gates, in order), and `SingleFlight::violation` (C13 + C16's whole payload). The rule the generated
code now obeys is checkable, and is checked: **no `match` over a `Validity`, no `if` that decides
whether a commit is refused, no re-derivation of single-flight sequencing.** A macro that reaches for
one of those shapes has moved the design's most consequential judgements to its least verifiable code.

**Crate layout** (physicalizes VISION's narrow-coupling promise):

```
bolted-core         all traits + generic types; sans-io, and lock-free; NEVER depends on boltffi
bolted-conformance  C01–C22, generic over a feature; the executable form of CONFORMANCE.md
bolted-decl         the declaration model + its parser; read by BOTH emitters below (D25)
bolted-macros       the derives (value, entity, rules); output = thin delegation to bolted-core
bolted-ffi-gen      declaration -> Rust source text for a feature's FFI layer (D22); no boltffi dep
bolted-ffi          shared #[data] DTOs; hand-written; the ONLY crate importing boltffi
bolted-check        build-time analyses (drift, coverage, constraint semver)

<feature>-ffi        GENERATED, committed src/generated.rs + hand-written src/custom.rs; imports boltffi
```

`bolted-decl` exists because two emitters would otherwise be two contracts, and the drift check would
be comparing a generator against itself (D25). `bolted-ffi-gen` is not a macro for the reason §5 gives:
bindgen cannot see macro output. `<feature>-ffi/src/lib.rs` must `pub use generated::*;` — under
`boltffi pack`'s expansion mode the whole-crate metadata blob resolves every exported type **from the
crate root**, and `mise run check` cannot see that failure.

`#[bolted::feature_model]` is **cut** (D21) and could never have existed: bindgen reads source text, so
a macro's `#[data]` output is silently invisible. The `Feature` trait it would have stamped was never
written and is now **struck** (D29); §1 describes the store-owned shape that shipped instead.

**Sans-io / runtime-agnostic core**: effects are data driven by the platform layer; no tokio in
`bolted-core`. This is what makes headless deterministic tests and wasm32 compatibility
structural rather than aspirational. The rule extends to **all ambient nondeterminism**: no clocks,
no randomness, no ambient identity sources inside core crates — time and identities arrive as
inputs (verb arguments or effect completions; the `CheckToken` begin/complete pair is the pattern
in shipped code). The store's own monotonic counters (`DraftId`, the check sequence) are not an
exception but the point: they are pure functions of the input sequence, which is the property the
rule protects — core state evolution is a deterministic function of its inputs, which is what
keeps interaction replay (§9) possible (D35). Enforced by the workspace `clippy.toml` deny-list
(`SystemTime::now` / `Instant::now`) riding `-D warnings`; `bolted-check` may widen it later.

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
- **The frame loop never crosses the FFI** (D36) — the echo rule generalized to every continuous
  interaction. Rendering runs at frame rate from natively held state; the core hears *events*, not
  frames. Continuous gestures (slider, drag-reorder) keep the value native while live and commit at
  interaction boundaries; scroll fetches by overscan + threshold refetch, never per-frame calls.
  Sparse snapshots animate via native implicit animation. Step 05's measurement justifies the
  boundary being exactly here: 12–13 µs per crossing against a 1.0 ms bar pays easily for
  per-*event* crossings and would still be the wrong bet per-*frame*, where FFI + encode + diff
  would be spent against an 8.3 ms ProMotion budget. In the other direction, core-driven churn is
  conflated at the driver — legal because observation is watch-shaped (D37) — so a burst of rebases
  costs one repaint of the newest state, never a queue drain.
- **GC languages (Kotlin, C#)**: no deterministic destruction, so `close()` cannot be optional. Since
  D16 it is not optional anywhere — an id is not an owner. What forgetting it costs differs per
  backend (v1.7, step 14): on Kotlin it leaks a Rust draft the store keeps rebasing forever (measured
  on ART, step 05); on C# the generated finalizer reaches the store-side close, so the leak lasts
  until some future collection — non-deterministically reclaimed is not the same as safe, and
  `Dispose` stays the contract. See §4.
- **JNI is the performance worst case**, not Swift. Measured (step 05, emulator): a per-keystroke
  `try_set` + `snapshot` round-trip costs **12–13 µs** against a 1.0 ms bar, ~1.5–2× Apple's on the
  same host. The per-keystroke bet holds; no shell-side write buffer is needed. *Re-check on physical
  hardware in step 07 — an emulator on an arm64 host is the right VM and the wrong CPU.*
- **Process death (Android)**: core-side drafts die with the process, so a draft flattens to raw data
  and comes back through `Store::restore` (D15, C20/C21, §4). The shell decides when — Android's
  `SavedStateHandle`; nothing else has to.
- **Rust shells** (web, Linux-native): consume `bolted-core` + feature crates directly; zero
  FFI; the web target also enforces `wasm32-unknown-unknown` discipline on the whole core.
- **Daemon topology (D30–D32)**: activation is per-OS in mechanism, identical in shape — launchd is
  one C call (`launch_activate_socket`, no env protocol), systemd is one env protocol
  (`LISTEN_PID`/`LISTEN_FDS`, fds from 3, no call); both hand a shared serve loop its
  `Vec<UnixListener>` (~40 lines per OS, measured in step 20). Single instance is the launchd label
  / systemd unit identity — never a lock file. `connect(2)` success is **not** daemon liveness under
  socket activation, on either OS: clients open-then-verify (ping before believing). Path wrinkles,
  priced: launchd does not expand `$HOME` in `SockPathName` (a signed bundle must bake a per-user
  absolute path at assembly time); systemd leaves the socket *file* behind on socket-unit stop
  unless `RemoveOnStop=yes`; user units get `%t` (`$XDG_RUNTIME_DIR`) expansion for free. Peer
  authentication beyond same-user filesystem permission on a user-private directory is out of v1's
  scope (D30).
- **BoltFFI's bindgen reads source text, not expanded Rust.** It `read_to_string`s the crate's files
  and parses them with `syn`, walking `mod` declarations. A `#[data]` emitted by a proc macro, or
  `include!`d from `OUT_DIR`, is **silently absent** from the generated bindings — `boltffi generate`
  exits 0. Two further silences: `generate` will happily emit Swift for Rust that does not compile,
  and a crate can pass `cargo build` *and* `generate` and still fail `pack` (see §5's root re-export).
  Each is a missing rung on the ladder, and each is `bolted-check`'s brief. Measured in step 10:
  `docs/steps/artifacts/step-10-boltffi-visibility/`.

## 7. Invariants — the conformance suite

The design's falsifiable claims, C01–C22, are stated normatively in **[CONFORMANCE.md](CONFORMANCE.md)**
and exist as generic functions (`c01_*` … `c22_*`) in `crates/bolted-conformance`, stamped into tests
by `*_suite!` macros. **Four** features implement the fixture traits — `spike-profile` (rule, async
check, composite value) and `spike-note` (none of them), plus `gen-profile` and `gen-note`, which
declare the same two features through `bolted-macros` — because a suite with one implementor is a
suite shaped like its implementor, and a macro with one input is a macro shaped like it. A drift test
parses the document and fails the build if an ID has no function, a function has no ID, or a function
no macro stamps: the mapping is verified by the build, not by review (VISION rung 3).

In one line each: **C01** value roundtrip · **C02** a clean field follows canonical · **C03** a dirty
field whose canonical moved is never silently overwritten · **C04** convergent rebase is clean ·
**C05** revert-for-free · **C06** no stale-value submit · **C07** commit is the parse moment, each
refusal typed, and orphaned outranks conflicted outranks validation · **C08** rebase re-runs tier 2 ·
**C09** resolution semantics · **C10** latest check
wins · **C11** deletion orphans · **C12** create-flow never rebases, and an ancestor anywhere means
entity-backed · **C13** verdicts are value-bound · **C14** auto-converge on edit · **C15** the base
version tracks the rebase · **C16** an unrun check blocks a dirty field · **C17** submit releases the
draft · **C18** release is explicit, idempotent, and the only path · **C19** rebase is a three-way
merge, and idempotent · **C20** a draft stashes to raw data and restores from it · **C21** restore is
a rebase · **C22** "exists" and "rebases" are different questions.

Not every feature owes every invariant: C08 presupposes a tier-2 rule, C10/C13/C16 an async check.
Step 10 emits the suite as per-language contract tests.

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
| **D16 (C17/C18/C22)** — the store owns its drafts in a `BTreeMap<DraftId, _>`, ships **no lock**, and returns its fan-out as data; handles are `Copy` ids | An `Rc<RefCell<Store>>` RAII handle that closes on `Drop` (keeping C18's old Drop clause); or keep two hand-written stores | Step 08, answering §9's store-concurrency question. Phase 1 wrote the loop three times and the copies had already drifted (C22). One `Store`, `Send` by construction, serves both a lock-free Rust shell and the FFI's single `Mutex` — step 02's three constraints become structural rather than remembered. The RAII alternative was *tried*: `LocalHandle::drop` must take the `RefCell` to reach the store, and ordinary safe code (`let g = store.borrow_mut(); drop(handle);`) panics; `try_borrow_mut` leaks instead. A framework mechanic that can only fail at runtime is rung 4, which VISION forbids. The price is real and named in C18: `close(id)` is now mandatory in Rust too |
| **D17** — `Draft` carries `resolve_keep_mine`/`resolve_take_theirs`; `Stashable: Draft` carries `type Stash` | Leave both as inherent methods and let the conformance fixture supply them as function pointers | Step 08. Making the suite generic is what exposed it: every shell calls the resolvers across the FFI, yet nothing generic could reach them. D12's principle is that `Draft` is what shells call — and shells call these. `Stashable` is a subtrait because `from_stash` needs `Sized` and because a feature whose process cannot die owes no stash. The alternative makes the fixture trait a mirror of `spike-profile`'s inherent API, and step 10's generated contract tests would inherit that shape |
| **D18** — the async check is a core subtrait, `Checked: Draft`, id-keyed by a concrete `CheckId` enum | Leave `begin`/`complete`/`state` as inherent methods the macro stamps per feature; or defer to step 10 | Step 09, answering §9. Four independent consumers — two shells, `spike-profile-ffi`, and step 08's `AsyncCheckFeature` — had each re-derived the same three methods, and the fixture carried a comment saying a later step should promote them. When that happens the contract is missing a name, not the consumers a convention. It is D17's argument one layer down, so it gets D17's answer: a `CheckId` enum is monomorphic and crosses FFI exactly as `FieldId` does; `check_pins` is what lets a generic state C13 ("verdicts are value-bound" — bound to *which* field?). Deferring would have step 10 design a core trait while writing codegen against it, with generated bindings as its only evidence. The fixture lost four members and not one test changed |
| **D19** — "codegen dedup by raw type" is **dissolved**; the residue is reassigned to step 10 | Build a cross-field dedup pass into `#[bolted::entity]` | Step 09, answering §9. In Rust there is nothing to dedup: generics already key on the axis that varies. `FieldStash<R>` keys on the **raw**, so `ProfileStash` shares one `FieldStash<String>` across three fields with no analysis at all; `Field<V>` keys on the **value**, and `Field<Username>` must differ from `Field<PersonName>` because they parse differently. `#[bolted::entity]` never emits a field-state family, so no dedup pass could exist for it. The near-duplication the question was about is entirely in `spike-profile-ffi/src/dto.rs`, where `#[data]` forbids generics — a `bolted-ffi` question, with the cost already measured in that file. The alternative adds the first cross-field analysis to a macro whose doctrine is that it stays readable at a glance (§5) |
| **D20** — `#[bolted::value]` is a **newtype** DSL that also derives the `ErrorData` bridge; composite values keep a hand-written `Value` impl | A full DSL including composites; or thin wiring over user-written `sanitize`/`validate` fns | Step 09. §5's sketch says "newtype", and `DateRange::try_new` is one `start <= end` comparison no DSL improves; a composite shape justified by exactly one example is a guess. The `From<Error> for ErrorData` block is the most repetitive thing in a hand-written value type and is pure name-stamping (variant → snake_case key, named fields → params) — generating it is most of why the macro pays for itself, so "thin wiring" leaves the boilerplate the doctrine exists to delete. `custom(..)` takes `key`/`variant` overrides so that a generated feature keeps the l10n keys its shells already ship; `len_chars` does not, and a uniform DSL therefore renames `spike-note`'s `blank` to `too_short` — a real migration cost, recorded rather than hidden |
| **D21** — `#[bolted::feature_model]` is **cut**; `Feature` is not designed here | Design `State`/`Msg`/`Caps`/`update` now; or emit `#[boltffi::data]` as opaque tokens without linking boltffi | Step 09. `bolted-macros` may not import boltffi (§5 makes `bolted-ffi` the only crate that does, and that seam is the swappable one), and the `Feature` trait has never been written in any of five spikes — it is a sketch in §5 and nowhere else. Emitting boltffi tokens blind is possible and **untestable** inside `mise run check`: the only crate that could compile the output is `spike-profile-ffi`, step 10's rewrite target. Designing `Feature` from zero evidence is a design session, not an implementation step. That the Elm half of §1 has no code behind it after five spikes is itself a finding — §9 now says so. **Step 10 amends the reasoning, not the verdict: a macro emitting `#[data]` tokens imports nothing, so the boltffi-dependency argument was beside the point. `#[bolted::feature_model]` was never *possible* — bindgen reads source text and would silently ignore its output (§6).** The right conclusion was reached from the wrong premise |
| **D22** — the FFI layer is **generated source**: `mise run gen:ffi` writes a committed `<feature>-ffi/src/generated.rs`, and `mise run check` regenerates and compares | A proc macro; a `build.rs` writing into `src/`; fix boltffi upstream first; or hand-write the FFI layer forever | Step 10. A macro **cannot work** (§6): bindgen never sees expanded Rust, and omits it silently. `build.rs` into `src/` makes `cargo build` mutate its own inputs and races `cargo fmt`. Waiting on upstream blocks Phase 3 on someone else's release. Committed source is not the consolation prize it looks like: §5 calls macro output the least-verifiable code on the ladder, and a formatted, reviewable, diffable file gets rustc, `clippy -D warnings` and a diff in code review — three rungs a macro's output gets none of. The price is a drift check, which is rung 3 and hermetic (source text in, source text out). It found a real bug immediately: `clippy::clone_on_copy` on a `Copy` wire type, invisible in an expansion |
| **D23** — a mutating verb on a released draft returns a typed `DraftClosed`; observers stay total | Make the FFI draft an id the store resolves, so a stale handle simply finds nothing; or probe-and-defer | Step 10, answering §9. Two hazards look alike and are not. **(a)** C17's submit releases the draft store-side while the foreign object lives on — ours, and a silent `Ok(())` for `trySetUsername` after submit is the framework lying about a write. Mutators (`try_set_*`, `resolve_*`, `run_*_check`) now refuse; `snapshot`, `validate`, `stash`, `is_live` stay total because a shell must be able to *ask* whether a draft is alive without catching an exception. **(b)** a foreign object released by Kotlin's `close()` frees the Rust object; a later call is a dangling pointer, and the generated Kotlin holds a `__boltffi_closed` flag it never reads. **(b) is not fixable from our side** and is filed upstream |
| **D24** — one field-state DTO family per **raw** type, hosted in `bolted-ffi` (`TextValidity`, `TextFieldSync`, `TextFieldState`); error types stay per **value** | One family per value type (what `spike-profile-ffi` hand-wrote) | Step 10, answering §9's dedup residue (D19). `#[data]` forbids generics, so the FFI must monomorphize what `Field<V>` keeps generic — but the axis it must key on is `V::Raw`, not `V`: a validity is `Valid(Raw)` or `Invalid { raw, error }`, and only the *error* differs per value. Three text fields collapse from nine types to three. Errors do not collapse: `UsernameError` and `EmailError` have different variants, and merging them would give Swift a `case tooShort` on an email. The measurement is in `step-10-surface-delta.md` — 11 declarations become 3, and every shell change is a rename |
| **D25** — the declaration is parsed **once**, by `bolted-decl`; `bolted-macros` and `bolted-ffi-gen` are both emitters over it | Each emitter parses the source it needs | Step 10. Two parsers are two contracts, and they disagree in ways nothing catches: `len_chars(min = 0)` raises no `TooShort`, so an FFI generator that re-derived the variant list would emit a `UsernameErrorFfi::TooShort` its own `From` impl could never construct — and rustc would accept it. Worse, the drift check would then compare a generator against itself. The one shared `ValueDecl::error_variants` is the whole argument. Undeclared value types (composites, D20) are not guessed at: the generator emits `use crate::custom::*;` and names the types it needs, so a missing one is a **compile error** (rung 2), not a binding that quietly lost a field |
| **D26** — **no `Cleaner` backstop.** Leak-freedom is a **contract test** over C22's live-draft count (teardown returns the count to its baseline, per language), `close()` in `onCleared()` is the tested Kotlin rule, and the use-after-`close()` half stays an upstream filing | Emit a Kotlin layer that registers a `java.lang.ref.Cleaner` per handle, so a forgotten `close()` leaks until GC rather than forever | Step-12 design pass. Three reasons, in order of hardness. **Ownership**: the handle class, its `__boltffi_closed` CAS and its free shim are BoltFFI bindgen output (step 05 read them, step 11 M0 re-read them) — a Cleaner registered from outside that class must reach the free shim while holding no reference to the object, i.e. bypass the idempotence guard, trading a deterministic leak for a nondeterministic double-free and coupling Bolted to upstream internals. **Doctrine**: it is D16's rejected mechanic again — a framework device that acts only at runtime, at GC's discretion; under a Cleaner a forgotten `close()` *passes every test* that does not provoke a collection, which absolves the exact bug C18 exists to make loud. **Coverage**: the dangerous half, H2's use-after-`close()` UB, a Cleaner cannot touch — freed-at-GC dangles exactly like freed-at-close. The only real fix is generated methods consulting the flag they already carry, which is upstream's flag and upstream's filing (step 12 drafts it). What Bolted owns is the store side, and C22 already counts it — so the backstop Bolted ships is *detection at the contract-test tier*, not absolution at runtime. If upstream grows an opt-in Cleaner inside bindgen, where the CAS makes it safe, revisit. *Revisited (v1.7, step 14): the condition arrived — the C# backend ships exactly this shape, a finalizer over the CAS-guarded `Dispose`, and it does reach the store-side close at runtime. The decision stands, sharpened rather than reversed: a bindgen-owned finalizer is a welcome safety net but is not a release path, and the per-language leak-freedom test is now **required** to assert its baseline immediately after deterministic release, before any GC — under a finalizer, a test that provokes (or merely tolerates) a collection would pass with every `Dispose` deleted. The Kotlin decline is unchanged; nothing Bolted ships registers a Cleaner* |
| **D27** — the stash envelope is **versioned, parse-don't-validate data**: the schema version is stamped into the *generated* stash DTO from the declaration; an envelope that fails the version/shape gate is refused **wholesale and typed**; inside a parsed envelope, restore salvages **per field** (step 07's degradation stands) and never refuses; constraint *tightening* is a **build-time** event — `bolted-check`'s constraint-semver snapshot (Phase 4) fails the build until the team makes a version decision | Refuse the whole stash whenever any field is stale; or bump the version on every constraint change so old stashes die at the gate; or the status quo — a hand-written shell codec owning an ad-hoc `FORMAT_VERSION` | Step-12 design pass. The raws inside a stash are the user's own keystrokes, and C06 already gives an unparseable raw a home (`Invalid { raw }`) — refusing them all because *one* field's constraints tightened is data loss as policy, the bug live-rebase exists to prevent; so the semantic case keeps per-field degradation (`base` → `None`). The degraded field is then *dirty from unset*, so the next rebase against live canonical must surface it as a **conflict** the UI already renders — a claim step 12 tests (C23) rather than assumes. The *structural* case is different: an envelope that cannot be parsed has nothing to salvage, and refusing it wholesale is just tier 1 applied to the envelope. The deciding fact is where today's only version gate lives: `StashCodec.kt`, a hand-written shell file that step 12's codec-deletion item would otherwise silently delete — the version therefore moves into the generated stash DTO, and the gate travels with the generated codec. What no runtime path can do is *warn the team* that a tightening happened; that is the build-time rung, and it is `bolted-check`'s (Phase 4). Costs nothing in the core: `Stashable::from_stash` stays infallible — the gate is at the DTO boundary, where the untrusted bytes are |
| **D28** — foreign-language artifacts (the stash codec, the per-language contract tests) are **committed generated source** — D22, one language out. `bolted-ffi-gen` grows per-language emitters over `bolted-decl::Feature` (D25: one parser, another emitter); the emitted Kotlin/Swift lives at a source path the platform build already compiles, carries the `@generated` banner, and is **byte-compared** by the drift check inside `mise run check`. Emission is string-building in plain Rust — no template engine | Generate at platform build time (a Gradle task / SPM build-tool plugin writing into `build/`); a template engine over `.kt.jinja` files; wait for upstream filing 04 (public DTO wire ser/de) to land; keep hand-writing the foreign files | Step-13 design pass. Step 12 proved the need three ways at once: the codec deletion, the checker helper and the Sendable extension all converted for the *single* reason that the generator emits only Rust. Build-time generation loses everything D22 was chosen for — the output escapes review, no compiler judges it inside `check`, and the generator moves behind Gradle/Xcode, so the one verb every machine runs stops seeing drift. Byte-comparison is honest here in a way it could not be for Rust *because nothing else owns these files*: no formatter rewrites them (rustfmt forced D22 to compare code, not bytes), and the check environment has no Kotlin/Swift parser — pretending to normalise would mean maintaining a second grammar. A template engine is a second source of truth with no compiler on it, and the askama-class tooling already cost this repo a CLI-install workaround (step 02). Waiting on filing 04 blocks Phase 3 on someone else's release — D22 rejected that once — and it retires only the codec, never the contract tests |
| **D29** — §1 is rewritten to the **store-owned** shape that shipped; the unwritten `Feature (State / Msg / Caps / update)` trait is **struck**, and the never-implemented `command` verb is demoted to §9 | Design `State`/`Msg`/`Caps`/`update` now so a Phase-4 harness has a trait to sit on; or leave §1 as aspiration and reconcile it after Phase 4 | Step-16 planning pass, discharging §9's "largest undischarged claim in the architecture". Six spikes and four shells drive `Store` and `Draft` directly and pass C01–C23 — the code has *been* the design since step 01, and §1's Elm core was the drift. Designing the trait now would make the spikes retroactively wrong and would design `State`/`Msg`/`update` from **zero examples**, the D20/D21 error twice-affirmed; the name `Feature` is meanwhile taken by `bolted_decl::Feature` (D25). Effects-as-data survives — it is what the sans-io row proved (`spawn_local` → a bare `CheckToken`) — so what is struck is the update-loop trait nothing ever implemented, not the effect model. The `command` verb goes to §9 for the same reason: zero of six spikes needed a session-less mutation, and a shape justified by no example is a guess (D20). D21's row stands as the historical record that first flagged this; this row closes it |
| **D30** — the store has **one owner**; the **daemon-owned** topology is blessed as the second deployment shape. When any surface lives outside the app process, the store moves into an OS-managed daemon and *every* surface — the main window included — attaches over the wire; the embedded topology stays the default for single-process products. Single instance is the OS's (launchd label / systemd unit identity), never a lock file; the socket is guarded by same-user filesystem permission on a user-private directory, and authenticated/hostile-peer surfaces are out of v1's scope | A hybrid — the UI embeds the core while a daemon owns a second store for background surfaces; or always-daemon, even for plain windowed apps; or per-surface embedded cores reconciled through storage | Topology design pass, closing §9's process-topology bullet on the Phase-5 evidence (steps 18–20). The daemon arm is priced and cheap: the contract crossed a Unix socket **values-only** with no framework crate touched (H1), a sandboxed Developer-ID Finder extension reached the group-container socket with **zero prompts** (R2/G3, EPERM controls run first), a full SwiftUI editor ran the whole contract — echo rule, conflicts, async check, stash-restore across a real `kill -9` — at ~100 µs/keystroke against a 16 ms bar (U1–U5), and the same topology re-confirmed **byte-unmodified** under systemd (P1, L1–L5). Every hybrid is two canonicals: "canonical is never mid-edit" is a statement about *the* store, and reconciling two of them is a merge protocol — the perimeter §4 already refuses. Always-daemon inverts the cost: a plain windowed product would pay a process boundary for nothing, and Phases 1–4 prove embedded needs no wire. The latency numbers say either topology is affordable (26–150× headroom everywhere measured), so the decision is about state ownership, exactly as step 18 framed it. Single-instance refusals were recorded verbatim on both OSes (A2/S4, L2); peer auth beyond same-user was priced in step 18 (audit-token → code-signing pushes toward XPC for authenticated surfaces) and deliberately deferred |
| **D31** — the contract crosses process boundaries **values-only over a generated wire**: `bolted-ffi-gen` grows wire emitters (the D22/D28 road) — per feature a Rust wire crate + daemon plumbing, per language a mirror + **two client shapes** (blocking, push-demultiplexer), all committed generated source; the generated client library owes **open-then-verify** and the **continuous-stash idiom** unconditionally. Priced now ([the price list](steps/artifacts/topology-wire-pricing.md)), emitted as its own step when the first product feature needs the daemon topology | Hand-written wire per product; a generic RPC/reflection layer shared across features; or building the emitter in this pass | Topology design pass. The spike's hand-written wire is the existence proof *and* the price list: 486 lines of Rust protocol + 672 of daemon body per feature, ~600 per foreign language, **zero bolted dependencies** — values-only held to the end (kill criterion 3 never approached): tier-1 refusals crossed with params intact, tier-2/check verdicts crossed as the same `validate()` report an in-process shell reads, single-flight held with the driver in another process (B2, watched red first). Hand-writing that per product is exactly the glue-fails-at-runtime VISION forbids; a generic RPC layer must either serialize judgements (kill criterion 3) or reflect (rung 4, out permanently). Building the emitter now would design it against one consumer that is a disposable spike — the D20 discipline — so the requirements are banked instead: `CheckToken` never crosses (correlation-id registry), verdicts as closed data (declared `failed_key`), object shapes not serde tuples, tick versions make un-serialized push ordering safe, `AlreadySubmitted` flattening to `UnknownDraft` at the connection-ownership gate ruled an acceptable *transport* refusal, the stash blob client-kept and re-entering through D27's gate |
| **D32** — daemon lifecycle is **OS-owned at rung 3**: socket activation (launchd `Sockets` / systemd socket units) + label/unit identity + idle-exit, from generated plists/units; no `KeepAlive`, no `Restart=always`. The steady state has a name — **on while any surface lives**: a persistent surface holding a connection keeps the daemon resident, a crashed daemon is resurrected by the next connect (surfaces run reconnect loops over open-then-verify), and a product with no live surface pays nothing | `KeepAlive`/`Restart=always` (an unconditionally resident daemon); client-managed spawn with lock files; or forcing idle-exit under persistent surfaces | Topology design pass. All three probes bought the same bargain at the same price: activation, single-instance and respawn-on-next-connect came from ~20 lines of configuration per OS, exercised as A1–A4, S1–S4 (SMAppService: **zero** approval steps) and L1–L4 (systemd; the user-unit posture priced at zero ceremony too). Step 19's M4 finding is the heart of the naming: with a FinderSync extension holding a connection, idle-exit never fires — that is the *intended* semantics, not a defect, because a surface that exists is expressing demand; forcing idle-exit under it would churn kill/respawn for nothing. `Restart=always` hides crash loops and pays residency with zero surfaces; hand-rolled lock files are the rung-4 single-instance the OS already owns (the verbatim A2/L2 refusals are the evidence). The reconnect loop healed the topology in the probe — kill -9 → the observing extension's next connect respawned the daemon through the socket unit — and step 20 bounded the good case (~45 ms queued-connect accept) while keeping the launchd limbo as the case the client library must survive |
| **D33** — the `command` verb graduates: **a command is a scratch-draft transaction** — checkout → mutate → commit (re-validates everything) → close — with refusals typed as `CommitError`; `apply_canonical` is never a shell-reachable mutation path. The contract rule is law now; macro/DSL stamping and core packaging wait for the first framework consumer (the composite-value posture, D20) | Keep it demoted to §9; a bespoke validate-then-apply path that skips the draft; or a full DSL + core helper designed now | Topology design pass, on the reopening condition §9 itself set ("when a real feature needs a session-less mutation"): the campaign produced one — `toggle_paused`, driven from a Finder context menu (G5, human-executed) and a tray, surfaces that structurally cannot host an edit session. The hand-written shape taught the hazard that decides the design: tier-1 validity is free for a canonical-to-canonical mutation, but **`apply_canonical` runs no tier-2 rules** — a command that skips the scratch checkout can write a canonical no draft could ever submit, and today only discipline prevents it: a rung-4 mechanic, which the founding rule forbids. Routing commands through `commit` makes "submit re-validates everything, always" bind session-less mutations *by construction* and re-derives nothing (§5's rule: the gates live in `commit_gates`, once — a bespoke validate-then-apply path is precisely that re-derivation). Designing the DSL now from one example would repeat D20's error; the blessed shape is not a guess — it is the one the spike shipped and probed (B3 fan-out to open drafts, G5 end-to-end) |
| **D34** — capability coverage is resolved **by construction at the generated seam**: each declared capability (today the one shipped family, the async check's `XChecker` trait) is an explicit **optional** parameter of the generated draft entry points — `checkout(username_checker: Option<Box<dyn UsernameChecker>>)`, same on `restore` — the `set_*_checker` slot and the silent `None` default are deleted, and the driver's "did not run" return survives only for a declared absence (or a reentrant outcall). A forgotten capability is a **platform compile error** (rung 2) at every call site on every FFI target; a `nil` is a *declared, reviewable* absence whose runtime floor is C16 unchanged. In-process Rust shells have no generated seam: C16 stays their floor, and no core API is invented for them (one consumer, D20). Scope: the check family only — no capability registry until a second family exists | A **mandatory** (non-optional) parameter; a rung-3 coverage analysis in `bolted-check` (committed per-target manifest + source-text scan for wiring tokens inside `check`); or the status quo (settable slot + C16 as the only floor) | Step-21 planning pass, on the topology campaign's capability evidence. The status quo is glue that fails only at runtime: nothing forces `set_*_checker`, the driver no-ops with `Ok(false)`, and the omission reaches the user as C16's submit refusal with no in-app fix — precisely what the founding rule forbids where a compiler could catch it. The mandatory parameter reads stronger on the ladder but is wrong on the spike's evidence: surfaces are heterogeneous (step 19 — the capability is the surface's *own* OS access), so a surface that structurally cannot implement a check would be forced to fabricate a stub, and a stub's lying `Pass` on a dirty field is strictly worse than C16's typed refusal — D6's logic again (don't demand what the surface cannot honestly supply). The manifest+scan polices a state better made unrepresentable, verifies only token presence, and adds an apparatus where the generator already owns the seam — so the planned rung-3 analysis **dissolves** (the D19/KC2 pattern). The wire inherits the shape client-side: the checker never crosses (D31 req. 1), so the generated wire client's checkout owes the same explicit parameter |
| **D35** — no ambient nondeterminism in core crates: time and identities arrive as inputs; enforcement is the workspace clippy deny-list riding `-D warnings` | A `Ctx` argument stamped by the driver at dispatch (the branch snapshot's shape); or allow ambient calls and mock the clock in tests | Core-evolution design session (triage T3c). The rule was already true of the shipped code (`CheckToken` takes completions as inputs; D16's ids are monotonic, not random) and already enforced by the committed deny-list — the architecture just never stated it, so the deny-list justified itself against a §5 paragraph that existed only on the branch snapshot. `Ctx` died with the `update` loop (D29); a mocked clock makes determinism a test-harness property rather than a structural one. The OS-integration campaign validated the enforcement shape in passing: the spikes' three genuine wall-clock timing sites took local `#[allow]`s rather than weakening the rule. No C-ID is minted: the property as stated is static and the deny-list is its rung (build-time); the runtime-testable face — same input sequence ⇒ identical snapshot sequence — is replay's first artifact when §9 ever schedules it |
| **D36** — the frame loop never crosses the FFI: the core hears event boundaries, not frames (§6) | Per-frame gesture/scroll round-trips through the core | Core-evolution design session (triage T3b). The generalization of D9's echo rule, justified by the same measurement read at the right grain: 12–13 µs per crossing (step 05) makes per-event crossings free and per-frame ones a bet of the 8.3 ms ProMotion budget on FFI + encode + diff — TCA's best-documented pitfall (per-frame reducer round-trips), designed out rather than mitigated |
| **D37** — `observe` is watch-shaped: the contract guarantees the newest state, never every intermediate; coalescing is legal on every target | Queue-shaped delivery (every snapshot reaches every subscriber); or a split contract — coalesced state plus a guaranteed-delivery sub-channel for check-state transitions | Core-evolution design session (triage T1). This is the industry-standard state semantics — `StateFlow` conflates by specification, SwiftUI renders latest-per-frame, `INotifyPropertyChanged` has no value queue, `tokio::watch` is the name — and three of four binding targets structurally cannot deliver more: the Android shell already pipes the stream into a `MutableStateFlow`, so the old `[Pending, Passed]` delivery never survived to a UI anyway. Queue-shape also forces a backpressure choice (unbounded buffer, or a core blocked on its slowest subscriber) that the watch shape dissolves — for the wire's push client (D31) exactly as for the in-process stream. The split alternative misreads the standard two-channel pattern: the industry splits state from *events*, and check sub-state is state by the litmus test — a consumer that missed the emission is still correct by reading current state; genuinely event-like needs (announce a verdict, focus a field) belong to §9's one-shot-effects item. Wins: D7/C15's stamped reconcile simplifies to "read the latest"; both step-02 probes prove BoltFFI implements the shape at 0.27.5. Replay (§9) is unaffected: D35's determinism governs the core's *emitted* sequence — delivery was never part of its claim |
| **D38** — `bolted-http`: a sans-io contract crate plus Bolted-shipped shell-side adapters (URLSession in Swift, OkHttp in Kotlin, WinRT in C#; Rust adapters only for Linux/web) | One Rust client binding the native stacks directly (objc2/windows-rs/JNI, nyquest-style) | Core-evolution design session (triage T7); full design in `crates/bolted-http/docs/`. Android has no credible Rust path to OkHttp/Cronet, so shell-side is the uniform default; adapters are rung-2 shipped components (the effect-side counterpart of a `BoltedTextField`), living in the framework's maintenance envelope with a per-adapter conformance suite; the contract is placement-blind, so Rust-side Darwin/Windows bindings stay possible without a breaking change. The snapshot's recorded retreat — reconsider if step-02's callback measurements came back ugly — closed the good way: callbacks measured cheap and reentrancy-safe (no deadlock, no lock held across an outcall), and both step-02 probes' stream findings at 0.27.5 clear the response-streaming half of the old freeze gate. Scheduling stays a §9 matter: the crate ships no feature until one needs HTTP |

## 9. OPEN questions (do not resolve ad hoc — bring to a design session)

Each names the step that owns it. Nothing below blocks Phase 3.

- **One-shot effects** (focus-first-invalid-field, toasts, **navigation**) — *its own session.*
  Likely `Option<(Request, Generation)>` state + ack, but navigation deserves the session.
- **Composite value objects in `#[bolted::value]`** — *whenever a second one exists.* D20 scopes the
  macro to newtypes, so `DateRange` (raw `(Date, Date)`, invariant across two parts) stays
  hand-written. A composite needs struct-shaped parts, a tuple raw, and a cross-field invariant — a
  second macro shape, currently justified by exactly one example. Do not design it from that one.
  *(Its shadow is now recorded twice: step 09 at the core, step 10 at the FFI, where the setter takes
  one `AvailabilityRaw` rather than two dates.)*
- **Collection observation (windowed) — designed when the first real collection feature lands.** No
  spike has a collection feature, so designing this now would repeat the error D20/D29 twice
  refused: a shape justified by zero examples. The candidate answer is preserved from the
  `design/core-evolution` snapshot: a windowed accessor (`open_window(range)` →
  `WindowSnapshot { version, total_count, range, rows }`) with stable `RowId`s,
  snapshot-authoritative and watch-shaped per window (D37, its precondition, is now decided), and
  deltas never crossing the FFI — the rejected alternative being a `VecDiff`-style delta protocol,
  which is unrepresentable now that coalescing is legal by contract (a delta stream cannot survive
  a dropped intermediate; snapshots don't care). Open with the first feature: what a `Row`
  projection is, where `RowId` comes from (entity key?), whether `open_window` takes sort/filter
  parameters, and the windowing etiquette (overscan, threshold refetch — §6's frame-loop rule,
  D36, already governs the scroll side).
- **`bolted-http` contract freeze** — *reopens when a feature needs HTTP; nothing schedules it.*
  The D38 shape is decided; still genuinely open before a freeze: the cookie capability's shape,
  whether Android's declarative `<pin-set>` binds OkHttp/Cronet, and `BackgroundTransfer` — a
  separate optional effect family whose precondition (effects as durable, serializable data with
  stable identities) is shared with interaction replay (below) and the draft stash; nothing may
  foreclose it. Response streaming, the old gate's hardest question, now has evidence: both
  step-02 probes' stream machinery converges at boltffi 0.27.5.
- **Interaction replay (protected possibility, unscheduled).** The contract boundary is a natural
  record seam: every mutation enters the core as a typed, serializable call (draft verbs, commands,
  canonical pushes, check completions), so logging those calls and re-driving the log against a
  fresh core yields deterministic session replay — bug reports that attach a replayable log,
  time-travel debugging, UI-less integration tests, and the strongest cross-platform conformance
  test available (same log ⇒ identical snapshot sequences on every backend). Not designed and not
  scheduled; the item exists so no other decision forecloses it. Three preconditions, with their
  current state: **(1)** no ambient nondeterminism — the §5 rule (D35), enforced by the clippy
  deny-list; its runtime face (same input sequence ⇒ identical snapshot sequence) would be replay's
  first conformance artifact. **(2)** stable logical identities for handles — structurally
  satisfied since D16: `DraftId`s are `Copy`, monotonically issued, never reused; `CheckToken`'s
  private seq is the same shape. **(3)** a total order over inputs — true behind FFI, where
  `bolted-ffi`'s single `Mutex` serializes every call, and by construction in the daemon topology,
  where the store's one owner (D30) serializes the wire; **not guaranteed for a lock-free Rust
  shell** holding the store by value, so a recording wrapper would have to impose the order it
  logs — acceptable, because recording is opt-in instrumentation, never a core obligation. Replay
  reproduces core state, not pixels: native view state (focus, IME composition, scroll) never
  crosses the boundary and is out of scope by design.

**Closed since the freeze** — kept here so the answers are findable from the questions:
*store concurrency* → D16 · *resolvers on the trait* → D17 · *the async check's contract* → D18 ·
*codegen dedup by raw type* → D19 (Rust half dissolved) and **D24** (FFI half: one family per raw
type) · *use-after-close* → **D23**, for the store-side half only; the foreign-side half is the
`Cleaner` question above · *a real `Pending` across FFI* → **answered by measurement, not by design**:
with a synchronous checker `begin`/`complete` are atomic inside one call, so a `Pending` never reaches
a `snapshot()` caller — it reaches a **stream subscriber**, because the generated check driver pushes a
snapshot between the two halves. *Re-closed under D37*: that emission is a **driver fact, not a
contract guarantee** — observation is watch-shaped, so delivery of the `Pending` may legally coalesce
(on Android it always did: the ViewModel pipes the stream into a `StateFlow`, which conflates below
anything Bolted controls). A spinner binds to `pending` read from the latest snapshot: a long check
shows it at every read while in flight, a fast one may never flash it — which is the better UX anyway.
`gen-profile-ffi`'s `a_check_in_flight_is_observably_pending` still asserts `[Pending, Passed]`,
rescoped to pin the driver's eager emission, not what a shell may rely on ·
*the `Cleaner` backstop* → **D26** (declined: leak-freedom becomes a per-language contract test over
C22's count; the use-after-close half stays an upstream filing) · *stash schema evolution* → **D27**
(versioned envelope, wholesale refusal only at the parse gate, per-field salvage inside it; tightening
becomes a build-time `bolted-check` event in Phase 4) · *the `Feature` trait / §1's Elm framing* →
**D29** (struck: §1 rewritten to the store-owned shape that shipped; the never-built `command` verb
was demoted to §9 — and has since graduated, D33) · *process topology* → **D30** (one store, one
owner; the daemon-owned topology blessed, every surface attaches), with the wire as **D31**
(generated, values-only, priced in
[topology-wire-pricing.md](steps/artifacts/topology-wire-pricing.md)) and lifecycle as **D32**
(OS-owned; steady state "on while any surface lives") — the Phase-5 campaign, steps 18–20, is the
evidence · *the `command` verb* → **D33** (a scratch-draft transaction; DSL/core packaging wait for
the first framework consumer) · *capability coverage* (VISION's harness promise, never a §9 bullet
but answered in the same spirit) → **D34** (by construction: an explicit optional parameter on the
generated draft entry points; the planned rung-3 analysis dissolved).

## 10. Prior art

- *Parse, don't validate* (Alexis King) — tier 1's philosophy; commit as the parse moment.
- **nutype** crate — the declaration mechanics for value types (sanitize/validate attrs).
- *Domain Modeling Made Functional* (Wlaschin) — value objects + Result workflows.
- **Crux** — closest relative (TEA over FFI); Bolted differs in the typed contract layer
  (drafts, fields, structured errors) vs serialized view-models and stringly events.
- Working-copy/checkout-commit (git, ORM unit-of-work) — the draft pattern; three-way merge
  with the trivial rule "unmodified takes theirs".
