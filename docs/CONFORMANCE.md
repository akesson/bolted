# Bolted — Conformance suite

**Status: frozen with ARCHITECTURE.md (step 06); C03 amended and C19 added in step 07; C12/C17/C18
amended and C22 added in step 08; C07 amended in step 09; C23 added in step 12 (D27); step 13 (D28)
adds the per-language-tier accounting at the foot of this document.** These are the design's
falsifiable claims.
Each one is normative: an implementation of the Bolted contract that violates any of them is not a
Bolted implementation, whatever else it does.

Every `CNN` below has at least one `cNN_*` function in
[`crates/bolted-conformance`](../crates/bolted-conformance/src), generic over a facet, and
[`tests/manifest.rs`](../crates/bolted-conformance/tests/manifest.rs) guarantees this document and
the suite cannot drift apart — three ways: every documented ID has a function, every function is a
documented ID, and every function is stamped by exactly one `*_suite!` macro, so a fixture cannot
skip one. That check is the suite's own rung-3 claim on [VISION](VISION.md)'s verification ladder:
the mapping is verified by the build, not by review.

## Where this suite is going

| Step | What happens to it |
|------|--------------------|
| 06 | Named, documented, and running against `spike-profile`, the hand-written "as-if-generated" reference implementation. |
| 08 | **Generic over a feature**, extracted into `bolted-conformance`, and run against **two** — `spike-profile` (rule + async check + composite value) and `spike-note` (neither). A suite with one implementor proves nothing about genericity. |
| 09 | Run against **four**: the two above, plus `gen-profile` and `gen-note`, which declare the same features through `bolted-macros`. A generated feature either satisfies the contract unmodified or the doctrine that macros only stamp names is wrong. |
| 13 (now) | Emitted as **per-language contract tests** (Kotlin, Swift) from the same IDs, generic over a values-only fixture, so a generated binding that breaks an invariant fails its own build (D28). Not every ID crosses the boundary; the per-ID accounting is at the foot of this document. C# is step 14. |

Wording convention: **must** is normative. "The field" means an editable `Field<V>` of a draft; "the
draft" means a value implementing `Draft`; "theirs" is an incoming canonical value.

Not every facet owes every invariant. C08 presupposes a tier-2 rule; C10, C13 and C16 presuppose an
async check; a facet with neither still satisfies the rest. The suite says so in trait bounds
(`RuleFeature`, `AsyncCheckFeature`) rather than in prose.

## The invariants

