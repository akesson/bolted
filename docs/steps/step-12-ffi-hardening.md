# Step 12 — FFI hardening

**Phase 3 — Framework extraction. Status: ready.**

Step 11 put all four shells on generated bindings and proved nothing regressed. This step makes the
generated layer *safe to hold*: the lifecycle bugs a shell can still write get a test or a typed
refusal, the stash gets its version gate before the hand-written codec that carries it today is
deleted, and the ergonomics items measured across steps 05–11 land as generator changes. The two
ARCHITECTURE §9 questions that blocked this step are **resolved — D26 and D27, v1.5** — by the design
pass that authored this doc; their implementation slices are deliverables here, their rationale is not
(read §8 first).

> **Process note.** This is the first step doc authored the way `CLAUDE.md` describes — in a planning
> session, for a separate implementation session. Steps 06–11 were all authored by their implementer;
> step 11 was implemented in the planning session at the owner's request. Whether the split earns its
> keep is now a live experiment: the report should say where this doc was wrong in ways an
> implementer-author would have known.

---

## Scope: hardening only — the contract-test generator is step 13

ROADMAP's old step-12 sketch ("FFI hardening **+ per-language contract tests**") was again two steps.
Generating per-language contract tests from the C-IDs is a new generator *target* — test code emitted
in two foreign languages, plus the generated typed field accessors it needs (step 08, friction 1).
That is a full session and it deserves its own doc, informed by what this step learns about emitting
foreign-language source at all (M0 here decides whether we can even annotate a DTO). The split:

- **Step 12 (this one)** — generator + shell hardening: the D26/D27 slices, the D23 ordering fix, the
  codec deletion, the ergonomics batch, the Compose rule, the upstream filing drafts.
- **Step 13 — per-language contract tests from the C-IDs.** Typed field accessors + the test emitter.
- **Step 14 — C# port + generator** (was 13).

## What the design pass handed over

**D26 — no `Cleaner`.** The backstop is declined (§8 has the three reasons; ownership is the hard
one — the CAS flag and free shim are bindgen output we cannot safely reach around). What ships
instead is *detection at the contract-test tier*: leak-freedom becomes an asserted property on both
platforms, using the count C22 already guarantees, and `close()` in `onCleared()` becomes a tested
rule rather than a convention. The use-after-close UB (step 05, H2) remains upstream's; this step
drafts the filing.

**D27 — versioned stash envelope.** The schema version moves from `StashCodec.kt`'s hand-written
`FORMAT_VERSION` into the **generated** stash DTO, stamped from the declaration; the wholesale-refusal
gate travels with the generated codec (typed, observable — never a silent `null`). Inside a parsed
envelope, per-field degradation stands. D27 carries one **testable claim this step must check, not
assume**: a field whose stashed ancestor no longer parses restores *dirty-from-unset* and therefore
surfaces as a **conflict** on the next rebase against live canonical — the UI already renders that.
If the claim is false, that is kill criterion 3, not a thing to patch.

**A D23 conformance bug, found by step 11's controls.** The generated check driver takes the checker
from its slot *before* looking the draft up, so `run_username_check()` on a released draft with no
checker installed returns `Ok(false)` — indistinguishable from "no checker" — instead of
`DraftClosedFfi` (`gen-profile-ffi/src/generated.rs`, the `let Some(checker) … else return Ok(false)`
ahead of the `draft_mut` lookup). D23's letter says mutating verbs on a released draft refuse,
unconditionally. The fix is ordering: resolve the draft first. This is enforcement of an existing
decision — no new D-row.

## Deliverables

1. **The D23 ordering fix.** `bolted-ffi-gen`'s check driver refuses a closed draft before consulting
   the checker slot. Regenerate, drift check green, and extend both probes' D23 positive controls
   with the no-checker case — each verified to fail against the unfixed generator (regenerate from
   `main`, watch red, re-apply).
