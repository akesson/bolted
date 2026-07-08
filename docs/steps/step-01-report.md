# Step 01 — Core semantics prototype — Report

**Status: done.** `mise run check` passes from a clean rebuild (fmt + clippy `-D warnings` + all
tests). This report is the handoff back to planning: what was built, every deviation, the friction
log (the input to the design freeze), and open questions.

- **Rust pinned:** `1.95.0` (stable, in `mise.toml`). Edition 2024, resolver 3.
- **Deps:** `bolted-core` has **zero** dependencies (runtime and dev). `proptest 1.11` is a dev-dep
  of `spike-profile` only.
- **Verification:** 27 tests — 4 `bolted-core` unit, 8 `spike-profile` behaviour, 15 invariants
  (I1–I12). I1–I5 are property-based (proptest); I6–I12 example-based.

## What was built

```
mise.toml                         rust 1.95.0; tasks test / check
Cargo.toml                        workspace, resolver "3", edition 2024, publish=false
crates/bolted-core/               generic primitives, zero deps, #![forbid(unsafe_code)]
  src/constraint.rs   Constraint (Required | LenChars | Custom)
  src/value.rs        Value trait
  src/field.rs        Validity, SyncState, Field<V> + all rebase/resolve/dirty logic (+unit tests)
  src/report.rs       ErrorData, RuleViolation, ValidationReport
  src/draft.rs        DraftStatus, Draft trait
  src/single_flight.rs SingleFlight/CheckToken/CheckState (+unit test)
  src/store.rs        StoreDraft, DraftHandle, Store, SubmitError
crates/spike-profile/             hand-written "as-if-generated" feature
  src/value_types.rs  Date, Username, PersonName, Email, DateRange + From<*Error> for ErrorData
  src/profile.rs      Profile, ProfileField, ProfileDraft, corporate_email rule, Draft/StoreDraft
  tests/invariants.rs I1–I12 (auditable names)
  tests/behaviors.rs  sanitization, composite setter, constraint metadata, full lifecycle, rules
```

### Invariant → test map (auditable)

| # | Invariant | Test(s) | Kind |
|---|-----------|---------|------|
| I1 | roundtrip | `i01_roundtrip_{username,person_name,email,date_range}` | property |
| I2 | untouched follows canonical | `i02_untouched_follows_canonical` | property |
| I3 | dirty preserved on conflict | `i03_dirty_preserved_on_conflict` | property |
| I4 | convergent rebase clean | `i04_convergent_rebase_clean` | property |
| I5 | revert clears dirty | `i05_revert_clears_dirty` | property |
| I6 | failed set blocks submit | `i06_failed_set_blocks_submit` | example |
| I7 | commit equivalence + entity == fields | `i07_commit_equivalence_and_entity_equals_fields` | example |
| I8 | rebase re-runs tier-2 | `i08_rebase_reruns_tier2_rule` | example |
| I9 | resolution semantics | `i09_resolution_semantics` | example |
| I10 | stale async ignored | `i10_stale_async_ignored` | example |
| I11 | delete orphans; typed submit | `i11_delete_orphans_and_submit_is_typed` | example |
| I12 | create-flow never rebases | `i12_create_flow_never_rebases_and_commits` | example |

All 12 hold. **I8 holds "for free":** `validate()` is a pure function of current draft state, so
there is no memoised validation to invalidate — a rebase changes field values and the next
`validate()` reflects them. (This is a *positive* design property; the one place it fails is the
async verdict — see friction **F1**.)

## Deviations from the step doc (all internal; the ARCHITECTURE §5 public traits are unchanged)

The step doc authorised renaming/adjusting internals and recording deviations. None of these touch
the public `Value` / `Draft` contracts as sketched in ARCHITECTURE §5, except D1 (a bound) — noted.

- **D1 — `Value::Raw: Debug`.** Added `Debug` to the `Raw` associated-type bound (the §5 sketch has
  `Clone + PartialEq + Send + Sync + 'static`). Reason: `Field`/`Validity` derive `Debug`, and the
  retained raw of a rejected input (`Invalid { raw, .. }`) must be printable for diagnostics/test
  failure messages. All real raw types (`String`, `(Date, Date)`) are `Debug`. Low-risk; the only
  trait-surface change in this step.