| ID | Statement |
|----|-----------|
| C01 | **Roundtrip.** `Value::try_new(v.into_raw()) == Ok(v)` for every valid `v`. Holding a `Value` is proof of validity, and the raw form loses none of it. |
| C02 | **A clean field follows canonical.** A non-dirty field must adopt `theirs` on rebase and stay `InSync`. |
| C03 | **A dirty field is never silently overwritten.** Rebase over a dirty field **whose canonical value moved** and whose value differs from `theirs` must preserve your value, enter `Conflicted { theirs }`, and leave the recorded ancestor (`base`) where it was. |
| C04 | **Convergent rebase is clean.** If a dirty field's value already equals `theirs`, rebase must adopt it as the base and land clean and `InSync` — two edits that agree are not a conflict. |
| C05 | **Revert-for-free.** Setting a field back to its base value must clear dirty. Dirtiness is a pure function of the data, never of touch history. |
| C06 | **No stale-value submit.** A failed `try_set` must be recorded as `Invalid { raw, error }` and must block submit. The previous valid value must never be silently committed in its place. |
| C07 | **Commit is the parse moment.** `commit` succeeds **iff** every field is `Valid`, none is `Conflicted`, no rule is violated, and the status is `Live`. The committed entity equals the field values. Each refusal is typed (`Validation` / `Conflicted` / `Orphaned`) and hands the draft back. When more than one gate would refuse, the refusal is the first of **`Orphaned` → `Conflicted` → `Validation`**: a deleted entity has nothing to conflict with, and a contested field's value is not yet the user's to fix. |
| C08 | **Rebase re-runs tier-2.** Validation is a pure function of current draft state, so a rebase that moves any field must change the next `validate()` accordingly — including rules that pin to a field the rebase did not touch. |
| C09 | **Resolution semantics.** `resolve_keep_mine`: value stays yours, base becomes theirs, the field stays dirty and returns to `InSync`. `resolve_take_theirs`: value and base become theirs, clean, `InSync`. |
| C10 | **Latest check wins.** A completion carrying a superseded token must be discarded. At most one check is in flight. |
| C11 | **Deletion orphans.** Deleting the canonical entity under a live draft must set status `Orphaned`, and submitting an orphaned draft must be a typed outcome, never a silent failure or a resurrection. |
| C12 | **Create-flow never rebases.** A draft with no base entity must not be moved by any canonical change, and must commit normally. Conversely, a draft that retains an ancestor in **any** field is entity-backed: it rebases, and it orphans. |
| C13 | **Verdicts are value-bound.** Any change to a checked field's *value* — by edit, rebase, or `resolve_take_theirs` — must reset its async check to unchecked. A verdict endorses a value, so a changed value un-endorses it. A mutation that leaves the value unchanged (edit-to-same, `resolve_keep_mine`, a conflict that preserves your value) must leave the verdict standing. |
| C14 | **Auto-converge on edit.** Editing a conflicted field to a value equal to `theirs` must resolve the conflict: base adopted, clean, `InSync`. This is C04 with the two events in the other order, and it must reach the same state. |
| C15 | **The base version tracks the rebase.** After a canonical change rebases a draft, the draft's `base_version` must equal the store's version. An orphaned draft is based on no canonical and its stamp must stop moving. |
| C16 | **An unrun check blocks a dirty field.** If an async check is pinned to a field, the field is dirty, and the check has not run, `commit` must refuse. If the field is clean it must not — a clean field holds the canonical value, which was verified when it was committed. |
| C17 | **Submit releases the draft.** A successful submit consumes the draft: the id reports `!is_live()`, yields no draft, and a second submit is `AlreadySubmitted`. A **refused** submit must leave the draft live and intact, under the same id. |
| C18 | **Release is explicit, idempotent, and the only path.** `close(id)` frees the draft, may be called any number of times — including on an id that is already gone — and stops the store rebasing it. Nothing else releases a draft: a handle that is merely forgotten leaves an edit session the store goes on rebasing, on **every** platform. |
| C19 | **Rebase is a three-way merge, and idempotent.** A field whose incoming canonical value equals its recorded ancestor must not be conflicted by a rebase, whatever its dirty state — nobody else moved it. A canonical that moves *back* to the ancestor must clear an existing conflict. Rebasing twice onto the same canonical must equal rebasing once. |
| C20 | **A draft stashes to raw data and restores from it.** The stash carries each field's last input attempt and its ancestor, both raw; restoring reproduces every field's value, ancestor, validity — including `Invalid { raw }` — and dirtiness. It must **not** carry `sync`: a conflict names a canonical value the server may no longer hold, so it re-derives on the next rebase. It must **not** carry an async verdict: a verdict endorses a value against a server state that may have moved, so a restored checked field is unchecked, and C16 demands a fresh check while it is dirty. |
| C21 | **Restore is a rebase.** Adopting a restored draft must conflict exactly those fields whose canonical moved while it was away, and leave the others dirty and `InSync` (C19). A resolution taken before the restore must survive it, because its effect lives in the ancestor. Adopting an entity-backed draft into a store with no canonical must orphan it (C11). A create-flow draft must never be moved (C12). |
| C22 | **"A draft exists" and "a draft rebases" are different questions.** The store must answer both, separately. A create-flow draft (C12) and an orphan (C11) exist but do not rebase; `close` removes a draft from both counts. No single count may stand for the pair. |
| C23 | **A stashed ancestor that no longer parses degrades to dirty-from-unset, and conflicts.** `from_stash` will not fabricate an ancestor a tightened constraint invalidated: it degrades that field to create-flow (`base: None`) and keeps the user's last input, so the field is dirty with no ancestor. On the next rebase against live canonical it is `Conflicted` whenever the rescued value differs (C03 — never a silent overwrite), and clean when it converges (C04). This is the failure mode D27 accepts *inside* a parsed envelope; the wholesale envelope refusal (version/shape) sits at the codec boundary, outside the core. |

