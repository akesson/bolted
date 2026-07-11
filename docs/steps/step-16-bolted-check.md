# Step 16 — `bolted-check`: the constraint-surface snapshot

**Phase 4 — Verification harness. Status: ready.**

Phase 3 is done. The ROADMAP's old "natural step 16" — resuming the C# port — is gated on an upstream
fix that has not landed (the step-14/15 tripwire is still green), so it waits for its own step when the
tripwire flips. Phase 4's own gate is now **discharged**: ARCHITECTURE §9's "largest undischarged
claim", the unwritten `Feature` trait, is resolved as **D29** (v1.8, the step-16 planning pass) — §1 is
rewritten to the store-owned shape that shipped, the trait is struck, and the never-built `command` verb
is demoted to §9. Phase 4 opens here.

This step creates the `bolted-check` crate §5 names and gives it its **first analysis**: a
**constraint-surface snapshot** per feature — a committed, human-readable file capturing every
declared constraint, byte-drift-checked inside `mise run check`. It pays a debt **D27 wrote down by
name**: "constraint *tightening* is a build-time event — `bolted-check`'s constraint-semver snapshot
(Phase 4) fails the build until the team makes a version decision." Today a constraint edit is one
attribute token (`max = 30` → `29` in `#[bolted::value]`), regenerates cleanly through every existing
drift check, and reaches review as noise inside a 600-line regenerated `generated.rs` diff — while
silently stranding every stashed draft in the field whose raw no longer parses. This step makes that
change loud, isolated, and reviewable, and makes its failure message name the `STASH_SCHEMA_VERSION`
duty D27 assigned but could not enforce at runtime.

## What the planning pass verified (by reading the code, 2026-07-11)

- **`bolted-decl` already carries the whole declared surface — `bolted-check` is the third emitter over
  the one parser (D25), with zero new parsing.** `bolted_decl::Feature::from_file(&syn::File)`
  (`crates/bolted-decl/src/feature.rs`) scans a feature crate's `src/lib.rs` — the same input the four
  generator bins take. `ValueDecl` (`crates/bolted-decl/src/value.rs`) exposes `error_variants() ->
  Vec<ErrorVariant>` (idents + stable l10n keys + param names/types, including the `len_chars(min = 0)
  ⇒ no TooShort` subtlety D25 exists to protect), plus `Sanitizer`/`Validator`/`ParamTy` with full
  params, `error_ident()`, `is_text()`. `EntityDecl`/`EntityField` carry field order, field→value-type
  mapping, and `#[check(...)]` rule metadata; `Rule` carries tier-2 rule names + pins. **A snapshot
  renderer is a pure function of `bolted_decl::Feature` — text in, text out, no macro-expansion
  parsing.** (Its doc comment: "This type is why `bolted-decl` exists" — two emitters must not
  disagree; this is the third.)
- **`Constraint` is declared metadata, three variants**: `Required`, `LenChars { min, max }`,
  `Custom(&'static str)` (`crates/bolted-core/src/constraint.rs`). `#[bolted::entity]` **prepends
  `Required`** to every field's constraint list (`crates/bolted-macros/src/entity.rs:108` —
  `<Entity>Field::constraints(self) -> Vec<Constraint>`); every entity field is non-optional (D13).
- **Composites are invisible to the declaration by design (D20), but reachable at rung 1.**
  `DateRange` is hand-written (`crates/gen-profile/src/value_types.rs:82`, `impl Value`); a pure
  source-scan cannot see its `Constraint::Custom("start_le_end")`, and `Feature::value(&ident)` returns
  `None` for it. But its constraints **are** reachable through the macro-emitted runtime accessor
  `ProfileField::Availability.constraints()`. So composite coverage rides a **runtime section** whose
  data a per-feature drift test (which links the feature crate) passes into the renderer — the renderer
  stays text-only, and cross-checks the runtime field list against the declared field list so an
  omitted field is a failure, not a silent gap.
- **`STASH_SCHEMA_VERSION` is a hardcoded constant in the generator, stamped per feature.**
  `crates/bolted-ffi-gen/src/dto.rs:190` emits `pub const STASH_SCHEMA_VERSION: u32 = 1;` into every
  feature's `generated.rs` (`crates/gen-profile-ffi/src/generated.rs`, `gen-note-ffi/src/generated.rs`);
  the emitted wrapper stamps it into every `stash()` and gates `accept_stash` on it (D27's wholesale
  typed refusal, `wrapper.rs`). Kotlin carries it only as **data** (`ProfileStashCodec.kt` round-trips
  the field; the gate is Rust-side). Changing the *derivation* would touch `dto.rs`, the two
  regenerated `generated.rs`, and the wrapper tests — not the Kotlin codec.
- **The drift pattern to copy is `crates/gen-profile-ffi/tests/drift.rs`.** It `include_str!`s both
  sides and calls a `bolted_ffi_gen::check_*_drift` library fn that panics with a first-differing-line
  message; it runs inside `mise run check` (= `cargo fmt --check` + `clippy -D warnings` + `cargo test
  --workspace`) on a box with no boltffi CLI / Xcode / NDK. The foreign-file half uses **byte** equality
  with an explicit rationale — "nothing formats a foreign generated file, which is what makes the
  comparison honest." A `.snap` file is in exactly that category: **no formatter owns it**, so byte
  comparison is the honest comparison. `bolted-check` is **not** yet a workspace member
  (`Cargo.toml` `members`) — M0 adds it.