- **D2 — store-driving methods live on a `StoreDraft: Draft` subtrait**, not on `Draft`. The public
  `Draft` trait is kept **exactly** as §5 sketches it (FFI surface). `from_canonical` / `rebase` /
  `orphan` (which the `Store` needs to construct and live-rebase drafts generically) sit on
  `StoreDraft`, and `Store<D: StoreDraft>`. See open question **Q1**.
- **D3 — `Constraint::Required` is field-level.** `Value::constraints()` returns value-intrinsic
  constraints only (`LenChars`, `Custom`). Required-ness is prepended by `ProfileField::constraints()`
  (the as-if-generated entity layer), since a value type cannot know whether its field is
  `Option<_>`. See open question **Q3**.
- **D4 — tier-1 `V::Error → ErrorData` bridge is in `spike-profile`.** Each value error type impls
  `From<XError> for ErrorData`; a `tier1_error<V: Value>` helper bounded on `V::Error: Into<ErrorData>`
  turns any field's validity into an optional `ErrorData`. Kept off the core `Value` trait. See
  open question **Q2** — this worked so cleanly it's a candidate to promote into `Value`.
- **D5 — Rc/RefCell/Weak store wiring.** Per the step doc's explicit "throwaway plumbing" licence:
  the store holds `Weak<RefCell<D>>` per live draft; `DraftHandle` holds the sole strong `Rc` and is
  not `Clone`. `apply_canonical`/`delete_canonical` upgrade-and-mutate; dead weaks are pruned.
  `submit` moves the draft out with `Rc::try_unwrap` (guaranteed to succeed under single ownership;
  the unreachable `Err` arm re-validates defensively rather than `unwrap`). Create-flow drafts are
  **not registered**, which is how I12 holds structurally. Orphaned drafts no-op on `rebase`
  (orphan is terminal).
- **D6 — ergonomic helpers not in the sketch:** `Field::base()`, `Field::into_valid()`,
  `Store::version()`, `ProfileDraft::base_version()`, `ProfileField::constraints()`, per-field
  `resolve_keep_mine(field)` / `resolve_take_theirs(field)` dispatchers, `ErrorData::new(key)`.
  `SubmitError` derives `Debug` only.

## Friction log — awkward-to-write-mechanically (the design-freeze input)

**F1 — Async verdict is not tied to the value it validated (staleness bug).** *This is the most
important finding.* `SingleFlight` coordinates the *ordering* of checks (latest `begin` wins, stale
completions ignored — I10), but a completed verdict is **not invalidated when the checked field
changes**. Probe: check `"alice"` → `Done(Ok)`; then `try_set_username("corp_bob")` (or a rebase to
it) → `validate()` still passes the uniqueness gate, endorsing `"corp_bob"` with a verdict computed
for `"alice"`. The single-flight has no notion of *which input* it validated. This is a sharper
version of F2: even a shell that *did* trigger a check can ship a stale "unique". Options for the
freeze: (a) core resets the check to `Idle` whenever the pinned field's value changes (via
`try_set`/`rebase`); (b) the verdict carries the value it was computed for and `validate()` compares;
(c) leave it to the shell + tier-3 server re-check. I implemented none (minimal scope) — flagged.

**F2 — "pending-or-failed blocks; never-checked passes" leaves a correctness-by-convention gap.**
As specified, `Idle` (never ran) and `Done(Ok)` both pass, so a draft that *never triggered* a
uniqueness check commits with uniqueness unverified. Correctness then depends on the shell
remembering to trigger — exactly the "glue that fails at runtime" the project exists to remove.
Modelling the pending/failed block as a rule pinned to `Username` felt clean; the *policy* about
never-checked is the open bit. Tied to F1. (VISION's tier-3 server re-check is the backstop, but
the core surface currently lets an unverified draft through client-side.)

**F3 — A failed `submit` destroys the draft.** `submit(&mut self, draft: DraftHandle<D>)` takes the
handle by value, and `DraftHandle` is not `Clone`, so on **any** outcome (including a validation or
conflict failure) the handle is consumed and the draft is dropped — the user cannot keep editing
after a rejected submit without re-checking-out and losing edits. Pre-checks already run under a
borrow, so only the *ownership* is the problem. A real API almost certainly wants to return the
handle on the error path (e.g. `Result<(), (DraftHandle<D>, SubmitError)>`) or borrow for
pre-checks and consume only on the commit path. Implemented the spec'd by-value signature; flag for
the freeze/extraction (it changes the signature).

