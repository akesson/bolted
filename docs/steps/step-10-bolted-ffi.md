# Step 10 — `bolted-ffi`: a generated FFI surface

**Phase 3 — Framework extraction. Status: ready.**

Extract the FFI layer from `spike-profile-ffi` (1 407 hand-written lines) the way step 09 extracted the
feature layer: one declaration, one generator, and the hand-written crate kept as the reference the
generated one is read against.

> **Process note** (carried from step 09, and now due). `CLAUDE.md` splits planning (Fable) from
> implementation (Opus). This step doc was authored by the implementer, for the fifth time — steps 06,
> 07, 08, 09 and now 10. A rule bent five times is not a rule. Either the split is real and a planning
> session authors step 11, or `CLAUDE.md` should be rewritten to describe what actually happens.
> Flagged, not resolved.

---

## What step 09 handed over, and what it got wrong

Step 09 cut `#[bolted::feature_model]` (D21) because *"it needs boltffi, and `bolted-macros` may not
import boltffi"*. **That reason is wrong**, and the correct one is worse.

A proc macro emitting `#[data]` tokens never imports boltffi. But BoltFFI 0.27.3 discovers its FFI
surface by reading the crate's source files off disk and parsing them with `syn`
(`boltffi_scan::SourceTree::load` → `read_to_string` → `syn::parse_file`), walking `mod` declarations.
Even the `BINDING_EXPANSION` path — the one `mise run pack:android` already works around — calls
`scan_package(ScanInput::new(&self.source, …))` and re-reads the file. Expansion mode governs *where
the metadata blob is emitted*, not *what bindgen can see*.

Measured, not inferred (M0 reproduces it):

| where the `#[data]` / `#[export]` items live | `cargo build` | `boltffi generate swift` |
|---|---|---|
| hand-written in `src/lib.rs` | ✅ | ✅ |
| emitted by a proc macro | ✅ | ❌ **silently absent** |
| in a committed `mod generated;` file | ✅ | ✅ |
| `include!`d (i.e. from `OUT_DIR`) | ✅ | ❌ **silently absent** |
| in a **dependency** crate that itself depends on boltffi | ✅ | ✅ |

`boltffi generate swift` exits **0** on the failing rows and prints nothing. A framework whose FFI
surface silently fails to exist is worse than one that cannot generate it at all.

Two consequences, and they are the step:

1. The FFI layer cannot be produced by an attribute macro. It must exist as literal source text.
2. Shared `#[data]` types **can** live in a dependency crate. §9's "FFI dedup of field-state families"
   is therefore a real choice, not a wish.

---

## Decisions taken before implementation (→ ARCHITECTURE §8, D22–D25)

Four structural questions were put to the owner with their alternatives. All four are recorded here so
the implementation has no latitude to re-decide them.

### D22 — the FFI layer is **generated source, committed, and drift-checked**

`bolted-ffi-gen` turns a declaration into Rust source text. `mise run gen:ffi` writes
`crates/<feature>-ffi/src/generated.rs`. A test inside `mise run check` regenerates it in memory and
asserts byte equality with the committed file.

