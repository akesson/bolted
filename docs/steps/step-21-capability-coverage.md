# Step 21 — Capability coverage: the capability is a checkout argument

**Phase 4 — Verification harness. Status: ready.**

The topology design pass unblocked this analysis by settling what OS surfaces demand of
capabilities (steps 18–20, D30–D33): surfaces are heterogeneous — badge-only observers, a
context-menu command, one full editor — and a capability is the **surface's own OS access** (step
19 U2: the client's filesystem access *is* the folder-check capability; `CheckToken` never
crosses). Coverage must therefore accommodate a surface that legitimately *has no* capability, not
just police one that forgot it.

## The defect this step closes

VISION's in-scope list promises "capability traits declared in Rust, implemented per platform,
**coverage-checked per target**". Today nothing checks it, at any rung. A feature declares its
async check once (`#[check(...)]` on the field, `crates/gen-profile/src/lib.rs:76`), the generator
emits the capability trait (`UsernameChecker`), a settable slot, and the single-flight driver —
and then:

- the slot defaults to `None` and **nothing forces a shell to ever call `set_username_checker`**;
- `run_username_check` on a checker-less live draft is a **silent no-op** (`Ok(false)`,
  `crates/gen-profile-ffi/src/generated.rs:654`);
- the omission surfaces as **C16's runtime submit refusal** (`username_check_required`), i.e. a
  broken product flow discovered by the user, with no in-app path to fix it.

Correctness is preserved (C16 is the floor working as designed), but the *wiring omission* is
glue that can only fail at runtime — the exact thing the founding rule forbids. The per-shell
probe tests assert the no-op returns `false`; none asserts that a real shell remembered to wire
anything.

## The design (decided in this planning pass — D34, ARCHITECTURE v1.10)

**The capability moves from a settable slot to an explicit argument of every generated draft
entry point.** For each declared check, the generated `checkout(...)` and `restore(...)` take
`Option<Box<dyn XChecker>>` (an optional/nullable parameter in Swift/Kotlin/C#); the
`set_*_checker` method and the silent default are **deleted**.

- **Forgetting becomes a compile error** (rung 2, the platform compiler) at every call site, on
  every FFI target, with zero new analysis apparatus.
- **`nil` is a declared absence, not an omission.** A surface that structurally lacks the
  capability (a sandboxed extension without network) writes `checkout(usernameChecker: nil)` —
  visible in review, honest at runtime: the check never runs and C16 refuses a dirty pinned field
  with `required_key`, exactly today's semantics. This is why the parameter is optional rather
  than mandatory: a mandatory parameter would force such a surface to fabricate a stub checker,
  and a stub's lying `Pass` on a dirty field is strictly worse than C16's typed refusal.
- **The planned rung-3 analysis dissolves** (the D19/KC2 pattern): there is nothing left for
  `bolted-check` to scan on FFI targets, because the uncovered state is no longer representable.
  In-process Rust shells (profile-web) have no generated seam to enforce at; their floor stays
  C16, and inventing core API for it would design from one consumer (D20). Recorded, not built.

Rejected alternatives (full rationale in ARCHITECTURE §8, D34): the mandatory parameter (forces
lying stubs); a committed coverage manifest + source-text scan inside `check` (token-presence is
weak verification, new apparatus, and it polices a state better made unrepresentable); the status
quo (C16 alone — typed, but rung 4 for a wiring defect a compiler can catch).

## What the planning pass verified (by reading the code, 2026-07-17)

- **The emission site is one function pair.** `crates/bolted-ffi-gen/src/wrapper.rs`: `checkout`
  (:295) and `restore` (:328) both stamp `checker_slots(feature)` (:380) — `username_checker:
  Mutex::new(None)` — and the setter/driver pair is emitted at :438–:514. The change is: emit one
  parameter per declared check on both entry points, initialize the slot from it, stop emitting
  the setter. The driver keeps its take-out-of-the-mutex reentrancy dance (:466–488) — during the
  outcall the slot is legitimately `None`, so the `Ok(false)` branch **stays**, its meaning
  narrowed to "declared absence, or reentrant call during the outcall".
- **Params + handle returns already cross boltffi** (`restore(accepted) -> ProfileDraftFfi`,
  `set_username_checker(Box<dyn UsernameChecker>)`); the **novel** shape is an
  `Option<Box<dyn Trait>>` *parameter*. Empirical gate, not assumed — M3 proves it on Swift and
  Kotlin via the smoke/probe tiers (kill criterion 1 names the fallback).
- **Blast radius, measured** (grep for `set_username_checker|setUsernameChecker`): the generator +
  `gen-profile-ffi` (generated.rs, tests/wrapper.rs) + the two emitted conformance suites
  (regenerate via `gen:ffi`) + Swift shells (profile-app VM + l10n tests, profile-probe ×3 test
  files, gen-profile-smoke) + Kotlin shells (same three) + C# probe (TestSupport.cs,
  CallbackDriverProbe.cs). `gen-note*` untouched (no check ⇒ no parameter — golden.rs:245 already
  pins "no check: no capability trait"). `spike-profile-ffi` is a pre-D34 hand-written fossil
  no shell links; it keeps its setter and is not migrated.