## Notes on the ones that cost something

**C07's precedence clause was added in step 09, and it is an invariant nobody had written down.**
Every `c07_*` assertion built a draft that fails exactly *one* gate, so none of them could see the
order the gates run in. `commit_gates` reordered to check conflicts before orphaned passed the entire
suite — found by mutation, not by reading. Both spikes have implemented `Orphaned → Conflicted →
Validation` since step 01, identically and by accident of writing order. It matters because a shell
obeying the wrong order shows a "keep mine / take theirs" banner over an entity the server has
deleted, and offers to merge into a record that is gone. Step 09 *generates* those three `if`s, where
a reordering is one line. Same disease as C12's second sentence: **a suite is silent about the states
it never constructs.**

**C13 + C16 together** are what make client-side async validation trustworthy. C13 guarantees a
surviving `Done(Ok)` was computed for the value now in the field; C16 guarantees the value in a dirty
field has a verdict at all. Neither alone is enough: without C13 a stale pass endorses a value it
never saw; without C16 the shell can simply never ask. Both were confirmed as *default* code paths on
two independent shells before they were promoted to invariants (step-01 F1/F2, step-03, step-04).

**C17 and C18** exist because handle lifetime is the one place the platforms genuinely disagreed.
Apple's ARC runs Rust `Drop` when the last Swift reference dies; Android's ART never does, so a
dropped Kotlin handle leaks the Rust draft and the store rebases a zombie forever (step 05, H1).

**C18 was amended in step 08** and now says `close` is the *only* release path. Under D16 the store
owns its drafts and a handle is a `DraftId` — `Copy`, and not an owner. There is nothing to drop.
Rust used to be forgiving here in exactly the way the GC platforms are not, which meant a lifecycle
bug written against the reference implementation could only surface for the first time on Android.
A shell may still wrap the id in a native RAII type (`ProfileDraftFfi`'s `Drop`, reached from ARC's
`deinit`); that is the shell calling `close`, not the framework doing it for free.

**C14 is not cosmetic.** Without it, a conflicted field edited to `theirs` shows a "keep mine / take
theirs" banner whose two buttons do visibly the same thing, while the dirty marker stays lit — a
state the running web shell (step 04) found actively confusing. C04 already makes the identical
judgement when the canonical change arrives second; leaving the edit-arrives-second case unresolved
made the conflict model depend on event order.

**C19 was added in step 07, and C03 was amended to make room for it.** A store rebases *every* field
of a draft on *every* canonical change, so a field the server never touched is routinely rebased onto
its own ancestor. `Field::rebase` compared `mine` against `theirs` but never `theirs` against `base`,
so a dirty `name` entered `Conflicted { theirs }` whenever the server moved `email` — offering a
"take theirs" button holding the user's own ancestor, and refusing `commit` with `Conflicted`. C14's
disease, a different vector.

It survived the freeze because **C03's property test never sampled it**: it drew `base`, `mine` and
`theirs` independently and assumed only `mine != base` and `theirs != mine`, and two random strings
are essentially never equal. `c08_rebase_reruns_tier2_rule` had been producing a spurious conflict on
`email` since it was written, and passed, because it only asserted on the rule. The lesson is about
property tests, not about rebase: **an `assume` set that is missing a precondition does not weaken the
property — it silently asserts the bug.**

**C12's second sentence was added in step 08, and it is the only thing that tests `is_based`.** Every
draft the rest of the suite builds has an ancestor in all of its fields or in none, so a `StoreDraft`
that decides entity-backedness by consulting a *single* field passes all 21 other invariants — on both
facets, verified by mutation. It matters because a partially-ancestored draft is not hypothetical:
the stash is an untrusted input, and a constraint tightened between app versions leaves exactly this
shape. A draft misjudged create-flow is never rebased and never orphaned, and it silently overwrites
the server on submit. Step 09 will *generate* `is_based`; this is the test that will catch it.

