# Step 07 — Kotlin/Compose spike app

**Phase 2 — Freeze. Status: ready.**

The frozen contract on a real Android app. Step 05 proved BoltFFI's four features work on ART from a
headless probe with no UI and no lifecycle; this step puts a Compose form in front of them and finds
out what an actual Android app does to a design that assumes the core owns the draft.

Three risks only a real app exercises:

1. **Process death mid-draft.** The core-side draft dies with the process. ARCHITECTURE §9 hands
   **stash/restore** to this step. It is the one genuinely undesigned mechanism left in Phase 2.
2. **Configuration change.** Rotation destroys the Activity but not the `ViewModel`. Since step 05,
   `close()` is *mandatory* on ART (the GC never frees a draft) — so the draft handle's scope and its
   release point are now a correctness question, not an ergonomics one.
3. **Main-thread snapshot delivery.** The generated `snapshots(): Flow<ProfileSnapshot>` is a
   `callbackFlow`. A form that repaints on every keystroke must not hop threads to do it.

It also doubles as the hand-written *"as-if-generated"* Kotlin reference — the golden output step 10's
generator is diffed against — and it **re-measures the per-keystroke round-trip on physical
hardware**. Step 05's 12–13 µs was measured on an arm64 emulator on an arm64 host: the right VM on the
wrong CPU, and therefore a lower bound.

## Process note

Per `CLAUDE.md` this should be a Fable planning session (write the doc) followed by an Opus
implementation session (execute it). The project owner asked for both in one sitting, as in step 06.
The doc is therefore written first and **committed before any code**, and the three decisions below
that touch frozen artefacts were put to the owner before a line was written. Everything else follows
`CLAUDE.md`'s rule: smallest reversible choice, recorded in the report.

---

## The defect this step found before it started

Planning this step meant asking what `rebase` does to a dirty field during restore. The answer is a
verified bug in a frozen core.

**`Field::rebase` never compares `theirs` against `base`.** A dirty field therefore enters
`Conflicted` whenever *any other* field's canonical value moves:

```
base = "Alice Anderson",  I type name := "My Name",  the server changes only `email`
  → name.rebase("Alice Anderson")
  → dirty, value != theirs  →  Conflicted { theirs: "Alice Anderson" }   // theirs IS my ancestor
```

The shell then renders a "keep mine / take theirs" banner whose two buttons do visibly the same
thing, and `commit` is refused with `Conflicted`. That is precisely the state **C14** was written to
abolish, arriving through a different door.

It hid because **C03's property test never generated it**:

```rust
prop_assume!(mine != base);
prop_assume!(theirs != mine);   // ...but never `theirs != base`
```

Two independently-drawn 3–20 character strings are essentially never equal, so the missing
precondition was never sampled. Worse: `c08_rebase_reruns_tier2_rule`, a test *inside the frozen
conformance suite*, already produces a spurious conflict on `email` (`theirs == base ==
"alice@corp.example"`) and passes anyway, because it only asserts on the rule.

This sits on this step's critical path. Restore = re-checkout, re-install the stashed fields, rebase
onto freshly-fetched canonical. Every dirty field would conflict against a canonical that never moved.

### The fix (owner-approved)

`Field::rebase` gains a four-line early-out. Nobody else changed this field, so keep my value, clear
any conflict, stay `InSync`:

```rust
pub fn rebase(&mut self, theirs: V) {
    if self.base.as_ref() == Some(&theirs) {
        self.sync = SyncState::InSync;
        return;
    }
    // ... existing conflicted / clean / convergent / conflict arms
}
```

It handles both halves of the real three-way rule:

| `theirs` vs `base` | `mine` vs `base` | Result |
|---|---|---|
| unmoved | dirty | keep mine, `InSync`, still dirty — *the bug* |
| moved **back** to the ancestor, while conflicted | dirty | conflict **clears**, keep mine — also the bug |
| moved | clean | adopt (C02) |
| moved, `mine == theirs` | dirty | converge, clean (C04) |
| moved, `mine != theirs` | dirty | conflict (C03) |

It also makes `rebase` **idempotent**, and makes `checkout()` exactly `adopt(from_canonical(..))` —
which M2 depends on.