**F4 — Uniform per-field `.clone()` collides with `clippy::clone_on_copy` for `Copy` value
objects.** Generated checkout/rebase code wants to `.clone()` every field uniformly, but `DateRange`
is `Copy`, so `p.availability.clone()` is a hard clippy error under `-D warnings`. I dropped the
clone for that one field (non-uniform code). A generator has three choices: emit non-uniform code
(track which value types are `Copy`), blanket-`#[allow(clippy::clone_on_copy)]` generated modules,
or **forbid `Copy` on value objects** (make them `Clone`-only) so codegen stays uniform. The last is
cleanest for a macro and is my recommendation to weigh at freeze.

**F5 — `commit`'s error channel is too narrow, so it re-encodes conflicts/orphan as fake rules.**
`Draft::commit` returns `Result<Entity, ValidationReport>`, but its contract (I7) forbids conflicts
and orphaned status — neither of which is a validation tier. I inject synthetic
`RuleViolation`s (`unresolved_conflict`, `orphaned`) so the report can express them. Meanwhile the
store's `submit` has *typed* `SubmitError::{Conflicted, Orphaned}` and checks them **before** commit.
So there are two divergent error taxonomies for the same failures. Freeze question: give `commit` a
richer `CommitError { Validation | Conflict | Orphan }` mirroring `SubmitError`? (Q4)

**F6 — `try_set` is orthogonal to sync (no auto-converge on edit).** Editing a `Conflicted` field to
a value equal to `theirs` does **not** resolve it; it stays `Conflicted` until an explicit
`resolve_*`. Defensible (resolution is a user choice), but a real decision — a UI might expect
"typing their value" to clear the conflict. Flagged.

**F7 — `SyncState::Conflicted.base` duplicates `Field.base`.** While conflicted they are always
equal (the ancestor doesn't move). Kept both to match the specified `Conflicted { base, theirs }`
shape (self-contained 3-way merge data), but it's redundant state to keep consistent. Could drop
`Conflicted.base` and read `Field.base()`.

**Wrote-cleanly (no friction) — worth noting as design wins:** the `Field` validity×sync split and
the value-based `is_dirty` were entirely mechanical; per-field `try_set`/`dirty_fields`/`conflicts`/
`validate` fan-outs are trivially macro-able; the composite value object (`DateRange`, `Raw =
(Date, Date)`, one grouped setter) needed zero special-casing; errors-as-data flowed straight
through. Nothing in the field/draft *semantics* fought back — the friction is all at the seams
(async, submit ownership, codegen-vs-lint).

## Open questions (structural — for a design session; not resolved here)

- **Q1** — Live-rebase driving: does it belong in the core `Draft` contract, or a `StoreDraft`-style
  capability (as prototyped)? Relates to how the FFI surface stays minimal.
- **Q2** — Should `Value::Error: Into<ErrorData>` be a trait bound? The bridge (D4/`tier1_error`)
  was cleaner than per-field code and points at promoting it into `Value`.
- **Q3** — `Constraint::Required`: same enum as value-intrinsic constraints, or a separate
  field-metadata channel? (Value can't know Option-ness.)
- **Q4** — `commit` error taxonomy: unify with `submit`'s typed variants (F5)?
- **Q5** — Async-check value-binding + commit policy (F1/F2): the single biggest correctness item.
- **Q6** — `submit` ownership on failure (F3): return the handle vs consume.

None of the ARCHITECTURE §9 OPEN questions were resolved ad hoc. Store concurrency used "the
simplest thing" (single-threaded `Rc<RefCell>`), per §9's explicit licence to defer to Phase 3.

## Exit checklist

- [x] `mise run check` passes from a clean rebuild (fmt, clippy `-D warnings`, all tests).
- [x] All 12 invariants present as named tests and passing.
- [x] No `unwrap`/`expect`/`panic!` in library code (test modules excepted).
- [x] `bolted-core` has zero runtime dependencies.
- [x] Report written (this file).
- [x] ROADMAP.md status table updated (01 → done, 02 → ready).
