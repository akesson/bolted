# Step 10 — report: `bolted-ffi`, a generated FFI surface

**Status: done. No kill criteria hit.** Plan: [step-10-bolted-ffi.md](step-10-bolted-ffi.md).

Four decisions were put to the owner before any code was written, each with the alternative it beat;
all four are now ARCHITECTURE §8 rows **D22–D25**. ARCHITECTURE is **v1.4**.

One deliverable did not land, and it is named plainly in *"What is not done"* below: the four Swift and
Kotlin shells still link the **hand-written** `spike-profile-ffi`. What replaced it is stronger than a
promise and weaker than a migration: a Swift package that compiles, links and runs against the
generated bindings, and a measured, classified diff of the two surfaces.

---

## The four headlines

### 1. `#[bolted::feature_model]` was never possible, and step 09 cut it for the wrong reason

Step 09's D21 said it could not exist because *"it needs boltffi, and `bolted-macros` may not import
boltffi"*. That is beside the point — a macro emitting `#[data]` tokens imports nothing.

**BoltFFI discovers its FFI surface by reading source files off disk and parsing them with `syn`**
(`boltffi_scan::SourceTree::load` → `read_to_string` → `syn::parse_file`, walking `mod` declarations).
It never sees expanded code. Not even under its own `BINDING_EXPANSION` mode, whose `Request::render()`
re-scans the same file from disk; that mode governs where the *metadata blob* is emitted, not what
bindgen can see.

So an attribute macro cannot produce an FFI surface. And the failure is silent:
`boltffi generate swift` exits **0** and simply omits it.

[`artifacts/step-10-boltffi-visibility/probe.sh`](artifacts/step-10-boltffi-visibility/probe.sh)
builds this table from scratch, in a temp dir, in about fifteen seconds:

| where the `#[data]`/`#[export]` items live | `cargo build` | `boltffi generate` | `boltffi pack` |
|---|---|---|---|
| hand-written in `src/lib.rs` | ✅ | ✅ | ✅ |
| emitted by a proc macro | ✅ | ❌ **silent** | ❌ |
| a committed `mod generated;` file | ✅ | ✅ | ❌ *until a root re-export* |
| `include!`d (i.e. `OUT_DIR`) | ✅ | ❌ **silent** | ❌ |
| a dependency crate that depends on boltffi | ✅ | ✅ | ✅ |

The last column cost an afternoon and is the sharper finding. **`boltffi pack` compiles with
`BOLTFFI_BINDING_EXPANSION=1`, and under that flag the *first* `#[data]`/`#[export]` item the compiler
expands is replaced by a whole-crate metadata blob that names every exported type from the crate
root** — wherever it happens to be injected. A crate whose classes live in `mod generated;` compiles,
generates Swift, and then dies during `pack` with:

```
error[E0425]: cannot find type `ProfileStoreFfi` in this scope
  --> crates/gen-profile-ffi/src/generated.rs:24:1
   |
24 | #[data]        ← a #[data] on an unrelated enum, twenty lines from anything called ProfileStoreFfi
```

The fix is `pub use generated::*;` and a comment, because nothing else can say it: **`mise run check`
structurally cannot see this**, since the blob only exists under the pack's environment variable.

That makes **three distinct ways this toolchain lets a broken FFI surface look fine**: it silently omits
macro output; it generates Swift for Rust that does not compile (observed, `generate` and `rustc` are
independent); and it lets a crate pass `check` and `generate` and still fail to `pack`. Each one is a
place where the verification ladder has no rung. `bolted-check` (Phase 4) now has a concrete brief.

**D22** is the answer: the FFI layer is generated *as committed source*, and `mise run check`
regenerates it and compares. This is not a consolation prize. §5 calls macro output "the least
verifiable code on the ladder"; a formatted, reviewable, diffable `generated.rs` that a compiler and a
clippy pass and a drift test all read is strictly better than an expansion nobody sees.

