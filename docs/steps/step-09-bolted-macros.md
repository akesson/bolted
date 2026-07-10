# Step 09 — `bolted-macros`

**Phase 3 — Framework extraction.** Status: **ready**.

Read first: [ARCHITECTURE.md](../ARCHITECTURE.md) §5 (the manifestation doctrine — *generics carry
behavior, macros only stamp names*), §8 (D1–D17), §9 (the two questions this step owns), and
[CONFORMANCE.md](../CONFORMANCE.md) C01–C22.

> **Process note.** ARCHITECTURE §9 says its OPEN questions must not be resolved ad hoc, and
> `CLAUDE.md` splits planning (Fable) from implementation (Opus). At the owner's instruction this
> step was planned and implemented in one session, as steps 06, 07 and 08 were. The four decisions
> below were put to the owner **before any code was written**, each with the alternative it beat;
> the owner chose all four. Everything else is ordinary implementation latitude. The bending of the
> rule is recorded here rather than in the report, because a rule quietly bent four times is a rule
> that has been repealed. If step 10 is planned the same way, the rule should be *rewritten* rather
> than bent again.

## Why this step exists

Every line of `spike-profile` and `spike-note` was written to be thrown away. Their doc comments say
so, over and over: *"exactly what `#[bolted::value]` would generate"*, *"the shape `#[bolted::entity]`
emits"*, *"a macro emits this per field, mechanically"*. Two crates now make the same twelve claims by
hand, and a third (`spike-profile-ffi/src/dto.rs`) makes them again in a monomorphic projection.

The claim under test is VISION's, not a convenience: **macro output is the least-verifiable code on
the verification ladder**, so if a macro must exist at all it must do nothing but stamp names over
generics that rustc has already checked. Step 08 gave that claim its teeth by making the conformance
suite generic. This step is where the claim gets falsified or banked: a *generated* feature either
passes `bolted-conformance` unmodified, or the doctrine is wrong.

There is a second, sharper reason. Step 08's memory reads: *a suite with one implementor is shaped
like it*. The same disease has a macro form. `#[bolted::entity]` given exactly one input grows that
input's shape, silently, and nothing about reading it reveals the fact. So the step generates **two**
features — one with a tier-2 rule, an async check and a composite value; one with neither — and then
**mutates the macro** to prove the tests can see it.

The specific defect this must not reproduce is on record. From C12, added in step 08:

> a `StoreDraft` that decides entity-backedness by consulting a *single* field passes all 21 other
> invariants — on both features, verified by mutation. […] Step 09 will *generate* `is_based`; this is
> the test that will catch it.

## Decisions (each with the alternative it beat)

### D18 — the async check gets a core subtrait, `Checked: Draft`

`begin` / `complete` / `state` for a single-flight check live on **no trait**. Every shell re-derives
them; `spike-profile-ffi` re-derives them; step 08's `AsyncCheckFeature` had to declare them itself in
order to test C10, C13 and C16, and left a comment saying so:

> *Note what this trait has to declare that no `bolted-core` trait does: `begin`/`complete`/`state`
> for the check. Every generated shell re-derives that surface today. Step 09/10 should promote it.*

This is D17's argument, one layer down, and it gets D17's answer. A check is **id-keyed** exactly as a
resolver is field-keyed:

```rust
pub trait Checked: Draft {
    type CheckId: Copy + Eq + std::fmt::Debug;
    fn begin_check(&mut self, check: Self::CheckId) -> CheckToken;
    fn complete_check(&mut self, check: Self::CheckId, token: CheckToken,
                      verdict: Result<(), ErrorData>) -> bool;
    fn check_state(&self, check: Self::CheckId) -> &CheckState<Result<(), ErrorData>>;
    /// The field this check endorses. C13's "value-bound" is a statement about *this* field.
    fn check_pins(check: Self::CheckId) -> Self::FieldId;
}
```

A concrete `CheckId` enum is monomorphic, so it crosses FFI exactly as `FieldId` already does — the
§5 constraint that killed generic methods at the boundary does not bite. `Checked` is a **subtrait**
for the same two reasons `Stashable` is: a feature with no async check owes nothing, and the bound is
where a generic consumer states that it needs one.

**Rejected: keep the surface inherent, and let the macro emit `begin_username_check()`.** Cheapest
today, and it is precisely the shape D17 was written to undo. The tell is that four independent
consumers (two shells, the FFI, the conformance fixture) each re-derived the same three methods from
scratch; when that happens the contract is missing a name, not the consumers a convention.

**Rejected: defer to step 10.** Step 10 would then design a core trait *while* writing FFI codegen
against it, with the generated bindings as its only evidence. The evidence already exists here.

### D19 — "codegen dedup by raw type" is dissolved, not answered

§9 asks whether `#[bolted::entity]` should notice that three of `spike-profile`'s four fields share
`Raw = String` and emit shared types for them. The answer is that **in Rust there is nothing to
dedup**, because generics already dedup on exactly the axis that matters:

| Generic | Keyed on | Consequence |
|---|---|---|
| `FieldStash<R>` | the **raw** type | `ProfileStash` already shares one `FieldStash<String>` across three fields |
| `Field<V>` | the **value** type | `Field<Username>` and `Field<PersonName>` are distinct, and must be — they parse differently |

