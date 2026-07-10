# Step 09 — report: `bolted-macros`

**Status: done. No kill criteria hit.** Plan: [step-09-bolted-macros.md](step-09-bolted-macros.md).

Four decisions were put to the owner before any code was written, each with the alternative it beat;
all four are now ARCHITECTURE §8 rows **D18–D21**. ARCHITECTURE is **v1.3**; C07 is amended.

---

## The four headlines

### 1. Writing the macro is what made the core honest

The doctrine (§5) says *generics carry behavior, macros only stamp names*. It had never been tested,
because there was no macro. Attempting one immediately produced three pieces of **judgement** that
wanted to be emitted per feature:

| Would-be generated code | Now, at rung 1 | The judgement it encodes |
|---|---|---|
| `match field.validity() { Valid → None, Invalid → .., Unset → "required" }` | `Field::required_error` | D13: whether an empty field is an error is a *field*-level call |
| `if orphaned {..} if !conflicts.is_empty() {..} if !report.is_ok() {..}` | `commit_gates` | C07: when a commit is refused, and with which typed reason |
| `match check.state() { Pending → .., Done(Err) → .., Idle if dirty → .. }` | `SingleFlight::violation` | C13 + C16: when an async verdict blocks |

Both spikes had hand-written all three, identically, because they were written as *as-if-generated*
code by someone who knew the answers. A macro does not know the answers. Emitting them would have put
the design's most consequential decisions in the least verifiable code on the ladder — and every
conformance test would still have passed, because the emitted code would have been *correct*. The
problem is not correctness; it is that the correctness would live somewhere no reviewer reads and no
type-checker constrains.

So the doctrine is now a test, `golden.rs::the_emitted_code_makes_no_judgement_of_its_own`: emitted
code may not mention `Validity::`, `CheckState::`, `CommitError::Conflicted`, `CommitError::Orphaned`,
or `is_ok()`. If one comes back, the judgement has moved, and nothing else would notice.

**This is the step's real finding.** "Thin macros" read like a style preference in §5. It is not: it
is the constraint that forces behavior down to where rustc checks it once.

### 2. The mutation pass found an invariant nobody had written down — and one of my own mutations was a lie

Eleven mutations of the macros and the core helpers. Two survived the first run.

**M9 is real, and it is C12's disease exactly.** Reordering `commit_gates` to check *conflicts* before
*orphaned* passed the **entire** suite — 22 invariants, four features, 281 tests. C07 said each refusal
is typed; it never said which one wins when two gates apply. And every `c07_*` assertion built a draft
that fails exactly one gate, so none of them could see the order. Both spikes have implemented
`Orphaned → Conflicted → Validation` since step 01, identically, by accident of the order someone typed
the `if`s. It matters: under the wrong order a shell shows a "keep mine / take theirs" banner over an
entity the server has **deleted**, and offers to merge into a record that is gone.

Step 09 *generates* those three `if`s, where a reordering is one line. C07 is amended, `c07_*` now
builds a draft that is orphaned **and** conflicted (and one that is conflicted **and** invalid), and
M11 was added for the second ordering. Both fail on both hand-written and both generated features.

> A suite is silent about the states it never constructs. That sentence has now cost this project two
> invariants — C12's second clause in step 08, C07's precedence clause in step 09 — and both were found
> by mutation rather than by reading.

**M7 was my own error, and it is worth more than the mutation would have been.** It repointed
`check_pins` at `fields.first()`. `Profile`'s first field *is* `username` — the checked field. The
mutation changed nothing, and "survived" for the reason a tautology passes. Repointed at `fields.last()`
it is caught by three tests immediately.

The lesson generalizes past this step: **a surviving mutation is two hypotheses, not one.** Either the
suite is blind, or the mutation was vacuous. Reporting the first without eliminating the second is how a
mutation pass produces false confidence in the opposite direction — you go looking for a missing test
that does not exist. Every survivor must be read as a claim about the *mutant*, and confirmed to differ
from the original, before it is read as a claim about the suite.

Final: **12 mutations, 12 caught, 0 survivors.**