### 2. It generates — and a Swift test suite proves it end to end

`gen-note-ffi` is a **20-line declaration** that becomes 1 416 lines of Swift. `gen-profile-ffi` is the
gnarly feature — composite value object, tier-2 rule, async check — and `mise run test:apple:gen` runs
**7/7** against bindings nobody hand-wrote:

- **D24's `TextFieldState`** reaching Swift *from the dependency crate* `bolted-ffi`;
- **D23's `.draftClosed`** thrown by a mutator on a draft C17 released;
- the generated `UsernameChecker` capability, implemented in Swift, called from Rust **with no lock
  held**, its `failed_key` coming from the declaration and not from Swift;
- **C13**: moving the checked value discards the verdict bound to it;
- the composite's hand-written projection, typed errors with params, and constraints as data — no
  numeric literal in Swift.

Declaration → generated Rust → generated Swift → compiles → links → runs. That chain had never been
walked before, on any platform.

### 3. The mutation pass found six real holes — and the drift check would have hidden every one

A trap this project had not met before. `generated.rs` is committed and `tests/drift.rs` compares it
against the generator's output, so **every** mutation of the generator fails the drift test instantly,
for a reason that says nothing about whether the behaviour is tested. Run naively, the pass reports
14/14 caught and means nothing — the same vacuity that made step 09's M7 a lie.

So [`artifacts/step-10-mutations.py`](artifacts/step-10-mutations.py) does the honest thing: apply the
mutation, **regenerate**, **assert the regenerated file actually changed** (a mutation whose output is
identical is reported as *vacuous*, not as a pass), then run the suite *with the drift tests excluded*.

First run: **8 caught, 0 vacuous, 6 survived.** The survivors were all *projection* properties:

| survivor | what nothing asserted |
|---|---|
| M1 | `to_field_id` mapped every core field to the first FFI variant |
| M2 | the snapshot's `any_dirty` was always `false` |
| M3 | the conflict list came out reversed |
| M9 | `resolve_take_theirs` kept mine |
| M12 | a text field never reported itself dirty |
| M14 | a `Pending` check projected as `Unchecked` — no spinner, ever |

`bolted-conformance` covers the **core**. `wrapper.rs` covered the **wrapper's behaviour** — D23, the
check driver, the lifecycle. Nobody covered the seam between them: what the snapshot *says*. It is step
08's disease ("a suite with one implementor is shaped like it") wearing the FFI layer's clothes, and it
took a mutation pass to see it, again.

Five new tests later: **14 caught, 0 vacuous, 0 survived.**

### 4. Two open questions answered, one of them differently than it was asked

**§9's *"a real `Pending` across FFI"* is closed.** With a synchronous checker, `begin` and `complete`
are atomic inside one call, so a `snapshot()` taken after `run_username_check()` returns can never be
`Pending`. It reaches a **stream subscriber**, because the generated driver pushes a snapshot between
the two halves. `a_check_in_flight_is_observably_pending` asserts exactly that sequence
(`[Pending, Passed]`), and M14 confirms nothing else would have noticed. A spinner bound to the stream
is real; one bound to `snapshot()` is not.

**KC2 — "`ProfileCheck` cannot cross `#[data]`" — is not hit, for a reason the step doc did not
anticipate.** The generated FFI **never crosses a `CheckId`**: it monomorphizes each check into its own
`run_username_check()`. And it *could not* cross one if it wanted to, because `ProfileCheck` is emitted
by `#[bolted::entity]`, and macro output is invisible to bindgen (headline 1). D18 therefore stands as
a **Rust-side contract the generator consumes**, which is a better place for it than the wire. The M0
probe separately shows a hand-written C-like enum crosses `#[data]` fine, as parameter and as return.

---

## What was built