`#[bolted::entity]` stamps names over these; it never emits a field-state family, so no dedup pass
can exist for it to skip. The near-duplication the question was written about is real, but it is
**entirely** in `spike-profile-ffi/src/dto.rs`, where `#[data]` forbids generics and three
structurally identical `…FieldState` families are stamped out one per value type. That file already
measures the cost, in a comment written for this purpose. So the surviving question — *should
`bolted-ffi` emit one `TextFieldState` instead of three?* — is **reassigned to step 10**, where the
crate that would answer it lives.

**Rejected: build a cross-field dedup pass in `bolted-macros` now.** It would be the first analysis in
a macro whose entire doctrine is that it must stay trivial enough to read at a glance, and it would
optimize a duplication that does not exist in the crate the macro emits into.

### D20 — `#[bolted::value]` is a newtype DSL, and derives the `ErrorData` bridge

```rust
#[bolted::value(raw = String)]
#[sanitize(trim)]
#[validate(len_chars(min = 3, max = 20), custom(ascii_alnum_underscore, key = "invalid_chars"))]
pub struct Username(String);
```

emits the newtype, `as_str`, `impl Value`, `enum UsernameError { TooShort { min, actual }, TooLong {
max, actual }, InvalidChars }`, `impl From<UsernameError> for ErrorData`, and `constraints()`.

Sanitizers: `trim`, `lowercase`. Validators: `len_chars(min, max)` and `custom(path)`, where `path`
is a user fn `fn(&str) -> bool`. Both take an optional `key = "…"` and `variant = …` so the l10n keys
three shells already ship do not move.

The `From<Error> for ErrorData` block is the single most repetitive thing in `value_types.rs` and it
is pure name-stamping (variant → snake_case key, named fields → params). Generating it is the whole
point; a macro that emitted the newtype and left that behind would not have paid for itself.

**`DateRange` — the one composite value — keeps its hand-written `Value` impl.** §5's sketch says
"newtype", and `DateRange::try_new` is a single `start <= end` comparison that no DSL improves. What
a composite would need (struct-shaped fields, a tuple raw, a cross-field invariant) is a second macro
shape justified by exactly one example, and step 09 has no evidence about which way it should go.
Recorded as a finding, not guessed at.

**Rejected: a full DSL including composites.** It pushes `#[bolted::value]` toward being a validation
framework, which §5 forbids by name.

**Rejected: thin wiring only** (`impl Value` over user-written `sanitize`/`validate` fns). Maximally
faithful to the doctrine, and it leaves the boilerplate the doctrine exists to delete.

### D21 — `feature_model` is cut from this step

ROADMAP lists `#[bolted::feature_model]` among step 09's macros. It cannot be built here:

1. It "composes down onto BoltFFI's `#[data]`/`#[export]`". `bolted-macros` may not import boltffi —
   §5 makes `bolted-ffi` the *only* crate that does, and that seam is the swappable one.
2. The `Feature` trait it would stamp (`State` / `Msg` / `Caps` / `update`) **has never been
   written**, in any of the five spikes. `grep -r Feature crates/bolted-core` returns nothing. It is
   a sketch in §5 and nowhere else.

Emitting `#[boltffi::data]` as opaque tokens without linking boltffi is possible and *untestable*
inside `mise run check` — the only crate that could compile the output is `spike-profile-ffi`, which
is step 10's rewrite target.

So this step ships `value`, `entity` and `rules`. That the Elm half of ARCHITECTURE §1 has no code
behind it after five spikes is a finding, and it becomes a new §9 question rather than a decision.

## Deliverables

1. **`bolted_core::Checked`** (D18), with `CheckId` / `begin_check` / `complete_check` /
   `check_state` / `check_pins`. `ProfileDraft` implements it; its inherent `begin_username_check`
   family survives as thin delegates so `spike-profile-ffi` and `profile-web` do not churn.
2. **`bolted-conformance`'s `AsyncCheckFeature` sheds four members** — `begin_check`,
   `complete_check`, `check_state`, `checked_id` — and bounds `Self::Draft: Checked` instead. The
   four existing async tests become the new trait's first consumer, unchanged.
3. **`crates/bolted-macros`** — a proc-macro crate exporting `value`, `entity`, `rules`. Logic lives
   in ordinary `fn(TokenStream2) -> Result<TokenStream2, syn::Error>` so it is unit-testable; the
   `#[proc_macro_attribute]` shells are three lines each.
4. **`#[bolted::value]`** (D20).
5. **`#[bolted::entity]`** — emits the entity struct, `…Field` enum + `constraints()`, `…Stash`,
   `…Draft` of `Field<V>`s, `…Check` enum, `…Store` alias, monomorphic `try_set_*` setters, and
   `impl Draft` / `StoreDraft` / `Stashable` / `Checked`. Three properties are load-bearing and get
   their own tests:
   - **`is_based()` ORs over every field** (C12).
   - **`dirty_fields()` / `conflicts()` emit in declaration order**, which is observable.
   - **every mutation that can move a checked field's value passes through one generated guard**, so
     no path can skip C13's verdict reset. Not per-call-site.