**C22 was added in step 08, and it is a bug given a name.** Phase 1 wrote the store loop twice, and
each copy grew a `live_draft_count()`: the core's meant *"how many drafts would a canonical change
rebase"*, the FFI wrapper's meant *"how many drafts exist"*. They disagreed by one on every
create-flow draft, and nothing could notice, because the two counts lived in two crates and no test
compared them. Step 07 finally proved the divergence with a Swift test whose name was
`testLiveDraftCountDisagreesWithTheCoreOnACreateFlowDraft` — a test that could only *document* the
bug, since with two hand-written stores there was no single answer to make right. D16 deleted one of
them. Two questions now have two names, and a shell that wants the other one has to ask for it.

## The per-language tier (step 13): what crosses the boundary

Step 06 promised the C-IDs would be *emitted* as per-language contract tests. Step 13 does it (D28):
`bolted-ffi-gen` emits a Kotlin and a Swift test per ID, generic over a hand-written, **values-only**
fixture, run in the emulator/simulator tiers the shells already build. The foreign tier verifies **the
boundary, not the algebra** — that the binding and wrapper *preserve* the core's semantics across the
seam — so it is example-based on purpose: the properties stay in the Rust suite above, which already
proves them against four facets. A foreign test that fails names a binding or wrapper bug, never the
core's.

Not every invariant crosses. An ID is **emitted** when the *public generated surface* (the `#[export]`
verbs and `#[data]` DTOs — nothing internal, kill criterion 2) can both **construct** its precondition
and **observe** its outcome. It is **exempt** when the surface cannot, with the reason stated — and an
ID that is observable but merely lacks a verb is *not* exempt: the generator gains the verb (it is our
output) and the ID is emitted. This table is the emitter's own source of truth
(`bolted_ffi_gen::foreign::BOUNDARY_MAP`); `bolted-ffi-gen`'s `tests/manifest.rs` ties it to this
document in both directions, so the two cannot drift. The map is step 13 M0; **22 of 23 emitted, one
exempt** — comfortably inside the "no more than a third exempt" gate.