## What earlier steps hand over (use it, don't re-derive it)

- **The one parser (D25):** `bolted_decl::Feature` + `ValueDecl::error_variants()` — the entire input.
- **The drift-test skeleton (D22/D28):** `include_str!` + a library `check_*_drift` fn + first-diff
  panic; regeneration lives in the separate `gen:ffi` task, never in the test (a verb that rewrites a
  file cannot verify it).
- **The byte-vs-code distinction (D28):** Rust generated files are `prettyplease`-formatted and compared
  as *code*; foreign/unowned files are compared as *bytes*. A `.snap` is unowned → bytes.
- **D27's recorded duty:** the runtime half (versioned envelope, wholesale refusal at the parse gate,
  per-field salvage) shipped in step 12; the build-time half — "warn the team a tightening happened" —
  is explicitly `bolted-check`'s, and explicitly Phase 4. This step is that half.

## Scope: one crate, one analysis, two committed snapshots, the D27 wire

`bolted-check` starts as **one analysis** and deliberately does **not** absorb the existing D22/D28
drift checks (that is churn with no new verification). It is a workspace **library crate + one
`gen-constraints` bin**, depending on `bolted-decl` **only** — never boltffi, never `bolted-ffi-gen`
(analyzer and emitter are different seams over the same parsed declaration). Enforcement is per-feature
drift tests inside the existing `cargo test --workspace`; **no new mise verb in `check`**, no CLI
product. Snapshots are committed beside each declaration; the git diff of a `.snap` line
(`min: 3 → min: 4`) is itself the review artifact.

## Deliverables

1. **`crates/bolted-check`** — library (`render_constraint_snapshot(feature: &bolted_decl::Feature,
   runtime: &RuntimeSurface) -> Result<String, RenderError>`; deterministic, line-oriented,
   format-versioned header) + a `gen-constraints` bin that reads a feature's `src/lib.rs` and prints the
   snapshot. Workspace member. Depends on `bolted-decl` only.
2. **Committed snapshots** `crates/gen-note/constraints.snap` and `crates/gen-profile/constraints.snap`,
   capturing per **value**: raw type, sanitizers *in order* (they run before validation), validators
   with params, error variants + keys + param types; per **entity**: fields in declaration order, each
   field's value type and `declared`-vs-`custom`, check rule + keys; tier-2 rules + pins; a **header**
   carrying the feature's `STASH_SCHEMA_VERSION`; a **runtime section** listing each `FieldId`'s full
   `constraints()` (Required + intrinsics — this is where composites are covered).
3. **Drift tests** `crates/gen-note-ffi/tests/constraint_snapshot.rs` and
   `crates/gen-profile-ffi/tests/constraint_snapshot.rs` — `include_str!` both sides, **byte** compare,
   first-differing-line failure message that states the duty: *"the constraint surface changed — review
   the diff, decide whether `STASH_SCHEMA_VERSION` must move (D27), then `mise run gen:ffi`."* (Hosted in
   the `-ffi` crates because those already link both the feature crate — for the runtime
   `FieldId::constraints()` — and the generated module — for `STASH_SCHEMA_VERSION`.)
4. **`gen:ffi` grows two `gen-constraints` invocations** (it is already the regenerate-every-committed-
   artifact verb since D28). `mise run check` stays untouched.
5. **The D27 derivation decision, recorded** (in the step report and the snapshot header comment): the
   schema version **stays a human-bumped constant**. Constraint-hash auto-bump is D27's *own rejected
   alternative* (loosening a max would kill every stash for no reason); tool-managed bumping needs a
   breaking/non-breaking classifier that does not exist (see kill criterion 4 / non-goals). The snapshot
   showing constraints and version *in one diff* is the enforcement D27 promised.
6. **Falsification** — every new check watched red (M4).
7. **Report + ROADMAP** (`step-16-report.md`).

## Milestones

- **M0 — the crate seam.** `crates/bolted-check` (lib + `gen-constraints` bin) added to `Cargo.toml`
  `members`; the renderer with unit tests over `include_str!`d `gen-note`/`gen-profile` sources; a
  **determinism test** (render twice → byte-equal; `BTreeMap`/sorted iteration, no `HashMap` in the
  output path). Commit.
- **M1 — snapshots + drift.** Generate and commit both `.snap` files via `gen-constraints`; the two
  drift tests; wire `gen:ffi`; `mise run check` green. Commit.
- **M2 — the runtime/composite section.** The `-ffi` tests pass each field's `constraints()` into the
  renderer; the renderer **refuses** a runtime list that does not cover exactly the declared fields (an
  omitted field is a failure). Prove `DateRange`'s `Custom("start_le_end")` appears in
  `gen-profile`'s snapshot. Commit.