- **`bolted-decl`** (D25) — the declaration model and its parsers, extracted from `bolted-macros`. Two
  emitters now read one contract. `ValueDecl::error_variants` is where it earns its keep:
  `len_chars(min = 0, …)` raises no `TooShort`, and the FFI generator must reach that same answer or
  `UsernameErrorFfi` gains a variant its `From` impl can never construct — which rustc would accept.
  Also `Feature::from_file`: a whole feature scanned out of source text with `syn`, exactly as BoltFFI
  scans ours. Neither tool can see expanded code, and now neither pretends to.
- **`bolted-macros`, refactored** onto it. *Proof it is only a refactor: all five golden snapshots are
  byte-identical, with no `BLESS`.* One nearly moved — I emitted `vec![a, b]` where step 09 emitted
  `vec![a, b,]` — which is precisely what the goldens are for.
- **`bolted-ffi`** — the shared `#[data]` DTOs. The only **hand-written** crate importing boltffi.
- **`bolted-ffi-gen`** — declaration → Rust source text. Depends on boltffi not at all: it writes
  `#[data]` as tokens, and the generated crate imports them.
- **`gen-note-ffi`**, written *first* (a generator with one input is shaped like that input — step 08's
  lesson, then step 09's), and **`gen-profile-ffi`** with `src/custom.rs`, the escape hatch.
- **`mise run gen:ffi`**, the drift tests, `pack:apple:gen`, `test:apple:gen`, and
  `apple/gen-profile-smoke`.

### The escape hatch, and why it is a compile error

`Profile::availability` is a `DateRange`: a composite (D20), hand-written, with no declaration for the
generator to read. The generator does not guess. It emits `use crate::custom::*;` and references four
types and six functions by name; a missing one is a **compile error** (rung 2), not a binding that
quietly lost a field. `custom.rs` is 138 lines — for *one* field. `spike-profile-ffi/src/dto.rs` pays
that for **all four**.

### The arithmetic, stated plainly

| | hand-written | generated | hand-written residue |
|---|---|---|---|
| `spike-profile-ffi` → `gen-profile-ffi` | **1 054** | 631 | 146 (`custom.rs` + `lib.rs`) |
| `gen-note-ffi` | — | 479 | 0 |

| the price | code lines |
|---|---|
| `bolted-ffi-gen` | 907 |
| `bolted-ffi` | 175 |
| `bolted-decl` + `bolted-macros` | 656 + 632 = 1 288 *(1 085 before the split)* |

Code lines, comments and blanks excluded. The generator is larger than the FFI layer it replaces, and
will be until roughly the third feature. Same shape as step 09, same defence: 1 054 lines of per-feature
FFI judgement became 146 lines of hand-written residue plus code that is **reviewed once**. The split
into `bolted-decl` cost ~200 lines of API surface and doc, and bought the guarantee that the macro and
the generator cannot disagree about what a declaration means.

---

## Deviations from the step doc

1. **`#[check(..)]` gained a fourth key, `failed_key`.** The generated verdict is `.pass`/`.fail`, and a
   failing check must raise *some* `ErrorData`. `spike-profile-ffi` hardcoded `"username_taken"` in
   Rust; the alternative was `Fail { error: ErrorData }`, letting Swift supply the key. Rejected for the
   reason step 09 gave `custom(..)` a `key` override: a localisation key is part of the contract, and a
   key that lives in Swift cannot be checked against a Kotlin strings file. It is FFI-only — the
   macro's golden snapshots did not move.
2. **The drift check compares code, not bytes.** `mise run check` also runs `cargo fmt --all --check`,
   so the committed file is rustfmt's while the generator emits `prettyplease`'s. Rather than pick a
   fight, both sides are parsed and re-printed through one formatter. What that leaves uncaught is a
   hand-added `//` comment; a `///` doc comment is an attribute and is caught, as is every token of
   code. (Import order is emitted pre-sorted for the same reason — otherwise the two verbs rewrite each
   other forever.)
3. **`snapshots_small()` is generated.** A 4-slot subscription is a probe affordance, not a framework
   concept, but the Apple probe's drop-newest overflow test needs it. A `snapshots_with_capacity(n)`
   would be the right shape. Recorded, not fixed.
4. **`canonical_snapshot` asks the core instead of building a second table.** `Draft::from_canonical`
   already yields exactly the shape — every field `Valid`/`InSync`, nothing dirty, no check run — so
   the generated code calls it. `spike-profile-ffi` wrote that table by hand, per field, twice.
5. **The setter parameter is `raw`, not `value`.** My first draft called it `value`, which is more
   accurate and would have broken every `trySetUsername(raw:)` call site in four shells for nothing.
   Swift argument labels are part of the surface.
6. **Deliverable 10 (repoint the shells) is not done.** See below.

## Friction log

0. **A committed generated file makes a mutation pass lie.** See headline 3. Any project that
   drift-checks generated code has this problem: the drift test is a perfect, useless detector. The fix
   is to regenerate inside the mutation harness and prove the output changed — and it is worth saying
   that the harness's *vacuity check* is what turned "14/14 caught" into "6 survived".
1. **A forbidding test that cannot fire forbids nothing.** `the_emitted_code_makes_no_judgement_of_its_own`
   was copied from step 09, which matches a `TokenStream`'s `to_string` (`quote` prints `Validity ::`
   with a space). This file matches `prettyplease`, which prints paths tight. **Every needle silently
   matched nothing, and the test was green.** Now pinned from both sides by
   `the_forbidden_needles_can_actually_fire`, which runs a deliberately guilty fragment through the same
   formatter. Same disease as a vacuous mutation; different costume.
