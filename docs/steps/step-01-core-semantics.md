# Step 01 — Core semantics prototype (pure Rust)

**Phase 1 · Spike.** Read first: [VISION.md](../VISION.md) (principles),
[ARCHITECTURE.md](../ARCHITECTURE.md) (the design this step validates — §1–§5 and §7 are the
substance), [ROADMAP.md](../ROADMAP.md) (working agreement).

## Goal

Prove the draft/field/store semantics in pure Rust: implement prototype framework primitives
and one hand-written feature, and encode **all 12 invariants of ARCHITECTURE.md §7 as passing
tests**. Also bootstrap the workspace and mise so `mise run check` is the single build entry
point from day one.

Everything here is *as-if-generated*: write by hand exactly what the future macros would emit,
as plainly and mechanically as possible. If a piece of per-field code can't be written
mechanically, that is a design finding — record it in the report, don't get clever.

## Non-goals (hard boundaries)

- No BoltFFI, no FFI of any kind. No macros (`macro_rules!` or proc). No UI. No async runtime
  (no tokio/async-std — see single-flight below). No performance work. No extra features
  beyond the profile spike. No crates published.

## Deliverables

### 1. Workspace bootstrap

```
mise.toml            # [tools] rust pinned to current stable (record the version in the report)
Cargo.toml           # workspace, resolver = "3", edition 2024 members
crates/
  bolted-core/       # prototype framework primitives (generic, no deps except std)
  spike-profile/     # hand-written "as-if-generated" feature using bolted-core
```

mise tasks (the only supported way to build/test):

```toml
[tasks.test]
run = "cargo test --workspace"
[tasks.check]
run = [
  "cargo fmt --all --check",
  "cargo clippy --workspace --all-targets -- -D warnings",
  "cargo test --workspace",
]
```

Conventions: edition 2024; `unwrap`/`expect`/`panic!` forbidden in library code (fine in
tests); `bolted-core` has **zero** dependencies (dev-deps: `proptest` allowed).

### 2. `bolted-core` — prototype primitives

One module per concept; signatures below are the contract (rename/adjust internals freely,
record deviations in the report):