- **M3 — the D27 wire.** Header carries `STASH_SCHEMA_VERSION` (read from the generated module *by the
  test* and passed in — the renderer stays text-only); the failure message names the version duty; the
  decision text (deliverable 5) lands in the report and the header comment. Commit.
- **M4 — falsification.** Tighten `PersonName` `max = 30 → 29` in `crates/gen-profile/src/lib.rs`,
  watch the drift test fail at the right line; hand-edit one `.snap` line, watch red; delete a
  `constraints.snap` path referenced by `include_str!`, confirm it is a **compile error** (not a skip);
  restore every mutation via `mise run gen:ffi` (never `git checkout` — step-13 friction 3), `touch`
  after restore. Commit.
- **M5 — report + ROADMAP.** Commit.

## Kill criteria (real; if hit, stop and report)

1. **The snapshot can only be produced by parsing macro-expanded output, or `bolted-check` needs a
   `boltffi` / `bolted-ffi-gen` dependency.** That breaks D25 (one parser) and §5's crate seams — a
   design fault, not an implementation one. Stop.
2. **Composite coverage forces the renderer to stop being text-only** — i.e. it forces `bolted-check`
   itself to link and *execute* feature crates rather than having the per-feature test pass the runtime
   `constraints()` in. That inverts the dependency direction (the analyzer would need editing per
   feature) and is a design question. Stop.
3. **The drift check cannot run inside plain `cargo test --workspace`** (needs a toolchain, network, or a
   generated file that does not yet exist at check time). Rung 3 inside the one verb every machine runs
   is the whole point; a separate verb is a worse design. Stop.
4. **Any-diff-fails proves unlivable in this session** — the snapshot churns on edits that are visibly
   *not* constraint changes (doc comments, field reordering the macro already tolerates, formatting
   leaking into the projection). Fix the renderer's projection; if it cannot be projected stably, stop —
   a noisy tripwire trains people to regenerate blind, which is worse than no tripwire.

## Non-goals (→ elsewhere)

- **The breaking/non-breaking classifier.** `Custom` predicates are opaque names (a semantic change
  inside `email()` never shows; a rename is neither tightening nor loosening) and sanitizer changes
  (adding `lowercase`) change what a raw *becomes* — neither. A classifier now is designed from **zero
  real migrations** (the D20/D21 precedent, thrice). Reopens with the first real constraint migration.
- **Auto-derived or tool-bumped `STASH_SCHEMA_VERSION`** — deliverable 5 records why it stays a constant.
- **A `bolted-check` CLI product or any new mise verb** — the crate ships one analysis behind
  `cargo test`.
- **Migrating the existing D22/D28 drift checks into `bolted-check`** — churn, no new verification.
- **The WASM size budget** (step-04's 304 KiB / 85 KiB-brotli baseline). A good *second* analysis, but a
  different tier: it needs `trunk build --release` + the wasm32 target and cannot live at rung 3 inside
  host-only `mise run check`. Its own step.
- **`doctor`, `bolted new`, capability coverage** — later Phase-4 sketches.
- **The C# resume** — tripwire-gated, its own step.
- **Snapshotting `spike-*`** — hand-written, no declaration to scan; the conformance suite owns them.
- **Anything upstream / anything filed.**

## Inherited cautions

- **A forbidding test can forbid nothing** (step 10): every planted red must be *watched* red — M4 is
  not optional, and the `include_str!` path guard must be seen to be a compile error.
- **A drift check makes a mutation pass vacuous** (step 10): the constraint snapshot is regenerated by
  `gen:ffi`, not by the test — never regenerate inside the falsification, or the mutation greens itself.
- **Restore generated files with `mise run gen:ffi`, never `git checkout`, and `touch` after** (step-13
  friction 3 — the mtime trap).
- **Nothing may format a `.snap` file** — no rustfmt, no `.editorconfig`, no hook on the path. That is
  what keeps the byte comparison honest (D28). `cargo fmt --all` after `gen:ffi` is for the *Rust* half
  only.
- The package-name trap: `gen_profile_ffi` (crate) vs `gen-profile-ffi` (dir) (step-13 friction 2).
- Commit per milestone; never `git -C`; build/test only via `mise run check` / `mise run gen:ffi`.

## Exit checklist

- [ ] `crates/bolted-check` in the workspace; renderer text-only over `bolted-decl`; **no boltffi
      anywhere near it**; determinism test green.
- [ ] Both `.snap` files committed; both drift tests green **inside `mise run check`**; `mise run
      gen:ffi` regenerates them; `check` untouched (no new verb).
- [ ] Composites covered via the runtime section; the field-coverage cross-check proven to **fail on
      omission**; `DateRange`'s custom constraint visible in `gen-profile.constraints.snap`.
- [ ] `STASH_SCHEMA_VERSION` visible in each snapshot header; the drift failure message names the D27
      duty; the derivation decision recorded.
- [ ] Every new check watched **red** (constraint tightening at the right line; hand-edit; `include_str!`
      path guard as a compile error) and restored green via `gen:ffi`.
- [ ] `step-16-report.md` written; ROADMAP row updated; **ARCHITECTURE untouched by the implementation
      session** — v1.8 already carries D29 from this planning pass.
