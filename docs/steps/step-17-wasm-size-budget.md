# Step 17 — the web shell onto `gen-profile` + the wasm size budget

**Phase 4 — Verification harness. Status: ready.**

Two debts, one step, one new tier. First: `profile-web` is the **last shell still consuming the
hand-written `spike-profile`** — step 11's charter was the FFI shells, and the zero-FFI shell never
moved, so §1's "Rust shells consume the contract directly — no codegen" has never once been proven
against a *generated* feature crate. Second: the **wasm size budget** the Phase-4 sketch names, against
step-04's baseline (311 610 B raw wasm / 87 437 B brotli; 92 790 B brotli total wire). The two belong
together because the budget must pick its subject: a budget guarding the frozen spike crate is
structurally blind to the one place web-target size risk actually lives — **macro output** (step 09
already caught emitted code doing per-keystroke work no test saw; size is another axis tests are blind
on). Migrate first, then budget the framework path. The migration also produces a one-time number no
other step can: the **spike-vs-gen wasm delta** — the first measurement of what macro output *weighs*.

Why a new tier: `mise run check` is the works-everywhere host verb (no wasm target, no trunk, no
network), so the budget cannot live at rung 3 inside it — step 04 made that call explicitly and
step 16's memory re-recorded it. The budget gets its own verb, `check:web`, beside the existing
`build:web` / `test:web` / `serve:web` family.

## What the planning pass verified (by reading the code, 2026-07-15)

- **The migration is two files and ~7 call sites.** `crates/profile-web/Cargo.toml:12` links
  `spike-profile`; only `controller.rs:16` and `app.rs:20` import it. `gen-profile` emits the same
  names (`Profile`, `ProfileField`, `ProfileDraft`, `ProfileStore`, `ProfileCheck`, plus re-exported
  `value_types`: `Date`, `DateRange`, `Email`, `PersonName`, `Username`). The step-09 report's two
  documented surface deltas are the whole diff, confirmed against today's sources:
  1. `try_set_availability(start, end)` → `try_set_availability((start, end))` — one call site,
     `controller.rs:358`.
  2. The three spike check conveniences (`begin_username_check` / `complete_username_check` /
     `username_check_state`) do not exist on `gen-profile`; the surface is
     `bolted_core::Checked` (`crates/bolted-core/src/draft.rs:140`) keyed by `ProfileCheck` —
     `begin_check(id)`, `complete_check(id, token, verdict)`, `check_state(id)`. Call sites:
     `controller.rs:229`, `:253–261`, `:380`, `:387–388`. Add `use bolted_core::Checked` and
     `gen_profile::ProfileCheck`.
  Every error key is reproduced exactly by `gen-profile` (the `key = "…"` overrides exist for
  precisely this), so `l10n.rs` and its tests need no change. `ProfileField::constraints()` is
  macro-emitted (step 16 leans on it), so `app.rs`'s constraint-derived counters keep working.
- **The verification net for the migration already exists**: 29 host controller tests (inside
  `check`) + 8 headless-Chrome wasm tests (`test:web`) + 2 l10n unit tests, all written against the
  behaviors, not the crate. They must pass **unmodified** — an edit forced by the migration is a
  finding, not a chore (both crates pass the same conformance suite; a shell-visible divergence means
  the suite is blind, which is report material).
- **`spike-profile` stays.** It is the golden reference `gen-profile` is read against (step-09 report)
  and `spike-profile-ffi` + the conformance suite still consume it. Only `profile-web` repoints.
- **Baseline facts.** Step-04 measured: `profile-web_bg.wasm` 311 610 B raw / 87 437 B brotli-q11;
  glue JS 30 326 B raw / 5 353 B brotli; total wire 341 936 B raw / 92 790 B brotli. Leptos CSR
  hello-world floor: 102 438 B raw. Toolchain: trunk pinned `github:trunk-rs/trunk` in `mise.toml`
  (the registry-shortname trap is already dodged), `leptos = "0.8"` floats at patch level via the
  lockfile — the report must record the *locked* leptos/wasm-bindgen versions next to the new numbers,
  or the drift-since-step-04 comparison is uninterpretable.
- **Where the checker lives.** Step 16's lesson (recorded in memory): ask first whether an analysis is
  a pure source function or needs runtime facts. This one is neither — it reads **built-artifact
  bytes**. That fits a real `bolted-check` **bin**, but its deps (brotli compression) must not leak
  into the host `check` graph: `bolted-check` is a dev-dependency of the `-ffi` crates (the
  constraint-snapshot examples), so a plain dependency would be built by every
  `cargo test --workspace`. A cargo **feature** (`budget`) with `[[bin]] required-features` keeps the
  compression dep out of every build that doesn't ask for it.