- **The one shipped capability family is the async check.** `bolted_decl::entity::Check` is the
  whole declared model; nothing else in six spikes produced a capability. D34 is scoped to it
  (D20: no registry designed from zero further examples).
- **The daemon topology is consistent with this shape**: over the wire the checker never crosses
  (D31 requirement 1) — the *client library's* draft session owns the capability, so a generated
  wire client inherits D34's checkout signature on the client side. One sentence lands in the
  pricing artifact; nothing is built (D31's gate stands).

## Deliverables

1. **Generator**: `wrapper.rs` emits capability parameters on `checkout`/`restore`; setter
   emission deleted; driver comment updated. `golden.rs` pins **both sides**: the parameter is
   present in the checkout signature *and* `set_` + `Checker` never co-occur in emitted source
   (needle verified against the real prettyplease output, step-10 lesson).
2. **Regenerated committed source**: `gen-profile-ffi/src/generated.rs`, both emitted
   conformance suites (`ProfileConformanceSuite.swift/.kt`) via `foreign.rs` template updates.
3. **Migrated shells**: apple (app, probe, smoke), android (app, probe, smoke), csharp (probe).
   App VMs pass their real checker at checkout/restore; probe tests that asserted the *forgotten*
   state now assert the *declared-absence* state (`nil` → driver `Ok(false)` → C16 floor) — the
   tests get truer, not deleted.
4. **Tier evidence**: `mise run check`, `test:apple`, `test:android`, `test:csharp` green (the
   platform tiers are the Option-trait-object empirical gate).
5. **Falsification**: the rung-2 claim watched red — omit the argument in one Swift and one
   Kotlin call site, record the verbatim compile errors; generator mutations (re-emit setter /
   drop parameter) each fail golden or drift.
6. **Docs**: ARCHITECTURE v1.10 (D34 + §2 capability paragraph + §9 closed-list) — *this planning
   pass*; report + ROADMAP + one pricing-artifact sentence — the implementation half.

## Milestones

- **M0 — planning artifacts** (this pass): step doc, ARCHITECTURE v1.10, ROADMAP row. Commit.
- **M1 — the generator**: wrapper.rs + golden.rs; `mise run gen:ffi`; migrate
  `gen-profile-ffi/tests/wrapper.rs`; `mise run check` green. Commit.
- **M2 — the foreign emitters**: `foreign.rs` suite templates construct drafts with the
  capability argument; regenerate committed suites; drift checks green inside `check`. Commit.
- **M3 — the shells**: migrate Swift/Kotlin/C# call sites; `test:apple`, `test:android`,
  `test:csharp`. This is the kill-criterion-1 gate. Commit.
- **M4 — falsification**: watched reds per deliverable 5; restore green via `gen:ffi` (never
  `git checkout`; `touch` after). Commit.
- **M5 — report + ROADMAP + pricing sentence.** Commit; PR.

## Kill criteria (real; if hit, stop and report)

1. **`Option<Box<dyn Trait>>` cannot cross as a parameter** on a live backend (codegen error,
   foreign compile error, or runtime marshalling failure). Fallback to price *before* abandoning:
   paired named entry points (`checkout(usernameChecker:)` /
   `checkoutWithoutUsernameChecker()`) — the absence stays explicit in the name. If the fallback
   also fails or turns the surface combinatorial (features with several checks), stop: upstream
   filing candidate + design session.
2. **A real surface needs to swap a checker mid-draft** (the setter turns out load-bearing).
   That would mean the capability is not checkout-scoped state — a design fault in D34. Stop.
3. **Any conformance/C-ID regression** not attributable to mechanical call-site migration. Stop.
4. **The migration forces a shell to fabricate a non-nil stub checker** to get through a flow
   that has no honest implementation — the exact hazard the optional parameter exists to avoid.
   Design fault. Stop.

## Non-goals (→ elsewhere)

- **A capability registry / families beyond the async check** — one family exists; a registry
  now is D20's error. Reopens with the second capability family.
- **Target/product completeness** ("does the product ship a WinUI shell at all?") — that is
  `bolted new` / product-manifest territory, not capability coverage.
- **The wire emitter** (D31 — gated on a product feature needing the daemon topology); this step
  contributes one sentence to its price list.
- **Migrating `spike-profile-ffi`** — a hand-written pre-D34 fossil, no shell links it.
- **In-process Rust-shell enforcement** — no generated seam; C16 stays the floor; recorded in
  D34, revisit with a second Rust surface.
- **`doctor`, `bolted new`** — later Phase-4 sketches. **C# resume** — tripwire-gated.
- **Anything upstream / anything filed.**

## Inherited cautions

- **Read check verdicts from an explicit exit-code echo** (`>log 2>&1; echo "check exit=$?"`),
  never through a pipe; never chain a commit after a check with `;`.
- **A forbidding test can forbid nothing** (step 10): golden needles must be written against the
  real prettyplease output and watched red via a generator mutation before being trusted.
- **A drift check makes a mutation pass vacuous** (step 10): regenerate *before* judging a
  generator mutation, or the drift test catches it instead of the golden.
- **Restore generated files with `mise run gen:ffi`, never `git checkout`, and `touch` after**
  (step-13 friction 3).
- **`test:android` can report BUILD SUCCESSFUL without running a test** (Gradle up-to-date);
  force `--rerun-tasks` semantics per step-11's caution and read counts from the JUnit XML.
- The package-name trap: `gen_profile_ffi` (crate) vs `gen-profile-ffi` (dir).
- Commit per milestone; never `git -C`; build/test only via mise verbs.
- The C# driver is still broken upstream (step-15 tripwire green): `test:csharp` verifies the
  *compile-level* shape of the new signatures and the non-driver tests; the driver probe stays
  the tripwire, not a regression.

## Exit checklist

- [ ] `checkout`/`restore` take the declared capabilities as optional parameters on every FFI
      target; `set_*_checker` exists nowhere in generated source; golden pins both sides.
- [ ] `gen-note*` byte-identical (no check ⇒ no parameter).
- [ ] All committed generated artifacts regenerated via `gen:ffi`; `mise run check` green.
- [ ] Swift + Kotlin + C# shells migrated; `test:apple` / `test:android` / `test:csharp` green.
- [ ] The rung-2 claim watched red: verbatim Swift and Kotlin compile errors recorded in the
      report; generator mutations caught by golden/drift.
- [ ] ARCHITECTURE at v1.10 (D34) from the planning pass; report + ROADMAP + pricing sentence
      from the implementation half.