```rust
// constraint.rs — declared metadata (future shell-affordance export)
pub enum Constraint {
    Required,
    LenChars { min: u32, max: u32 },
    Custom(&'static str),          // named, opaque predicate
}

// value.rs
pub trait Value: Clone + PartialEq + Send + Sync + 'static {
    type Raw:   Clone + PartialEq + Send + Sync + 'static;
    type Error: Clone + PartialEq + std::fmt::Debug + Send + Sync + 'static;
    fn try_new(raw: Self::Raw) -> Result<Self, Self::Error>;
    fn into_raw(self) -> Self::Raw;
    fn constraints() -> &'static [Constraint];
}

// field.rs — the workhorse. Validity × sync are INDEPENDENT dimensions.
pub enum Validity<V: Value> {
    Unset,
    Valid(V),
    Invalid { raw: V::Raw, error: V::Error },
}
pub enum SyncState<V: Value> {
    InSync,
    Conflicted { base: Option<V>, theirs: V },
}
pub struct Field<V: Value> { /* validity, sync, base: Option<V> */ }
impl<V: Value> Field<V> {
    pub fn new_unset() -> Self;                    // create flow (no base)
    pub fn from_base(base: V) -> Self;             // checkout of existing entity
    pub fn try_set(&mut self, raw: V::Raw) -> Result<(), V::Error>;
        // ALWAYS records the attempt: Ok → Valid(v); Err → Invalid{raw,error}.
    pub fn value(&self) -> Option<&V>;
    pub fn validity(&self) -> &Validity<V>;
    pub fn sync(&self) -> &SyncState<V>;
    pub fn is_dirty(&self) -> bool;
        // VALUE-based: Valid(v) → v != base; Invalid → true; Unset → base.is_some()
    pub fn resolve_keep_mine(&mut self);           // base := theirs; keep value; InSync (still dirty)
    pub fn resolve_take_theirs(&mut self);         // value := theirs; base := theirs; InSync, clean
    pub fn rebase(&mut self, theirs: V);
        // !dirty → adopt theirs (value+base), InSync.
        // dirty && value == theirs → adopt base, clean, InSync (convergent edit).
        // dirty otherwise → Conflicted{base: old base, theirs} (yours preserved).
        // Already Conflicted → update `theirs` only.
}

// report.rs — errors are data (key + params), never strings
pub struct ErrorData { pub key: &'static str, pub params: Vec<(&'static str, String)> }
pub struct RuleViolation<FieldId> { pub rule: &'static str, pub pins: Vec<FieldId>, pub error: ErrorData }
pub struct ValidationReport<FieldId> {
    pub field_errors: Vec<(FieldId, ErrorData)>,   // tier 1 (Invalid/Unset-required fields)
    pub rule_errors: Vec<RuleViolation<FieldId>>,  // tier 2
}
impl<FieldId> ValidationReport<FieldId> { pub fn is_ok(&self) -> bool; }

// draft.rs
pub enum DraftStatus { Live, Orphaned }
pub trait Draft {
    type Entity;
    type FieldId: Copy + Eq + std::fmt::Debug;
    fn status(&self) -> DraftStatus;
    fn dirty_fields(&self) -> Vec<Self::FieldId>;
    fn conflicts(&self) -> Vec<Self::FieldId>;
    fn validate(&self) -> ValidationReport<Self::FieldId>;   // tiers 1+2, full
    fn commit(self) -> Result<Self::Entity, ValidationReport<Self::FieldId>>;
        // Ok ⇔ all fields Valid, zero Conflicted, zero rule violations, status Live.
}

// single_flight.rs — deterministic async-check modeling, NO runtime.
// Effects are data (sans-io): beginning a check yields a token; the driver (platform layer,
// or the test harness) later completes it. Stale completions are ignored.
pub struct SingleFlight<T> { /* seq: u64, state */ }
pub struct CheckToken(u64);
pub enum CheckState<T> { Idle, Pending { seq: u64 }, Done { verdict: T } }
impl<T> SingleFlight<T> {
    pub fn begin(&mut self) -> CheckToken;                    // supersedes any pending check
    pub fn complete(&mut self, token: CheckToken, verdict: T) -> bool; // false = stale, ignored
    pub fn state(&self) -> &CheckState<T>;
}

// store.rs — prototype only: single-threaded, Rc<RefCell<…>> internally is acceptable.
// (The real concurrency model is decided at Phase 3 — see ARCHITECTURE.md §9.)
pub struct Store<D: Draft> { /* canonical: Option<D::Entity>, version: u64, live drafts */ }
impl<D: Draft> Store<D> {
    pub fn new(canonical: Option<D::Entity>) -> Self;
    pub fn canonical(&self) -> Option<&D::Entity>;
    pub fn checkout(&mut self) -> DraftHandle<D>;             // registers for rebase
    pub fn apply_canonical(&mut self, entity: D::Entity);     // bump version, rebase all live drafts
    pub fn delete_canonical(&mut self);                       // live drafts → Orphaned
    pub fn submit(&mut self, draft: DraftHandle<D>) -> Result<(), SubmitError<D::FieldId>>;
        // refuse on conflicts/orphaned/validation; on Ok: canonical := committed entity
}
pub enum SubmitError<FieldId> {
    Validation(ValidationReport<FieldId>),
    Conflicted { fields: Vec<FieldId> },
    Orphaned,
}
```

Design note on `Store`/rebase wiring: the store must push canonical changes into live drafts.
Keep it simple and single-threaded (e.g. store owns `Rc<RefCell<DraftInner>>`, hands out
handles, iterates registered drafts on `apply_canonical`). Whatever you pick, it's throwaway
plumbing — the *field/draft semantics* are the deliverable.