- **`dist/` is gitignored and hash-named.** Trunk emits `profile-web-<hash>_bg.wasm`; a stale dist
  can hold leftovers. The checker must glob and **refuse more than one** `*_bg.wasm` match rather
  than silently picking one.

## What earlier steps hand over (use it, don't re-derive it)

- **The tier discipline (step 04, D-recorded in the ROADMAP):** `check` stays host-only; browser-shaped
  verification gets its own doctor-guarded verb. Copy `build:web`'s guard-and-self-heal shape.
- **The measurement method (step 04):** raw + brotli-q11, wasm and glue separately, totals for wire.
  The *differential* hello-world analysis was a one-time attribution exercise — do **not** rebuild it.
- **The falsification doctrine (steps 10/13/16):** every new check watched red before it is trusted;
  a forbidding test can forbid nothing.
- **The browser rule (memory `bolted-verify-in-a-real-browser`):** a green suite is not evidence about
  a UI — after the migration, drive the running app by hand via `serve:web`.

## Scope: one migration, one bin, one committed budget, one verb

Migrate `profile-web` from `spike-profile` to `gen-profile` (nothing else repoints). Add a
`wasm-budget` bin to `bolted-check` behind a `budget` feature: `--print <dist>` measures (raw +
brotli-q11 for the wasm and the JS glue, plus totals), `check <dist> <budget-file>` enforces a
committed budget. Commit `crates/profile-web/wasm-budget.txt` (hand-parsed `key = value` lines +
comment header — no TOML dep for three numbers): `wasm_raw_max_bytes`, `wire_brotli_max_bytes`, and a
sanity floor `wasm_raw_min_bytes` so an empty or missing artifact can never pass as "under budget".
Add `check:web` = release build + budget assertion. **Budget policy (record it in the file header and
the report):** maxima = measured post-migration baseline × 1.10, rounded up to a whole KiB; the
failure message prints measured vs budget and names the duty — *review what grew; if intended, raise
`wasm-budget.txt` in the same change, deliberately*. Re-baselining is a human edit of a committed
file, never automatic (the D27 constant-not-derived precedent). No change to `check`.

## Deliverables

1. **`profile-web` on `gen-profile`** — dependency line, imports, the availability tuple site, the
   `Checked`/`ProfileCheck` rewrite of the check-drive sites. All 29 + 8 + 2 existing tests green
   **unmodified**; manual browser pass done.