**Frozen-artefact amendments:** C03's statement gains its missing precondition; its proptest gains
`prop_assume!(theirs != base)`; a new **C19** pins both halves of the early-out; ARCHITECTURE §5's
rebase table is corrected.

---

## Decisions taken before implementation

All three were put to the project owner, because each touches something frozen.

| # | Decision | Rejected alternative |
|---|---|---|
| **D14** | Fix the spurious conflict in `Field::rebase`; amend C03; add C19. | Guard in the feature's `Draft::rebase` (every generator must then emit it, and it mishandles the moved-back-to-ancestor case); or stop and defer (blocks the step outright). |
| **D15** | Stash/restore lives in the **core**: `Store::adopt(draft)` + feature-level `stash()`/`from_stash()`. The stash is `{base_version, per-field (raw, base)}`. `sync` is **not** stashed — it re-derives. | FFI-only `ProfileStoreFfi::restore` (no C-ID, no build-time check, the one new mechanism outside the verification ladder); or shell-side replay with no ancestor (a field the server moved during process death returns *dirty*, not *conflicted*, and submit silently overwrites the server — C03's spirit violated to save a struct). |
| **D16** | Build the hardware benchmark; run it if a device is attached, ship it flagged-unrun otherwise. Never invent a number. | Defer the physical re-measurement to step 10 (leaves the chattiness kill criterion resting on a lower bound for two more steps). |

### Why `sync` is not stashed

A conflict is a *relationship* between my value and a canonical value. Process death invalidates the
canonical half: `theirs` from before the death is a value the server may no longer hold. Stashing it
would restore a lie.

Stash `{raw, base}` and the relationship re-derives, correctly and against **fresh** canonical:

```rust
Field::from_base(stashed_base)   // the ancestor, exactly as it was
    .try_set(stashed_raw)        // my value; Invalid { raw } survives verbatim (C06)
// then, inside adopt:
    .rebase(current_canonical)   // conflict / converge / adopt, decided now
```

Every prior resolution survives for the right reason. A field the user `resolve_keep_mine`'d has
`base == old theirs`; if canonical still holds that value, D14's early-out leaves it dirty and
`InSync` — the resolution stands. If canonical moved *again*, a fresh conflict is exactly right.

**The async verdict deliberately does not survive.** It endorses a value against a server state that
may have moved while the process was dead. It restores as `Idle`, and **C16** then refuses to submit a
dirty username without a fresh check. No new invariant is needed: C13 + C16 already make restore safe.

### `Store::adopt`

One new verb on the prototype store, general enough not to name "stash" in the contract:

```rust
/// Register an externally-built draft and rebase it onto current canonical.
pub fn adopt(&mut self, draft: D) -> DraftHandle<D>;

// and checkout collapses into it:
pub fn checkout(&mut self) -> DraftHandle<D> {
    self.adopt(D::from_canonical(self.canonical.as_ref(), self.version))
}
```

`adopt` needs one bit `checkout` used to read off the store: *was this draft based on an existing
entity?* Create-flow drafts never rebase and never orphan (C12). So `StoreDraft` — the plumbing trait
no shell ever calls — gains `fn is_based(&self) -> bool`, **derived** from the fields' bases and never
stored, because two copies of one fact are two facts to keep consistent (D3/F7).

| `is_based()` | store canonical | `adopt` does |
|---|---|---|
| true | `Some` | rebase onto it, register for live rebase |
| true | `None` | **orphan** (C11 survives process death: the entity was deleted while we were dead) |
| false | either | nothing; do not register (C12) |

---

## Deliverables

### 1. The rebase fix + stash/restore (Rust)

- `bolted-core`: `Field::rebase` early-out; `FieldStash<R>`; `Field::stash()` / `Field::from_stash()`;
  `StoreDraft::is_based()`; `Store::adopt()`; `checkout()` reimplemented on top of it.
- `spike-profile`: `ProfileStash`, `ProfileDraft::stash()` / `from_stash()` — written as plainly as
  `#[bolted::entity]` will emit them.
- `spike-profile-ffi`: `ProfileStashFfi` + per-raw-type `…FieldStashFfi` DTOs;
  `ProfileDraftFfi::stash()`; `ProfileStoreFfi::restore(stash)`.

### 2. `docs/CONFORMANCE.md` — C03 restated, C19–C21 added

| ID | Statement |
|----|-----------|
| C03 *(amended)* | A dirty field **whose canonical value moved** and differs from `theirs` must preserve your value, enter `Conflicted { theirs }`, and leave the ancestor where it was. |
| **C19** | **Rebase is a three-way merge.** A field whose canonical value equals its recorded ancestor must not be conflicted by a rebase, whatever its dirty state; and a canonical that moves back to the ancestor must clear an existing conflict. Rebase is idempotent. |
| **C20** | **Stash round-trips.** `from_stash(d.stash())` reproduces every field's value, ancestor, validity (including `Invalid { raw }`) and dirtiness. `sync` is not stashed and re-derives. An async verdict does not survive, so C16 demands a fresh check on a dirty checked field. |
| **C21** | **Restore is a rebase.** Adopting a restored draft conflicts exactly those fields whose canonical moved while it was away; adopting an entity-backed draft into a store with no canonical orphans it (C11); a create-flow draft is never moved (C12). |

The drift test (`conformance_manifest_has_a_test_for_every_id`) enforces the mapping, as it did in
step 06. C19–C21 get `c19_*`, `c20_*`, `c21_*` tests.

### 3. `android/profile-app` — the Compose spike app

Mirrors `apple/profile-app` file for file, so step 10 can diff two hand-written references:

| File | Role |
|---|---|
| `ProfileViewModel.kt` | the as-if-generated VM: `StateFlow`, the echo rule, debounce, submit, stash |
| `Localization.kt` | the l10n key table — **with every core error key covered** (step-06 friction 7) |
| `ProfileForm.kt` | Compose UI. No constraint literal: `maxLength` comes from `store.constraints()` |
| `StashCodec.kt` | `ProfileStashFfi` ⇄ JSON string, for `SavedStateHandle` |
| `MainActivity.kt` | hosts the form |

The VM owns the draft, closes it in `onCleared()`, and writes its stash into `SavedStateHandle` — the
only container that survives process death.

### 4. Tests — three tiers, all headless

| Tier | Verb | What it proves |
|---|---|---|
| VM (instrumented, ART) | `test:android:app` | the contract, mirroring `ProfileViewModelTests.swift` |
| **Compose UI** | `test:android:app` | real events into a real render tree — **on a headless GMD**, unlike step 03's XCUITest |
| Lifecycle | `test:android:app` | rotation keeps the draft; `onCleared` closes it (C18); process death restores it |

Process death is simulated faithfully rather than approximated: the VM's `SavedStateHandle` is saved
via `savedStateProvider().saveState()`, the resulting `Bundle` is **written to and read back from a
real `Parcel`**, and a fresh VM + fresh `Store` is built from the restored handle. The old VM and
store are dropped first. That exercises the same bytes and the same container the OS uses. What it
does *not* prove is that Android chose to kill us — no headless test can.

### 5. `mise run bench:android:device` — the hardware re-measurement

A `PhysicalChattinessProbe` that **refuses to run on an emulator** (`Build.FINGERPRINT` /
`ro.kernel.qemu`), so the number can never be quietly emulator-sourced again. Same shape as step 05's
`ChattinessProbe`, same 1.0 ms bar.

### 6. New mise verbs

- `mise run test:android:app` — Compose + VM + lifecycle tests on the headless `dev34` GMD.
- `mise run bench:android:device` — the hardware benchmark, physical device required.
- `mise run run:android` — install and launch the app (the manual protocol).

`mise run check` stays Rust-only and JDK-free, as in step 05.

---

## Kill criteria — real; if hit, stop and report

1. **Compose UI tests cannot run on the headless GMD.** Then Android has no headless UI tier either,
   step 04's "the wasm tier is the contrast worth banking" claim loses half its force, and the
   verification ladder's rung 3 has a platform-shaped hole. *Bar: `ProfileFormTest` runs green on
   `dev34` with no GUI session.* Answered early, in M4.
2. **A `ViewModel`-scoped draft does not survive a configuration change**, or `onCleared()` cannot
   release it. Then the draft lifetime model does not fit Android's, and that is a design question,
   not a bug.
3. **`from_stash` + `adopt` cannot reproduce the pre-death draft.** *Bar: C20 and C21 pass.* If the
   ancestor, an `Invalid { raw }`, or a resolution cannot survive the round trip, D15's stash shape is
   wrong — stop, design session.
4. **The chattiness bar breaks on physical hardware.** *Bar: median `try_set` + `snapshot` ≤ 1.0 ms.*
   This is step 05's kill criterion finally measured on the right CPU. A break means the
   core-validates-every-keystroke contract needs a shell-side write buffer, which is a design change.
   (Only assessable if a device is attached — D16.)
5. **`Store::adopt` cannot express `checkout`.** If the two need materially different registration or
   rebase logic, `adopt` is the wrong primitive and the stash needs its own store verb — which would
   mean the contract grows two draft entry points, not one. Stop.

---

## Milestones

| # | Content | Commit |
|---|---|---|
| **M1** | The rebase fix. C03 restated, C19 added, `prop_assume!(theirs != base)`, regression tests at *every* tier that should have caught it (core unit, conformance, web, Swift VM). ARCHITECTURE §5 corrected. | alone — it is a contract change and must be reviewable on its own |
| **M2** | `FieldStash`, `Field::stash/from_stash`, `StoreDraft::is_based`, `Store::adopt`, `checkout` on top of it; `ProfileStash`, `ProfileDraft::stash/from_stash`; C20 + C21. | with M3 if the type ripple is one change |
| **M3** | The stash across BoltFFI: DTOs, `ProfileDraftFfi::stash()`, `ProfileStoreFfi::restore()`. An Apple probe test, because it is nearly free and proves the DTO crosses. | |
| **M4** | `android/profile-app` skeleton: Gradle module, Compose, `MainActivity`, `test:android:app`, one walking-skeleton Compose UI test. **Kill criterion 1 is answered here, deliberately early.** | |
| **M5** | `ProfileViewModel.kt` + `Localization.kt`: `StateFlow`, main-thread delivery, the echo rule (`focusedTouched` — never `dirty`), debounce, `username_check_required` rendered as *progress*, stash into `SavedStateHandle`. | |
| **M6** | `ProfileForm.kt` + the Compose UI tests + the lifecycle tests (rotation, `onCleared`→`close`, process-death `Parcel` round trip). | |
| **M7** | `bench:android:device` + `PhysicalChattinessProbe`. Run it if a device appears; otherwise ship it flagged. | |
| **M8** | `docs/steps/step-07-report.md`, ROADMAP, ARCHITECTURE §9 (stash/restore closes), memory. | |

---

## Non-goals

- A real backend. The uniqueness checker and the "server" stay simulated, as in every prior shell.
- Navigation, DI frameworks, Material theming beyond legibility, tablet layouts.
- Any §9 question this step does not own. **Store concurrency** and **weak draft registry** are step
  08's; **use-after-close must become a typed error** and the **`pack android` upstream bug** are step
  10's. `close()` being mandatory is the *premise* of this step, not its subject.
- Generalising the conformance suite over a feature (step 08).
- Serialization as a framework concern. `StashCodec` is hand-rolled JSON in the app, and what it costs
  is a *finding* for step 10, not a crate.

---

## Exit checklist

- [ ] `mise run check` green; `bolted-core` still zero-dependency and `#![forbid(unsafe_code)]`.
- [ ] `mise run test:web`, `mise run test:apple`, `mise run test:android`, `mise run test:android:app`
      green. `test:android:hazard` still 3/3.
- [ ] C19, C20, C21 exist in `docs/CONFORMANCE.md`, each with a test; the drift test passes; C03's
      amended statement matches its amended proptest.
- [ ] The spurious conflict has a regression test **at every tier that should have caught it** — not
      only where it was found.
- [ ] Kill criterion 1 answered with a green Compose UI test on a headless device, or reported.
- [ ] Kill criterion 4 answered with a number from physical silicon, or **explicitly** reported as
      unrun. No invented figures, no emulator number relabelled.
- [ ] No `unwrap`/`expect`/`panic!` in library code; no constraint literal in `ProfileForm.kt`.
- [ ] Every core error key has a `Localization.kt` template (the step-06 friction-7 trap).
- [ ] `docs/steps/step-07-report.md` written; ROADMAP updated; ARCHITECTURE §9 loses stash/restore.