6. **`#[bolted::rules]`** — an impl block whose `#[rule(pins(email))]` fns return `Result<(),
   ErrorData>`; the macro wraps each in a `RuleViolation` with the rule's name and pins, and emits
   the rule set the entity's `validate()` calls. Pinning a nonexistent field is a compile error
   because the `…Field` enum has no such variant — the property `profile.rs` claims in a comment and
   nothing has ever tested.
7. **`crates/gen-profile`** — `spike-profile` re-declared with macros. Runs the **entire**
   conformance suite (four `field_suite!`, `feature_suite!`, `rule_suite!`, `async_check_suite!`).
8. **`crates/gen-note`** — `spike-note` re-declared with macros: no rule, no check, no composite. The
   macro's falsifier, and the reason the step can use the word "generic".
9. **Golden tests** — `prettyplease`-formatted snapshots of what `value` and `entity` emit, checked
   in, compared in unit tests. Not `cargo-expand`, not `macrotest`: no tool outside `mise run check`.
10. **A mutation pass over the macro**, with each mutation verified to fail: single-field `is_based`;
    dropped guard; reordered `dirty_fields`; an off-by-one `len_chars`; an emitted `Copy` (D8).
11. **Docs**: ARCHITECTURE §5 (the trait sketches gain `Checked`), §8 (D18–D20), §9 (loses two
    questions, gains the `Feature` one); `docs/steps/step-09-report.md`; ROADMAP.

## Kill criteria (real — if hit, stop and report; do not work around)

1. **A generated feature cannot pass `bolted-conformance` unmodified.** If `gen-profile` needs a
   trait, an escape hatch or a fixture concession that `spike-profile` did not, the extraction is
   wrong somewhere upstream of the macro. Stop.
2. **`#[bolted::entity]` needs more than per-field name-stamping** — cross-field analysis, type
   inference, or conditional emission that depends on what another field is. §5 forbids it. This is
   the criterion that guards D19 from being quietly reversed.
3. **`#[bolted::value]`'s DSL grows a validator that must know about another validator.** That is a
   validation framework, and it is a different product.
4. **`Checked::CheckId` cannot be made monomorphic across the FFI boundary.** Then D18 was wrong and
   the surface belongs somewhere else. (Checkable in this step against `#[data]`'s constraints
   without running `boltffi`; if it needs a real pack, say so rather than assume.)
5. **A mutation of the macro survives the suite.** Deliverable 10 is not a formality: step 08 found a
   hole that 21 invariants had missed, and that hole is now *generated code*. If a mutation passes,
   the missing test is the deliverable, not a note in the report.

## Milestones

| # | What | Done when |
|---|------|-----------|
| M1 | `Checked` in `bolted-core`; `ProfileDraft` implements it | `mise run check` green, inherent methods delegate |
| M2 | `AsyncCheckFeature` bounds on `Checked` and sheds four members | the four async tests pass unchanged |
| M3 | `bolted-macros` skeleton + golden-test harness | an empty golden snapshot round-trips |
| M4 | `#[bolted::value]` | golden snapshot; `Username`/`PersonName`/`Email`/`Title`/`Body` reproduce |
| M5 | `#[bolted::entity]` | golden snapshot |
| M6 | `#[bolted::rules]` | a bad `pins(…)` fails to compile |
| M7 | `gen-note` — the falsifier, written **before** `gen-profile` | full suite green (no rule, no check) |
| M8 | `gen-profile` | full suite green, all four `field_suite!`s |
| M9 | mutation pass (deliverable 10) | every mutation fails at least one named test |
| M10 | docs, report, ROADMAP | working tree clean |

M7 precedes M8 deliberately. Writing the rule-and-check feature first would let the macro grow that
feature's shape before anything could notice.

## Non-goals

- `#[bolted::feature_model]` and the `Feature` trait (D21).
- Composite value objects in `#[bolted::value]` (D20).
- Any change to `spike-profile` / `spike-note` beyond the `Checked` retrofit. They are the golden
  reference the generated code is read against; a step that edits its own reference proves nothing.
- Regenerating `spike-profile-ffi`, the Swift app, or the Kotlin app. Step 10.
- FFI DTO dedup (D19, reassigned to step 10).
- Deleting anything. `gen-profile` and `spike-profile` coexist; the report says which is which.

## Exit checklist

- [ ] `mise run check` green; clippy `-D warnings`; no `unwrap`/`expect`/`panic!` in library code
      (`bolted-macros` returns `syn::Error`; the generated code contains no panic).
- [ ] `mise run test:web`, `test:apple`, `test:android`, `test:android:app`, `test:android:hazard`
      green — the `Checked` retrofit touches `spike-profile`, which every shell consumes.
- [ ] `gen-note` and `gen-profile` each pass every C-ID their feature owes.
- [ ] Every mutation in deliverable 10 verified to fail, by name, in the report.
- [ ] No emitted `Copy` on a value object; no `is_based` that reads one field.
- [ ] ARCHITECTURE §9 is down two questions and up one, and says why.