### 3. `spike-profile` — the hand-written feature

Value types (each hand-implements `Value` exactly as `#[bolted::value]` would generate —
sanitize first, then validate; error keys like `"too_short"`, `"invalid_email"` with params):

- `Username` — trim; 3..=20 chars; ASCII alphanumeric + `_`.
- `PersonName` — trim; 1..=30 chars.
- `Email` — trim + lowercase; must contain `@` with non-empty local/domain parts.
- `DateRange` — **composite value object**: `Raw = (Date, Date)`, invariant start ≤ end.
  (Define a minimal `Date` (y/m/d ord-comparable) locally; no chrono dependency.)

Entity + draft (hand-written as-if-generated):

```rust
pub struct Profile { pub username: Username, pub name: PersonName,
                     pub email: Email, pub availability: DateRange }
pub enum ProfileField { Username, Name, Email, Availability }
pub struct ProfileDraft {
    pub username: Field<Username>, pub name: Field<PersonName>,
    pub email: Field<Email>, pub availability: Field<DateRange>,
    username_check: SingleFlight<Result<(), ErrorData>>,   // async uniqueness
    /* status, base version */
}
impl ProfileDraft {
    pub fn try_set_username(&mut self, raw: String) -> Result<(), UsernameError>;
    // … one monomorphic setter per field, incl. try_set_availability(start: Date, end: Date)
    pub fn begin_username_check(&mut self) -> CheckToken;
    pub fn complete_username_check(&mut self, t: CheckToken, verdict: Result<(), ErrorData>) -> bool;
}
impl Draft for ProfileDraft { /* … */ }
```

Tier-2 rule (hand-written as `#[rule(pins(email))]` would generate): `corporate_email` — if
`username` starts with `"corp_"`, `email`'s domain must be `"corp.example"`. Pinned to
`ProfileField::Email`. (Deliberately relational and slightly arbitrary; it exists to exercise
rule mechanics, including re-evaluation on rebase of *either* involved field.)

A pending-or-failed username check must block `validate()`/`commit` (treat as a rule error
pinned to `Username` — record in the report how this felt; it feeds the freeze).

### 4. Tests — the deliverable that matters

Encode **invariants I1–I12 from ARCHITECTURE.md §7**, each as at least one test named
`i01_roundtrip`, `i02_untouched_follows_canonical`, … so the mapping is auditable.
Property-based (proptest) where the invariant quantifies over inputs (I1–I5 at minimum:
generate raw strings/dates, valid and invalid); example-based for the rest. Plus:

- sanitization cases (`"  Alice  "` → `"Alice"`; email lowercasing),
- `DateRange` composite (reversed dates → error; grouped setter),
- rule re-evaluation when a rebase changes `username` while `email` is dirty (I8 via tier 2),
- full lifecycle: checkout → edit → conflict → resolve → submit → canonical updated,
- create flow (Store with `None` canonical) end-to-end (I12).

## Exit checklist

- [ ] `mise run check` passes from a clean clone (fmt, clippy `-D warnings`, all tests).
- [ ] All 12 invariants present as named tests and passing.
- [ ] No `unwrap`/`expect`/`panic!` in library code.
- [ ] `bolted-core` has zero runtime dependencies.
- [ ] `docs/steps/step-01-report.md` written: what was built; every deviation from this doc
      with rationale; **friction log** (anything that felt awkward to write mechanically —
      this is the input to the design freeze); open questions; Rust version pinned.
- [ ] ROADMAP.md status table updated (01 → done, 02 → ready).

## If you hit a wall

Smallest-reasonable-choice rule (see CLAUDE.md): if this doc conflicts with reality or omits a
decision, make the smallest reversible choice and record it in the report. If the choice is
structural (changes a trait, an invariant, or ARCHITECTURE.md), **stop and record the question
in the report instead** — a design session resolves it.