*Rejected:* a `build.rs` writing into `src/` (depends on cargo running before boltffi's scan, and keeps
the FFI layer out of review); fixing boltffi's bindgen upstream first (right, eventually — it blocks
this step on a crate we do not control); hand-writing the FFI layer forever with `bolted-check`
verifying it (concedes the framework's promise for the layer with the most boilerplate).

*Why this is not a consolation prize.* §5 calls macro output "the least-verifiable code on the ladder".
A committed, formatted, reviewable `generated.rs` is **strictly better** than a macro expansion nobody
reads: it can be diffed against `spike-profile-ffi`, mutated, and reviewed. The drift test is VISION's
rung 3 — verified at build time — landing on the layer that most needed it, one phase early.

### D23 — a typed `DraftClosed` for the hazard that is ours; the other is BoltFFI's

Two distinct hazards were being described with one sentence:

- **H-a — the draft is gone store-side, the foreign object is alive.** Reachable today: C17 says a
  successful `submit` releases the draft, and the Swift/Kotlin object survives it. Every mutating
  method then takes the `draft_mut(id) → None` branch and returns `Ok(())`. **A silent no-op.** This is
  ours, and it becomes a typed refusal.
- **H-b — the foreign object was released, and is then used.** Generated Kotlin already holds the flag
  it needs and never reads it:

  ```kotlin
  class ProfileDraftFfi internal constructor(internal val handle: Long) : AutoCloseable {
      private val __boltffi_closed = AtomicBoolean(false)
      override fun close() { if (__boltffi_closed.compareAndSet(false, true))
          Native.boltffi_release_class_…_profile_draft_ffi(handle) }   // frees the Rust object
      fun trySetUsername(raw: String) { Native.boltffi_method_…_try_set_username(this.handle, …) }
      //                                ^^ never consults __boltffi_closed → dangling pointer, silent UB
  }
  ```

  No Rust we write runs before the dereference. **Not fixable from this side.** It is reported upstream
  (M8), with the observation that the fix is a guard on a field that already exists.

Scope of the typed error: the **mutating** verbs — `try_set_*`, `resolve_*`, `run_*_check`, `submit`.
Observers (`snapshot`, `validate`, `stash`, `is_live`) keep their total shapes; `is_live()` exists
precisely so a shell can ask before it acts. Smallest reversible choice; recorded for the report.

*Rejected:* moving every method onto the store and passing `DraftId` as a `u64` (eliminates H-b by
construction, and D16 set it up — but it rewrites the entire FFI surface and both shells in the same
step that introduces the generator; revisit in step 11 if H-b's upstream fix stalls). *Rejected:* probe
and defer both.

### D24 — one field-state DTO per **raw** type, hosted in `bolted-ffi`

`Username`, `PersonName` and `Email` all have `Raw = String`, and `dto.rs` stamps three structurally
identical `…Validity` / `…FieldSync` / `…FieldState` families for them. Because a dependency crate's
`#[data]` types are visible to bindgen (the table above), one `TextValidity` / `TextFieldSync` /
`TextFieldState` can live in `bolted-ffi` and be written once.

This is exactly the residue D19 left behind: generics dedup on the axis that varies (the value type),
`#[data]` cannot, and the axis that actually varies *across the boundary* is the **raw** type.

Error types stay per value (`UsernameErrorFfi` ≠ `EmailErrorFfi`) — they carry different variants, and
typed `throws` is a feature. What is lost is per-field type naming: Swift sees
`snapshot.username: TextFieldState`. The field name carries the meaning; the type name never did.

*Rejected:* one family per value type (preserves naming, pays N× duplication per feature).

### D25 — the declaration is parsed once, by a crate both consumers share

`bolted-decl` holds the declaration model (`ValueDecl`, `EntityDecl`, `Check`, `Validator`, `Rule`) and
its `syn` parsers. `bolted-macros` consumes it to emit the feature; `bolted-ffi-gen` consumes it to emit
the FFI. A proc-macro crate cannot export ordinary items, so this is forced — but it is also the point:
**two parsers would be two contracts**, and the drift check would be checking a generator against
itself.

The generator scans the feature crate's source with `syn`, exactly as boltffi scans ours. A field whose
value type was **not** declared in that source (`availability: DateRange`, composite, hand-written per
D20) is not guessed at: the generator emits a reference to a `custom` module the feature's FFI crate
must supply. A missing projection is a **compile error**, not a wrong binding.

---

## Crate layout after this step

```
bolted-core        traits + generics; sans-io, lock-free; NEVER depends on boltffi
bolted-decl        the declaration model + parsers                     (new)
bolted-macros      value / entity / rules — emits the feature          (now consumes bolted-decl)
bolted-ffi-gen     emits the FFI layer as Rust source text             (new; no boltffi dep)
bolted-ffi         shared #[data] DTOs; the ONLY hand-written crate importing boltffi   (new)
bolted-conformance C01–C22, generic over a feature
gen-note-ffi       mod generated;  ← committed, drift-checked          (new)
gen-profile-ffi    mod generated;  + src/custom.rs (the composite)     (new)
```

Note that "the only crate importing boltffi" survives, in a sharper form: `bolted-ffi` is the only
**hand-written** one. Each `<feature>-ffi` crate imports boltffi too, but every line of it is generated
from a declaration and byte-checked against that declaration on every `mise run check`.

---

## Deliverables

1. **M0 — the visibility probe.** `docs/steps/artifacts/step-10-boltffi-visibility/` — a script that
   builds the five-row table above from scratch, plus its recorded output. The finding must not live
   only in a report.
2. **`bolted-decl`** — model + parsers, extracted from `bolted-macros`.
3. **`bolted-macros`, refactored** onto `bolted-decl`. *Proof of no behaviour change: every step-09
   test and every golden snapshot passes unchanged.* If a snapshot moves, the refactor was not one.
4. **`bolted-ffi`** — `Param`, `ErrorData`, `ConstraintFfi`, `DraftStatusFfi`, `CheckStateFfi`,
   `DraftClosedFfi`, and the `Text*` family (D24). Everything that does not mention a feature's
   `FieldId`.
5. **`bolted-ffi-gen`** — `Model → String`. Golden snapshots via `prettyplease`, `BLESS=1` to rewrite,
   no tool outside `mise run check` (step 09's rule).
6. **`gen-note-ffi`**, written **before** `gen-profile-ffi`. A generator with one input is shaped like
   that input; step 09 learned this and the lesson has not expired. `gen-note` has no rule, no check,
   no composite — so the zero-check and zero-rule paths are exercised first, not retrofitted.
7. **`gen-profile-ffi`** — generated, plus `src/custom.rs` for the composite (`PlainDate`,
   `PlainDateRange`, the `Availability*` family, `DateRangeErrorFfi`).
8. **The drift test**, inside `mise run check`, for both crates. Hermetic: generation is
   declaration → text, so it needs no boltffi CLI, no Xcode, no NDK.
9. **`mise run gen:ffi`** — writes the committed files.
10. **Both shells run on generated bindings.** `pack:apple` / `pack:android` repoint at
    `gen-profile-ffi`; the Swift and Kotlin apps and probes are updated for D24's rename and D23's new
    error, and the existing suites pass: `test:apple`, `test:android`, `test:android:app`,
    `test:android:hazard`.
11. **Three upstream reports**, written up as artifacts: `pack android`'s missing expansion env (owed
    since step 05); generated methods not consulting `__boltffi_closed` (H-b); bindgen silently
    ignoring macro-generated items (this step).
12. **A mutation pass over the generator**, checked in as
    `docs/steps/artifacts/step-10-mutations.py`. Step 09 found two invariants this way and one of its
    own mutations was vacuous — both lessons apply.

---

## Kill criteria (real; if hit, stop and report)

1. **`#[export] impl` / `#[ffi_stream]` do not work from a non-root `mod`.** The whole design rests on
   `mod generated;` being ordinary source. Verify in M0, before anything is built on it.
2. **`ProfileCheck` cannot cross `#[data]`.** Step 09's KC4, assessed there by inspection only. If a
   `CheckId` enum cannot be projected, **D18 is wrong** and the async check's surface belongs
   elsewhere.
3. **The drift check cannot be made hermetic** — if regeneration needs the boltffi CLI or a platform
   toolchain, `mise run check` grows a dependency it has refused since step 02.
4. **A generated binding forces a shell change that is not a rename.** D24 renames types and D23 adds
   an error; anything beyond that means the generated FFI is not behaviourally a drop-in for the
   hand-written one, and the extraction has changed the contract without saying so.
5. **A mutation of the generator survives the suite.**

---

## Non-goals (→ step 11, "FFI hardening & per-language contract tests")

Deferred deliberately, each with the reason it can wait:

- `java.lang.ref.Cleaner` backstop (§9) — depends on how H-b's upstream report lands.
- `@Parcelize` / `Codable` on DTOs — a shell that persists one hand-writes a codec today; it works.
- **Per-language contract tests generated from the C-IDs** — the largest single item; it needs the
  generator this step builds.
- l10n key coverage per target; the Compose parameter-passing rule; the platform-stdlib
  name-collision policy (`Date`, `URL`, `Data`, `Error`) — `dto.rs` already dodges it with `PlainDate`,
  and the dodge is recorded.
- Deleting `mise run pack:android`'s workaround — not ours to delete until boltffi ships the fix.

Not this step, and not step 11: **the `Feature` trait** (its own design session, before Phase 4) and
**composite value objects in `#[bolted::value]`** (§9: do not design it from one example).

`spike-profile-ffi` is **not deleted.** It is the reference. A step that edits its own reference proves
nothing.

---

## Exit checklist

- [ ] `mise run check` green, including both drift tests.
- [ ] `boltffi generate swift` on `gen-profile-ffi` produces the surface; on `gen-note-ffi` too.
- [ ] Every step-09 golden snapshot unchanged (M1's proof).
- [ ] `test:apple`, `test:android`, `test:android:app`, `test:android:hazard` green on **generated**
      bindings.
- [ ] Mutation pass: every mutation caught, and every survivor proved to differ from the original
      before it is called a finding.
- [ ] `docs/steps/step-10-report.md`; ARCHITECTURE §8 gains D22–D25 and §9 loses two questions;
      ROADMAP updated and steps renumbered (11 = hardening, 12 = C#).
- [ ] `bench:android:device` and `test:apple:ui` — state their status honestly. They are still owed.