2. **Uniformity has a price, and clippy found it this time.** The generator cloned every field out of
   `…Values` in `build_entity`. Free for a `String`; a `clippy::clone_on_copy` **error** for the `Copy`
   wire type `AvailabilityRaw`. That is D8's disease one layer down. Fixed by taking the values by value
   and moving the fields out — cheaper *and* correct. Step 09 caught the same shape by reading the
   emitted code; this time `-D warnings` did it, because generated code is compiled like any other.
   **That is an argument for committing it.**
3. **`boltffi generate swift` and `boltffi pack apple` fight over `dist/apple/Sources/`.** `generate`
   writes `Sources/Foo.swift`, `pack` writes `Sources/BoltFFI/Foo.swift`, and SwiftPM then refuses the
   package with `multiple producers`. I broke `mise run test:apple` this way and did not notice for an
   hour, because nothing ran it. `pack:apple:gen` now deletes `dist/apple` first.
4. **`#[boltffi::ffi_stream(item = …)]` in path form is silently not recognised** by `#[export]`. The
   method is then typed as returning `Arc<EventSubscription<T>>` and fails with a `WireEncode` bound
   error pointing at the wrong attribute. Generated code must `use boltffi::*;` and write a bare
   `#[ffi_stream]`.
5. **`gen-note` has no `custom` module, and that is why the escape hatch has a zero-composite path.**
   `gen-note-ffi` was generated before `gen-profile-ffi`, on purpose. Had it been the other way round,
   `use crate::custom::*;` would plausibly have been emitted unconditionally and `gen-note` could not
   have existed.

## Kill criteria

**None hit.** Two deserve their argument:

- **KC1** — `#[export] impl` / `#[ffi_stream]` from a non-root `mod`: **verified in M0 before anything
  was built on it**, which is the only reason the design survived. They work. What does *not* work is
  the pack-time metadata blob, and that was found by `pack`, not by M0. M0's artifact now carries the
  correction; my own table was overconfident about a column it never tested.