| # | Mutation | Caught by (representative) |
|---|---|---|
| M1 | `is_based` consults a single field | `c12_an_ancestor_in_any_field…`, `golden::is_based_ors_over_every_field` |
| M2 | `resolve_take_theirs` skips the guard | `c13_verdicts_are_value_bound`, `golden::every_mutation_path_routes_through_the_single_guard` |
| M3 | `dirty_fields` in reverse declaration order | `golden::dirty_fields_and_conflicts_emit_in_declaration_order`, `differential::…agree_at_every_step…` |
| M4 | `len_chars` max exclusive (off-by-one) | `differential::the_length_bounds_are_inclusive_on_both_ends` |
| M5 | `rebase` skips the guard | `c13_verdicts_are_value_bound` |
| M6 | tier-2 rules never collected | `c08_rebase_reruns_tier2`, `differential::…agree_at_every_step…` |
| M7 | `check_pins` names the wrong field | `c13`, `c16` |
| M8 | the `lowercase` sanitizer is dropped | `differential::the_generated_value_types_parse_exactly…` |
| M9 | `commit_gates`: conflicts before orphaned | `c07_commit_is_the_parse_moment` *(new)* |
| M10 | an unrun check never blocks a dirty field | `c16_an_unrun_check_blocks_a_dirty_field` |
| M11 | `commit_gates`: validation before conflicts | `c07_commit_is_the_parse_moment` *(new)* |
| M12 | the checked field's setter loses its guard | `c13_verdicts_are_value_bound`, `golden::an_unchecked_fields_setter_does_not_pay_for_the_guard` |

### 3. A uniform macro nearly cost a String clone per keystroke, and the report nearly claimed otherwise

