# Bolted — Conformance suite

**Status: frozen with ARCHITECTURE.md (step 06).** These are the design's falsifiable claims. Each
one is normative: an implementation of the Bolted contract that violates any of them is not a Bolted
implementation, whatever else it does.

Every `CNN` below has at least one `cNN_*` test in
[`crates/spike-profile/tests/conformance.rs`](../crates/spike-profile/tests/conformance.rs), and a
test in that file guarantees this document and the suite cannot drift apart
(`conformance_manifest_has_a_test_for_every_id`). That check is the suite's own rung-3 claim on
[VISION](VISION.md)'s verification ladder: the mapping is verified by the build, not by review.

## Where this suite is going

| Step | What happens to it |
|------|--------------------|
| 06 (now) | Named, documented, and running against `spike-profile`, the hand-written "as-if-generated" reference implementation. |
| 08 | Made **generic over a feature** and extracted alongside `bolted-core`. Doing it now would mean inventing a fixture trait with exactly one implementor. |
| 10 | Emitted as **per-language contract tests** (Swift, Kotlin, C#) from the same IDs, so a generated binding that breaks C09 fails its own build. |

Wording convention: **must** is normative. "The field" means an editable `Field<V>` of a draft; "the
draft" means a value implementing `Draft`; "theirs" is an incoming canonical value.

## The invariants

| ID | Statement |
|----|-----------|
| C01 | **Roundtrip.** `Value::try_new(v.into_raw()) == Ok(v)` for every valid `v`. Holding a `Value` is proof of validity, and the raw form loses none of it. |
| C02 | **A clean field follows canonical.** A non-dirty field must adopt `theirs` on rebase and stay `InSync`. |
| C03 | **A dirty field is never silently overwritten.** Rebase over a dirty field whose value differs from `theirs` must preserve your value, enter `Conflicted { theirs }`, and leave the recorded ancestor (`base`) where it was. |
| C04 | **Convergent rebase is clean.** If a dirty field's value already equals `theirs`, rebase must adopt it as the base and land clean and `InSync` — two edits that agree are not a conflict. |
| C05 | **Revert-for-free.** Setting a field back to its base value must clear dirty. Dirtiness is a pure function of the data, never of touch history. |
| C06 | **No stale-value submit.** A failed `try_set` must be recorded as `Invalid { raw, error }` and must block submit. The previous valid value must never be silently committed in its place. |
| C07 | **Commit is the parse moment.** `commit` succeeds **iff** every field is `Valid`, none is `Conflicted`, no rule is violated, and the status is `Live`. The committed entity equals the field values. Each refusal is typed (`Validation` / `Conflicted` / `Orphaned`) and hands the draft back. |
| C08 | **Rebase re-runs tier-2.** Validation is a pure function of current draft state, so a rebase that moves any field must change the next `validate()` accordingly — including rules that pin to a field the rebase did not touch. |
| C09 | **Resolution semantics.** `resolve_keep_mine`: value stays yours, base becomes theirs, the field stays dirty and returns to `InSync`. `resolve_take_theirs`: value and base become theirs, clean, `InSync`. |
| C10 | **Latest check wins.** A completion carrying a superseded token must be discarded. At most one check is in flight. |
| C11 | **Deletion orphans.** Deleting the canonical entity under a live draft must set status `Orphaned`, and submitting an orphaned draft must be a typed outcome, never a silent failure or a resurrection. |
| C12 | **Create-flow never rebases.** A draft with no base entity must not be moved by any canonical change, and must commit normally. |
| C13 | **Verdicts are value-bound.** Any change to a checked field's *value* — by edit, rebase, or `resolve_take_theirs` — must reset its async check to unchecked. A verdict endorses a value, so a changed value un-endorses it. A mutation that leaves the value unchanged (edit-to-same, `resolve_keep_mine`, a conflict that preserves your value) must leave the verdict standing. |
| C14 | **Auto-converge on edit.** Editing a conflicted field to a value equal to `theirs` must resolve the conflict: base adopted, clean, `InSync`. This is C04 with the two events in the other order, and it must reach the same state. |
| C15 | **The base version tracks the rebase.** After a canonical change rebases a draft, the draft's `base_version` must equal the store's version. An orphaned draft is based on no canonical and its stamp must stop moving. |
| C16 | **An unrun check blocks a dirty field.** If an async check is pinned to a field, the field is dirty, and the check has not run, `commit` must refuse. If the field is clean it must not — a clean field holds the canonical value, which was verified when it was committed. |
| C17 | **Submit tombstones the handle.** A successful submit consumes the draft: the handle reports `!is_live()`, yields no draft, and a second submit is `AlreadySubmitted`. A **refused** submit must leave the handle live and the draft intact. |
| C18 | **Release is explicit and idempotent.** `close()` frees the draft, may be called any number of times, and stops the store rebasing it. Dropping the handle must do the same. |

## Notes on three of them

**C13 + C16 together** are what make client-side async validation trustworthy. C13 guarantees a
surviving `Done(Ok)` was computed for the value now in the field; C16 guarantees the value in a dirty
field has a verdict at all. Neither alone is enough: without C13 a stale pass endorses a value it
never saw; without C16 the shell can simply never ask. Both were confirmed as *default* code paths on
two independent shells before they were promoted to invariants (step-01 F1/F2, step-03, step-04).

**C17 and C18** exist because handle lifetime is the one place the platforms genuinely disagree.
Apple's ARC runs Rust `Drop` when the last Swift reference dies; Android's ART never does, so a
dropped Kotlin handle leaks the Rust draft and the store rebases a zombie forever (step 05, H1). The
contract therefore names an explicit release. In Rust, `close()` is a convenience that `Drop` already
performs; in Kotlin and C# it is the *only* release path.

**C14 is not cosmetic.** Without it, a conflicted field edited to `theirs` shows a "keep mine / take
theirs" banner whose two buttons do visibly the same thing, while the dirty marker stays lit — a
state the running web shell (step 04) found actively confusing. C04 already makes the identical
judgement when the canonical change arrives second; leaving the edit-arrives-second case unresolved
made the conflict model depend on event order.