2. **Leak-freedom contract tests (D26).** Both platforms assert that tearing down whatever owns a
   draft returns `liveDraftCount` to its baseline: on Android, a ViewModel test driving
   `onCleared()`; on Apple, the deinit/scope-exit equivalent. The Kotlin app's `onCleared()` must
   `close()` — if it already does, the test pins it; if not, that is the first leak the test catches.
3. **The versioned stash envelope (D27).** The generated stash DTO carries a schema version stamped
   from the declaration; decode-side, a version/shape mismatch is a **typed, observable refusal**
   (the shell can tell "no stash" from "stash refused"). `Stashable::from_stash` stays infallible —
   the gate lives at the DTO boundary. Core signatures do not move (kill criterion 2 if they must).
4. **C23 — the degradation claim, as a conformance invariant.** A stash whose ancestor no longer
   parses restores that field dirty-from-unset, and adopting it into a store with live canonical
   surfaces the field as conflicted (C21's machinery, no new mechanism). Add C23 to CONFORMANCE.md
   *with its test* (the doc's drift check will hold you to it), and update the "C01–C22" mentions.
5. **Delete the hand-written codec.** Per M0's outcome: either BoltFFI passes annotations through to
   emitted DTOs (`@Parcelize` / `Codable`), or the codec itself becomes a generated file beside the
   bindings, drift-checked like the rest of D22. The contract is "**zero hand-written per-feature
   codec lines**", not the literal annotation: `StashCodec.kt` is deleted either way, and the D27
   version gate is inside whatever replaces it. (Apple has no codec today — emit its half only if
   the mechanism makes it free; a stash story for iOS process death is not this step.)
6. **Ergonomics batch**, each a small generator change with a shell test:
   - a generated convenience constructor for single-method capability traits (Kotlin cannot
     SAM-convert bindgen's interface; a generated `fun UsernameChecker(block: …)` helper restores
     the lambda; ask for `fun interface` itself in the upstream filing);
   - `Sendable` on Swift classes whose Rust type is `Send + Sync` — a generated
     `extension …: @unchecked Sendable` file in our target, justified per class by the Rust bound;
   - a **name-collision refusal**: the generator rejects, loudly at generation time, a declaration
     whose emitted type name lands on a per-language deny-list (`Date`, `URL`, `Data`, `Error`, …).
     No silent renaming — the declarer picks another name. Golden test: a colliding declaration
     fails with a message naming the language and the collision.
7. **l10n key coverage per target.** The declared key set (`error_variants` + `pending_key` /
   `required_key` / `failed_key` + `required`) becomes machine-readable per feature, emitted by the
   generator from `bolted-decl`. Each shell's coverage test consumes that list, so a key added to the
   declaration fails every shell that lacks a template. The Kotlin drive-the-core test stays (step 11
   proved its design right); **Swift gets the coverage test it has never had** — step-06 friction 7
   happened *on Swift*, and only the Kotlin shell learned from it.
8. **The Compose parameter rule, encoded.** Step 07 fixed the app; the rule ("a Compose shell never
   reads core state by calling a ViewModel method — parameters or `collectAsStateWithLifecycle`
   only") lives in one report. Audit the post-migration app for regressions, write the rule where a
   Compose author will meet it (the app's source docs), and record it as a `bolted-check` rule
   candidate for Phase 4.
9. **Upstream filing drafts**, one file each under `steps/artifacts/step-12-upstream/`: (a) `pack
   android` missing the binding-expansion env (with the workaround from `mise.toml` as the repro —
   deleting that workaround is the acceptance test); (b) generated methods never consult
   `__boltffi_closed` (step 05's H2 transcript is the repro; ask for the flag check, mention
   `fun interface` and an opt-in `Cleaner` as adjacent asks); (c) bindgen silently ignoring
   macro-generated items (step 10's finding). **Drafts are the deliverable; filing them is the
   owner's action**, not the session's.

## Milestones

- **M0 — the passthrough probe (timeboxed, ~30 min).** Determine whether BoltFFI 0.27.3 can put
  `@Parcelize` / `Codable` (or any annotation/conformance) on the DTOs it emits — config, attribute
  passthrough, anything documented. The answer picks deliverable 5's branch; the timebox exists
  because *both* branches are acceptable and the generated-codec branch is fully under our control.
  Record the answer either way — it also scopes step 13's options.
- **M1 — the D23 ordering fix** (deliverable 1). Generator, regen, drift check, both probes'
  extended controls, planted-red verification.
- **M2 — leak-freedom tests (D26)** (deliverable 2), both platforms.
- **M3 — the versioned envelope + C23 (D27)** (deliverables 3–4). The conformance test first — if
  the degradation claim is false, stop here (KC3) before building a gate on top of it.
- **M4 — codec deletion** (deliverable 5), on M0's branch. `StashCodec.kt` is deleted in this
  milestone's commit; the stash/restore and process-death tests must pass unchanged, since the wire
  format only gained a version field it already had (`"v"`).
- **M5 — ergonomics batch + l10n coverage** (deliverables 6–7).
- **M6 — Compose rule + filings + report** (deliverables 8–9); ROADMAP row; the standard sweep.

## Kill criteria (real; if hit, stop and report)

1. **Patching BoltFFI's emitted foreign sources is forbidden** — no post-processing of `dist/`.
   An item that cannot be done without it converts to an upstream filing draft and is recorded.
   If **more than two** items convert, stop the step: the generator seam itself is wrong, and that
   is a design finding, not an implementation obstacle.
2. **D27's gate cannot be built without changing `Stashable` or any core signature** → stop. The
   design pass priced it at zero core changes; a nonzero price goes back to a design session.
3. **The degradation claim tests false** — a no-longer-parsing ancestor does *not* surface as a
   conflict on the next rebase → stop the D27 thread and report. D27's record must be corrected by
   a design pass, not patched around in generated code.
4. **Any generator change that alters the observable semantics of an existing C-ID** (beyond the
   D23 ordering fix, which is *restoring* stated semantics) → stop. Same spirit as step 10's KC4.

## Non-goals (→ elsewhere)

Per-language contract tests from the C-IDs and typed field accessors (→ step 13) · C# (→ step 14) ·
actually *submitting* the upstream filings (owner's action, drafts here) · an iOS stash/process-death
story (no evidence it is needed; Apple has no codec because nothing stashes there) · `bolted-check`
itself, including the constraint-semver snapshot D27 assigns it (Phase 4) · the `Feature` trait
(its own session, before Phase 4) · resolving anything in ARCHITECTURE §9.

## Inherited cautions

The Gradle tiers report BUILD SUCCESSFUL without running tests when up-to-date — `--rerun-tasks`,
counts from JUnit XML, and `test:android` / `test:android:hazard` share one XML file. A forbidding
test can forbid nothing — every "watched red" in this doc means *planted, observed failing, reverted*,
per step 11's controls. The first `test:apple:ui` run after a repoint is flake-prone (step 11,
friction 3); re-run before attributing a red to code. New Gradle modules need their own `.gitignore`
entries (step 11, friction 4).

## Exit checklist

- [ ] `mise run check` green, including the regenerated drift check and C23's conformance test;
      CONFORMANCE.md updated to C01–C23 and its drift check still green.
- [ ] Both probes' D23 no-checker controls green, each watched red against the unfixed generator.
- [ ] Leak-freedom tests green on both platforms; the Kotlin `onCleared()` → `close()` rule pinned.
- [ ] `StashCodec.kt` deleted; stash/restore + process-death tests pass unchanged; the version gate
      is typed and observable, with a test distinguishing "no stash" from "stash refused".
- [ ] Name-collision golden test red on a planted collision; l10n coverage driven from the emitted
      key list on **both** shells, red on a planted missing template.
- [ ] The full sweep, every tier alone, counts from JUnit XML: `check` · `test:apple` ·
      `test:apple:gen` · `test:android` (+ `:hazard`, `:app`, `:gen`) · `test:web` · `test:apple:ui`.
- [ ] Three filing drafts exist under `steps/artifacts/step-12-upstream/`; none submitted.
- [ ] `docs/steps/step-12-report.md` written — including where this doc was wrong; ROADMAP updated;
      §9 untouched.