| ID | Boundary | Observed through — or exempt because |
|----|----------|--------------------------------------|
| C01 | emitted | `try_set_*` then `snapshot().<f>.validity == Valid{value}`; re-setting that `value` is idempotent — the canonical raw that crosses back re-parses to the same value. |
| C02 | emitted | An entity-backed checkout leaves `<f>` untouched; `apply_canonical` moves it; `snapshot()`: `Valid{theirs}`, `InSync`, `dirty == false`. |
| C03 | emitted | Edit `<f>`; `apply_canonical` moves it to a third value; `snapshot()` keeps your value, `sync == Conflicted{base, theirs}`, `conflicts` names it. |
| C04 | emitted | Edit `<f>`, then `apply_canonical` to that same value; `snapshot()`: `InSync`, `dirty == false`. |
| C05 | emitted | Edit `<f>`, then set it back to base; `snapshot().<f>.dirty == false`. |
| C06 | emitted | `try_set_*` an invalid raw returns the typed error and records `Invalid{raw, error}`; `submit()` → `Validation` naming the field; `canonical()` is unchanged. |
| C07 | emitted | `submit()` returns the typed `SubmitErrorFfi`; the precedence clause is composed at the boundary — `delete_canonical` over a conflict → `Orphaned` outranks `Conflicted`; a conflict plus an invalid field → `Conflicted` outranks `Validation`. (Needs `delete_canonical`.) |
| C08 | emitted | Arrange the rule satisfied, `apply_canonical` moves an unpinned field; `validate().rule_errors` names the rule and `conflicts` is empty. (Richest fixture surface — the rule-flip is supplied as values; KC3 is the M2 watch.) |
| C09 | emitted | `resolve_keep_mine` / `resolve_take_theirs`; value·dirty·sync from `snapshot()`, and the resolved `base` from `stash().<f>.base` (the `InSync` DTO carries no base). |
| C10 | exempt | See below — the one exemption. |
| C11 | emitted | `delete_canonical` under a live draft → `snapshot().status == Orphaned`, `submit()` → `Orphaned`, `is_live()` still true. (Needs `delete_canonical`.) |
| C12 | emitted | Create-flow: checkout from an empty store, `apply_canonical` leaves it unset with `rebasing_draft_count == 0`, then fill + `submit`. Contrapositive: null one field's `base` in the `stash` DTO, `restore`, `rebasing_draft_count == 1`, and it orphans when the canonical is gone. |
| C13 | emitted | `run_*_check` → `Passed`; a value-moving change (edit, rebase, take-theirs) → `snapshot().<check> == Unchecked`; a value-preserving one (edit-to-same, keep-mine, a preserved conflict) leaves it standing. |
| C14 | emitted | Conflict `<f>`, then `try_set_*` theirs; `snapshot()`: `InSync`, `dirty == false`, `conflicts` empty. |
| C15 | emitted | `apply_canonical` advances `snapshot().version`; after `delete_canonical` an orphan's stamp stops moving. (Orphan half needs `delete_canonical`.) |
| C16 | emitted | A dirty, unchecked checked-field blocks `submit()` (`…_check_required`, pinned); a clean one does not. |
| C17 | emitted | A refused `submit()` leaves `is_live()` true and the edit intact; a successful one tombstones it; a second is `AlreadySubmitted`. |
| C18 | emitted | `close()` / `AutoCloseable` frees the draft, is idempotent, and stops rebase (`rebasing_draft_count`). The "a merely-forgotten handle leaks on ART" clause stays with the lifecycle probes — a non-goal to re-emit, since GC makes it non-deterministic to observe. |
| C19 | emitted | `apply_canonical` on a *different* field leaves an edited `<f>` dirty · `InSync` · unconflicted; rebasing onto the same/ancestor canonical is idempotent. |
| C20 | emitted | `stash()` carries each field's `raw` + `base` and structurally no `sync`/verdict; `restore` reproduces value, ancestor, validity (incl. `Invalid{raw}`), dirtiness; a restored checked field is `Unchecked`. |
| C21 | emitted | `restore` conflicts exactly the fields whose canonical moved, keeps a pre-death resolution, orphans into a deleted canonical, and never moves a create-flow draft. |
| C22 | emitted | `live_draft_count` vs `rebasing_draft_count` diverge on a create-flow draft and on an orphan; `close` removes from both. |
| C23 | emitted | Set a field's `base` to an invalid raw in the `stash` DTO, `accept_stash` + `restore`: the field comes back dirty with its `base` dropped; on rebase it conflicts when the rescued value differs (C03), clean when it converges (C04). |

**The one exemption. C10** (latest check wins) is the single ID the boundary cannot express. Its
property — *a completion carrying a superseded token is discarded* — presupposes **two checks in
flight at once**. The generated check driver (`run_username_check`) is atomic: within one FFI call it
begins one token, calls the foreign checker with no lock held, and completes that same token, over a
*single* `take`-n checker instance. A second token can therefore never exist to be superseded, and the
"at most one in flight" guarantee is *enforced* by that shape rather than *observable* through it. The
mechanism is driven directly in the Rust tier (`SingleFlight`, `begin_check` / `complete_check`).
Emitting it would mean exposing raw single-flight tokens across the FFI — a change to the check-driving
contract (D18), not an added accessor — so it stays exempt, and honestly so.

**Accessor gaps the map found.** One real gap: the store cannot **delete** its canonical across the
boundary — `apply_canonical` only *sets* one. `bolted_core::Store::delete_canonical` exists and the
Rust suite drives C11/C15/C22 and C07's precedence through it; the FFI simply never projected it. Per
the rule above, an observable ID that merely lacks a verb is not exempt: `delete_canonical` is added to
the generated store (step 13, deliverable 8), and C11 (plus the strongest forms of C07, C15, C12) is
emitted. One non-gap, recorded so it is not re-litigated: C09's resolved `base` is absent from the
`snapshot()` `InSync` DTO but present in `stash()`, so the boundary reads it there rather than growing
the snapshot — the smaller change.