The first `#[bolted::entity]` routed **every** `try_set_*` through the C13 guard, because that is what
"one guard, no mutation path can skip it" naturally emits. The guard clones each checked field's value
before the mutation and compares afterwards. So generated `try_set_name` cloned the `Username` on every
keystroke of the *name* box — work `spike-profile::try_set_name` does not do, on the exact path step
07's kill criterion 4 measures, and the path the framework's central bet ("the core validates every
keystroke") rests on.

I had already written *"nothing in step 09 touches the hot path"* into this report before checking the
emitted code against it. It was false.

The fix is not a compromise: which fields carry a check is written in the declaration, so the macro
knows statically that `try_set_name` cannot move `username`'s value. A setter is guarded **iff its own
field carries a check**; the resolvers take a field id at *runtime* and `rebase` moves every field, so
both stay guarded unconditionally. Generated and hand-written now do the same work. The property is
pinned from both sides — `an_unchecked_fields_setter_does_not_pay_for_the_guard` — and M12 confirms
that removing the *remaining* guard fails `c13_verdicts_are_value_bound`.

Two things worth keeping from this:

- **A uniform macro is not automatically a cheap one.** "Emit the same line for every field" is the
  doctrine's virtue and, unexamined, its cost: uniformity moved work onto a path where the reference
  implementation had made a per-field decision, and no conformance test could see it, because
  behaviour was identical.
- **Writing the report against the code, rather than from memory, is what caught it.** The same habit
  caught two false numbers in step 08's report. It is worth the hour.

### 4. `#[bolted::feature_model]` could not be built, and the reason is bigger than the macro

ROADMAP listed it. It needs two things that do not exist:

1. It composes onto BoltFFI's `#[data]`/`#[export]`, and `bolted-macros` may not import boltffi — §5
   makes `bolted-ffi` the only crate that does, and that seam is the swappable one.
2. **The `Feature` trait it would stamp has never been written.** `grep -r Feature crates/bolted-core`
   returns nothing. Five spikes have shipped without one: every shell drives `Store` and `Draft`
   directly.

§1 opens *"MVVM with an Elm core"* and §5 lists `Feature (State / Msg / Caps / update)` among "the
contracts". Neither has any code behind it, on any platform, after four shells and nine steps. Either
the trait is real and the spikes have been quietly ignoring it, or §1's Elm framing actually describes
the **store** (canonical state + effects-as-data + a fan-out returned as `Vec<DraftId>`), and the trait
should be struck.

This is now the largest undischarged claim in the architecture, and it is a new §9 question owning its
own session. It should be settled before Phase 4 plans a harness around a trait that may not exist.

---

## What was built

- **`bolted_core::Checked`** (D18) — `CheckId` / `begin_check` / `complete_check` / `check_state` /
  `check_pins`. `ProfileDraft` implements it; its three inherent methods survive as delegates.
- **`bolted-conformance`'s `AsyncCheckFeature` lost four members** and gained one (`check_id`); it
  bounds `ConformanceFeature<Draft: Checked>`. **Not one test changed.** The pinned field now comes
  from `Checked::check_pins`, which is what keeps that method load-bearing.
- **Three new `bolted-core` generics** (headline 1): `Field::required_error`, `commit_gates`,
  `SingleFlight::violation`.
- **`crates/bolted-macros`** — `value`, `entity`, `rules`. Logic is `fn(TokenStream2, TokenStream2)
  -> syn::Result<TokenStream2>`; the three `#[proc_macro_attribute]` shells are one line each. Errors
  are `syn::Error` → `compile_error!`, so `CLAUDE.md`'s no-panic rule holds and a malformed declaration
  fails the **build**.
- **`crates/gen-note`** (written *before* `gen-profile`, on purpose) and **`crates/gen-profile`**, each
  passing the whole conformance suite unmodified: 38 and 68 tests.
- **`gen-profile/tests/differential.rs`** — the two implementations driven through one nine-step edit
  session, compared on *everything a shell can observe* at **every** step (dirty set, conflict set,
  validation report, base version, status), plus parse outcomes, constraint metadata, orphaning,
  create-flow, and the stash round-trip.
- **Golden snapshots** (`prettyplease`, `BLESS=1` to rewrite) — no `cargo-expand`, no `macrotest`, no
  tool outside `mise run check`.
- **D8 moved from rung 3 to rung 2.** `#[bolted::value]` *refuses* a `#[derive(Copy)]` value, with the
  reason. ARCHITECTURE said `bolted-check` would flag it; a macro can simply decline to compile it.

### The arithmetic, stated plainly

| | hand-written | declared | delta |
|---|---|---|---|
| `spike-profile` → `gen-profile` | 574 | 135 *(69 declaration + 66 hand-written `DateRange` + predicates)* | −76 % |
| `spike-note` → `gen-note` | 269 | 20 | −93 % |
| **feature code total** | **843** | **155** | **−82 %** |
| `bolted-macros` | — | 1085 | the price |

Code lines, comments and blanks excluded. **The macro crate is larger than the two features it
replaces**, and will be until roughly the fifth feature. That is the expected shape of an extraction and
not a defence of it: the argument for `bolted-macros` is not line count, it is that 843 lines of
per-feature judgement became 155 lines of declaration plus 1085 lines that are *reviewed once*. The
1085 also buys the rung-2 refusals (`Copy`, composite values, unnamed l10n keys, duplicate error
variants) that no amount of hand-writing provides.

---

## Deviations from the step doc

1. **`#[bolted::value]` takes no `raw = T` argument.** The raw type *is* the newtype's field type, so
   the macro infers it. The sketch in the plan carried `raw = String` from a world where composites
   were in scope. Smallest reversible choice; adding the argument later is additive.
2. **`#[bolted::entity(rules)]` needs a flag.** The entity emits the rule-set trait; `#[bolted::rules]`
   emits its impl. Without the flag the entity emits an *empty* impl, and with it, none — so promising
   rules you don't write is a missing-impl error, and writing rules you didn't promise is a
   conflicting-impl error. Both at rung 2. I could not find a way to remove the flag that keeps both
   errors; deriving the entity name from `ProfileDraft` by stripping `"Draft"` is name-guessing that
   breaks on the first feature called `Redraft`, so `#[bolted::rules(entity = Profile)]` names it.
3. **`try_set_availability(start, end)` became `try_set_availability((start, end))`.** A macro sees a
   value's `Raw` as one type; it does not know a 2-tuple is two arguments a human would rather pass
   separately. Cosmetic, and it only affects the composite.
4. **The attribute path is `#[bolted_macros::value]`, not `#[bolted::value]`.** There is no `bolted`
   facade crate yet; it is a Phase-4 item. Every doc keeps writing `#[bolted::…]` because that is the
   name it will have.
5. **`spike-profile` keeps `begin_username_check`/`complete_username_check`/`username_check_state`** as
   thin delegates to `Checked`, so `spike-profile-ffi` and `profile-web` did not churn in this step.
   `gen-profile` does **not** emit them, and its fixture drives the check purely through `Checked` —
   which is the evidence that D18's surface is sufficient on its own.
6. **`bolted-core` grew three public items** the plan did not list. Justified by §5, and by the
   alternative being "emit the logic": see headline 1.
7. **`gen-profile` dev-depends on `spike-profile`.** ROADMAP says the hand-written code is "the golden
   reference the generated code is diffed against". A textual diff cannot do that job across 574 vs 135
   lines. `differential.rs` does it by behaviour.
8. **The C13 guard wraps a setter only when that setter's own field carries a check** (headline 3). The
   plan's deliverable 5 said "every mutation that can move a checked field's value passes through one
   generated guard" — which is what shipped, once "can" is read as the compiler reads it. The first
   implementation guarded all setters and was uniformly correct and needlessly slower.

## Friction log

0. **Uniformity has a price, and it lands on the hot path.** See headline 3. The general shape: a macro's
   emitted code is judged by conformance tests, which see *behaviour*; a per-field decision the reference
   implementation made for **latency** is invisible to every one of them. Whatever `bolted-check` becomes
   (Phase 4), a generated-vs-reference *work* comparison — allocations on `try_set`, say — would have
   caught this without a benchmark.
1. **A macro with one input is shaped like that input** — the step-08 lesson, in a new costume. `gen-note`
   was written first for exactly this reason, and it is why `check_enum`, `checked_impl` and the guard
   all have a zero-checks path that is *not* dead code. Had `gen-profile` come first, `Checked` would
   plausibly have been emitted unconditionally, and `spike-note`'s shape would have been impossible.
2. **Two `len_chars` on one value emitted two `TooShort` variants.** Nothing forbade it; the compiler
   catches it at the use site, pointing into code the user never wrote. Now refused with a message that
   names the fix. Found by asking "what does the second one do?" rather than by any test.
3. **A uniform DSL normalizes error keys.** `spike-note`'s `Title` rejects an empty string with
   `blank`; `gen-note`'s rejects it with `too_short`. `len_chars` cannot know a feature calls its
   minimum-length failure something else. `custom(..)` takes `key = "…"` and `variant = …` overrides for
   exactly this reason, and `gen-profile` uses them to keep `invalid_chars` and `invalid_email` — the
   keys three shells already ship. `len_chars` deliberately does not. **This is a real migration cost
   the framework imposes**, and a shell adopting `#[bolted::value]` will re-key its localisation files.
   Recorded rather than papered over; if it bites, the fix is `min_key`/`max_key` overrides.
4. **Proc-macro crates cannot be used from their own integration tests.** The golden tests therefore
   live in `src/golden.rs` behind `#[cfg(test)]`, which is only possible because the expanders are plain
   functions and `proc_macro::TokenStream` is confined to `expand::run`. Worth keeping: it is also what
   made the mutation pass cheap.
5. **`gradle dev34DebugAndroidTest` is up-to-date-cacheable.** `mise run test:android:app` reported
   `BUILD SUCCESSFUL` in 2 s with "4 executed, 61 up-to-date" and did not run a single test; the results
   XML was 50 minutes stale. The green exit code was meaningless. Every Android number in this report
   was re-taken with `--rerun-tasks` after deleting the results XML. **A verb that can succeed without
   doing anything is a verb that will eventually lie to a report**, and `bolted-check`/Phase 4 should
   consider making the test verbs non-cacheable or printing the observed test count.

## Kill criteria

**None hit.** Two were close enough to argue about, so here is the argument:

- **KC2 — "`#[bolted::entity]` needs more than per-field name-stamping."** The `rules` flag (deviation 2)
  is a per-*entity* fact stated in the declaration, not a cross-field analysis; `check_pins` maps a
  check to its own field. No emitted code depends on what a *different* field is. The criterion stands
  un-hit, and D19 is what it was guarding: there is no dedup pass.
- **KC3 — "a validator that must know about another validator."** `len_chars` emits `TooShort` only when
  `min > 0`, which is a validator knowing about itself. The `__len` binding is hoisted once when *any*
  `len_chars` exists — shared codegen, not shared semantics. Friction 2 shows where the line actually is,
  and the answer was to refuse the ambiguous declaration rather than resolve it.
- KC1 (a generated feature needing a concession) — not hit. `gen-profile` and `spike-profile` each score
  **62** on `bolted-conformance`; `gen-note` and `spike-note` each score **37**. The same suites, the same
  counts, no skips. (`gen-note`'s crate total is 38: it carries one extra test of its own, asserting that
  the declared `Constraint`s survive the macro — the one thing `#[bolted::value]` emits that no C-ID
  covers, since constraint metadata is exported to shells and never re-checked.)
- KC4 (`CheckId` cannot be monomorphic across FFI) — not hit, and not *proved* either: see below.
- KC5 (a mutation survives) — not hit, after M9's real survivor was closed with a test rather than a note.

---

## Open questions and what is still unverified

**§9 is down two and up three.** D18 answers "where does the async check's surface live?"; D19 dissolves
"codegen dedup by raw type" and reassigns its residue to step 10. Newly opened: the `Feature` trait
(headline 3), composite values in `#[bolted::value]` (D20), and the FFI-side dedup.

**Three things this step could not verify, and does not claim:**

1. **`Checked::CheckId` has not crossed a real FFI boundary.** It is a plain C-like enum, so `#[data]`
   accepts it by inspection and KC4 is *not hit*; but `spike-profile-ffi` still exports the old inherent
   methods and no generated binding exists. Step 10 tests this for real. If a `CheckId` cannot be
   projected, D18 is wrong and the surface belongs elsewhere.
2. **`mise run bench:android:device` — still NOT RUN.** No device attached; the verb refused, as
   designed. Step 07's kill criterion 4 (per-keystroke round-trip ≤ 1.0 ms on physical silicon) still
   rests on step 05's emulator figure. After headline 3, generated `try_set_*` does the same work as
   the hand-written reference: an unchecked field's setter calls `Field::try_set` directly, and the
   checked field's setter adds one `Option<Username>` clone — which `spike-profile` also does. So step
   09 leaves the hot path where it found it. **But nothing here is measured**, on the platform the
   measurement was promised for, and the shells still run the hand-written feature anyway. To close
   it: connect a phone with USB debugging and run the verb.
3. **`mise run test:apple:ui` — still never run** in this project (needs Xcode plus a logged-in GUI
   session holding Accessibility permission). Outstanding since step 06.

## Verification sweep

Every Android figure below was taken after `rm`-ing the results XML and re-running with
`--rerun-tasks`; see friction 5.

```
mise run check                 283 tests   (158 at step 08)
mise run test:web              8/8
mise run test:apple            39 probe + 14 VM
mise run test:android          44/44 on ART        (probe, forced rerun)
mise run test:android:app      35/35 headless      (Compose UI, forced rerun)
mise run test:android:hazard   3/3                 (forced rerun)
mise run bench:android:device  NOT RUN — no device (the verb refused, as designed)
mise run test:apple:ui         not run — still GUI-gated
no emulator left running        ✓
```

Of the 283: `bolted-macros` 19, `gen-note` 38, `gen-profile` 68 (62 conformance + 6 differential),
`spike-profile` 70 (62 conformance + 8 `behaviors.rs`), `spike-note` 37, `profile-web` 37,
`bolted-core` 11, `bolted-conformance` 3 (the drift manifest).