2. **`wasm-budget` bin** in `crates/bolted-check` behind a `budget` cargo feature
   (`[[bin]] required-features`), compression via the pure-Rust `brotli` crate at quality 11
   (matching step-04's `brotli -q11`). Parse/compare/format logic compiled unconditionally with unit
   tests that run inside plain `cargo test --workspace`; only the fs-walk + compression sit behind
   the feature. Refuses an ambiguous dist (≠ 1 `*_bg.wasm`).
3. **`crates/profile-web/wasm-budget.txt`** — committed, header comment carrying the policy and the
   baseline it was set from (numbers + locked leptos/wasm-bindgen versions + date).
4. **`check:web` mise verb** — doctor-guarded like `build:web` (trunk present, wasm32 self-heal),
   then `trunk build --release`, then the budget check. `check` untouched.
5. **Two one-time numbers in the report**: (a) baseline drift since step 04 on the *pre-migration*
   app (12 steps of core evolution + toolchain float, same crate); (b) the **spike-vs-gen delta**
   (same shell, same toolchain, same session — the only clean measurement of macro-output weight the
   project will ever get).
6. **Falsification** — every new check watched red (M4).
7. **Report + ROADMAP** (`step-17-report.md`).

## Milestones

- **M0 — the bin.** `wasm-budget` in `bolted-check` (feature-gated as above); unit tests for budget
  parsing, comparison, ambiguous-dist refusal, and the failure-message format, running inside `check`
  with **no new deps in the default graph** (verify: `cargo tree -p bolted-check` shows no `brotli`
  without `--features budget`). Commit.
- **M1 — the continuity measurement.** `mise run build:web` on the untouched shell; `wasm-budget
  --print` it. Record raw/brotli beside step-04's table with the locked toolchain versions. No file
  changes — the numbers land in the report at M5. No commit.
- **M2 — the migration.** Repoint to `gen-profile`; rewrite the ~7 sites; all existing tests green
  unmodified (`check` + `test:web`); then the real browser via `serve:web` — echo rule under fast
  typing with leading spaces, a conflict + keep-mine/take-theirs, orphan banner, and `admin`/`taken`
  driving the uniqueness check through Pending → failed verdict. Commit.
- **M3 — the budget.** Measure the migrated app (this is delta (b) against M1); set and commit
  `wasm-budget.txt` per the policy; add `check:web`; run it green. Commit.
- **M4 — falsification.** Lower `wasm_raw_max_bytes` below measured → watched red, message shows the
  right measured/budget numbers; point the checker at a missing dist → red, not green; plant a second
  `*_bg.wasm` in dist → red (then delete it); raise the sanity floor above measured → red; restore →
  green. Confirm `mise run check` is byte-for-byte indifferent to all of it. Commit.
- **M5 — report + ROADMAP.** Both one-time numbers, the locked versions, deviations, friction.
  Commit.

## Kill criteria (real; if hit, stop and report)

1. **The migration needs a structural change** — anything beyond the two documented deltas that
   requires touching `bolted-core`, `bolted-macros`, or `gen-profile`'s declaration to serve the
   shell. That is §1's "Rust shells consume the contract directly" failing for generated code — a
   design finding for a design session, not something to patch here. Stop.
2. **A behavioral spike↔gen divergence surfaces** — an existing controller/wasm test fails after the
   migration for a reason that is not the two documented deltas. Both crates pass the same
   conformance suite, so a shell-visible divergence means the suite is blind (the step-08 `is_based`
   precedent). Stop; the conformance gap outranks this step.
3. **The budget proves unlivable** — identical rebuilds of identical source flap across the 10 %
   headroom (wasm-opt nondeterminism, hash-dependent sizes). A tripwire that fires on noise trains
   people to re-baseline blind, which is worse than no tripwire (step-16 kill 4's logic). Measure the
   flap, then stop — the fix (different metric, different pin) is a design choice.

## Non-goals (→ elsewhere)

- **Auto-ratcheting or auto-bumping the budget** — the policy keeps re-baselining a deliberate,
  reviewed edit (the D27 precedent: derived version-bumps were D27's own rejected alternative).
- **Rebuilding the hello-world floor differential** — step 04's attribution stands; this step ships a
  tripwire, not an attribution study.
- **Per-feature size deltas, size CI dashboards, gzip/CDN simulation** — one compressed metric
  (brotli-q11) is the wire truth the budget guards.
- **Budgeting `spike-profile` or deleting it** — it stays the golden reference; only `profile-web`
  repoints.
- **Emitting the web shell / a Leptos generator** — Phase 3+ sizing input is banked in the step-04
  report (friction 2); this step doesn't touch it.
- **Folding anything into `mise run check`** — the tier boundary is the point.
- **`doctor`, `bolted new`, capability coverage, the C# resume** — later steps; the C# resume stays
  tripwire-gated.

## Inherited cautions

- **A green suite is not evidence about a UI** — M2's manual browser pass is not optional
  (memory: `bolted-verify-in-a-real-browser`).
- **A forbidding test can forbid nothing** (step 10) — M4 watches every red, including
  missing-dist ≠ green.
- **Leptos flushes DOM writes a microtask late** (step-04 friction 4) — if any wasm test does need
  touching, yield first; but a needed touch is itself a finding (kill 2).
- **`leptos = "0.8"` floats via the lockfile** — record locked versions with every measurement, or
  the comparisons are noise.
- **Stale dist** — never measure without a fresh `trunk build --release`; the ≠ 1 wasm guard is the
  backstop, not the procedure.
- The package-name trap: `gen_profile` (crate ident) vs `gen-profile` (dir) (step-13 friction 2).
- Commit per milestone; never `git -C`; build/test only via `mise run …` verbs.

## Exit checklist

- [ ] `profile-web` links `gen-profile`; `grep -rn "spike_profile" crates/profile-web/src` returns
      nothing; 29 host + 8 wasm + 2 l10n tests green **unmodified**; manual browser protocol run.
- [ ] `wasm-budget` bin feature-gated; default `cargo tree -p bolted-check` free of compression deps;
      parse/compare logic unit-tested inside plain `check`.
- [ ] `wasm-budget.txt` committed with policy header, baseline numbers, locked toolchain versions.
- [ ] `check:web` green; `check` untouched (no new verb inside it, no new deps in its graph).
- [ ] Every new check watched **red**: over-budget, under-floor, missing dist, ambiguous dist —
      each with the right message — then restored green.
- [ ] Report carries both one-time numbers (drift since step 04; spike-vs-gen delta) with locked
      versions; ROADMAP row updated; **ARCHITECTURE untouched** — no §9 question is resolved here.