- **KC4** — "a generated binding forces a shell change that is not a rename." **62 declarations
  hand-written, 57 generated, 42 identical.** Every removal maps to an addition; nothing behaves
  differently. Full classification in
  [`artifacts/step-10-surface-delta.md`](artifacts/step-10-surface-delta.md). One item is not a rename:
  `trySetAvailability(start:end:)` becomes `trySetAvailability(raw:)`, because a generator sees
  `Value::Raw` as one type. The criterion's own words are *"changed the contract **without saying
  so**"*: the same two dates cross, in the same order, with the same validation and the same typed
  error, and this is the second time D20's shadow has been written down. Not hit — and if the ergonomics
  matter, the fix is a declaration-level `#[ffi(spread)]`, not a contract change.
- KC2 (`CheckId` across `#[data]`) — not hit; see headline 4.
- KC3 (a non-hermetic drift check) — not hit. Generation is source text in, source text out:
  `include_str!`, no subprocess, no boltffi CLI, no Xcode, no NDK.
- KC5 (a surviving mutation) — not hit, *after* six real survivors were closed with tests rather than
  notes.

---

## What is not done

**Deliverable 10 — repointing the four Swift and Kotlin shells at `gen-profile-ffi` — did not land.**
`mise run pack:apple` / `pack:android` / `test:apple` / `test:android` / `test:android:app` still build
and link the **hand-written** `spike-profile-ffi`. They pass, unchanged, because nothing about them
changed.

I stopped deliberately rather than half-migrate four codebases. What exists instead:

- `apple/gen-profile-smoke` — the generated bindings compile, link and run (7 tests).
- `artifacts/step-10-surface-delta.md` — the exact, classified, five-item work list.

That is honest evidence that the migration is mechanical, and it is **not** evidence that it is done.
Step 11 does it. The claim in ROADMAP's step-10 title — *"+ regenerate Swift/Kotlin"* — is unfulfilled,
and the shells have never linked a generated binding outside `gen-profile-smoke`.

## Still unverified, and owed

1. **`mise run bench:android:device` — still NOT RUN.** Third step running. No device attached; the verb
   refuses an emulator, as designed. Step 07's kill criterion 4 (per-keystroke round-trip ≤ 1.0 ms on
   physical silicon) still rests on step 05's emulator figure. Nothing in step 10 touches the hot path —
   `try_set_*` is one `draft_mut` lookup and one core call, as before — but **that sentence is exactly
   the one step 09 found to be false when it checked**, so read it as a claim, not a measurement.
2. **`mise run test:apple:ui` — still never run** in this project. Needs Xcode plus a logged-in GUI
   session holding Accessibility permission. Outstanding since step 06.
3. **The generated Android bindings have never been built.** `boltffi pack android` on `gen-profile-ffi`
   is untried; the `pack:android` workaround (step 05's upstream bug) would apply to it too. Kotlin's
   `AutoCloseable` shape and the `Cleaner` question (§9) are step 11's.
4. **Three upstream reports are written up here but not filed.** `pack android`'s missing expansion env
   (owed since step 05); generated methods not consulting `__boltffi_closed` (step 05's H2, and the
   reason D23 can only fix half the hazard); bindgen silently ignoring macro-generated items. All three
   are reproducible from artifacts in this repo.

## Verification sweep

```
mise run check                 319 tests   (283 at step 09)
mise run test:apple            39 probe + 14 VM   (hand-written; unchanged)
mise run test:apple:gen        7/7        GENERATED bindings: compile, link, run
mutation pass                  14 caught, 0 vacuous, 0 survived
mise run test:web              8/8
mise run test:android          44/44 on ART        (forced rerun)
mise run test:android:app      35/35 headless      (forced rerun)
mise run test:android:hazard   3/3                 (forced rerun)
mise run bench:android:device  NOT RUN — no device (the verb refused, as designed)
mise run test:apple:ui         not run — still GUI-gated
```

Of the 319: `bolted-decl` 13, `bolted-macros` 17, `bolted-ffi-gen` 10, `gen_profile_ffi` 14,
`gen_note_ffi` 1, `bolted-core` 11, `bolted-conformance` 3, `gen-note` 38, `gen-profile` 68,
`spike-note` 37, `spike-profile` 70, `profile-web` 37.
