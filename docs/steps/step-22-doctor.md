# Step 22 — `doctor`: the environment report for what mise cannot pin

**Phase 4 — Verification harness. Status: ready.**

VISION names the standard verb set — `doctor · check · build · test · pack` — and `doctor` is the
one verb that has never existed. Its charter is VISION risk 5, verbatim: *"mise can't pin
Xcode/NDK/Windows SDK — doctor verifies and warns instead."* And the success criterion that uses
it: *"Clone → `mise install && mise run doctor && mise run build:macos` → running app. No wiki."*

## The defect this step closes

The knowledge of what a machine needs is real and already written down — **scattered across
seventeen task bodies in `mise.toml`** as hand-rolled guards (`command -v xcodebuild`,
`[ -d "$ANDROID_HOME" ]`, the aosp_atd system-image probe, the boltffi version match in
`setup:boltffi`). Each guard is good (fail fast, name the exact remedy), but the *aggregate* is
invisible: there is no way to ask a fresh machine "what can this box run, and what exactly is
missing for the rest?" short of running every verb and collecting its failure. That is the
environment-setup wiki failure mode VISION opens with, minus the wiki.

Worse, the set is unguarded: a new machine-bound task can be added with no guard and no doctor
row, and nothing notices — the requirements knowledge decays silently. The defect is not that any
one guard is wrong; it is that their union is unqueryable and its completeness is unchecked.

## The design (decided in this planning pass; no ARCHITECTURE change)

`doctor` is a **read-only environment report, grouped by verb tier**, implemented as a pure-std
module + bin in `bolted-check`, run as `mise run doctor`.

- **Scope rule: doctor covers exactly what `mise install` cannot guarantee** (VISION risk 5).
  Tools mise pins — rust, trunk, wasm-pack in `[tools]`; the JDK/Gradle/dotnet per-task pins —
  are *not* doctor rows: re-checking them would make doctor a second mise. What remains is the
  un-pinnable set: Xcode (`xcodebuild`, `swift`), `xcodegen`, the Android SDK / NDK / aosp_atd
  system image, Chrome (the headless wasm tier's engine), and the cargo-installed `boltffi` CLI
  at the exact pinned version.
- **Warn, never fail.** `mise run doctor` always exits 0 (VISION's "verifies and warns"); a
  machine that deliberately lacks Android is not broken, it is a machine that runs the other
  tiers. Each MISSING row prints the same remedy string the task guard prints. Rows that cannot
  be checked statically (a GUI session + Accessibility permission for `test:apple:ui`; an
  attached device for `run:android`/`bench:android:device`) are printed as *manual* notes, not
  silently omitted — doctor names what it cannot see.
- **The drift hazard is the design's center.** Doctor restating the guards creates a second copy
  of the requirements — the two-contracts failure D25/D28 exist to kill. Two rung-3 pins close it:
  1. **The coverage manifest test** (in `check`): parse the task names out of `mise.toml` and
     assert every task is either mapped to ≥1 doctor row or carries a recorded exemption *with a
     reason* (mise-pinned; pure-cargo; session-/device-bound; spike — disposal-eligible). Both
     directions: an exemption or mapping naming a task that no longer exists also fails. Adding a
     machine-bound task without deciding its doctor row becomes a build failure, at rung 3.
  2. **The version cross-pin test** (in `check`): doctor's `boltffi 0.27.5` literal must equal
     `setup:boltffi`'s `want="…"` extracted from `mise.toml` — pinned from both sides (the
     step-10 vacuous-needle lesson: the extraction must be shown to find the literal, so the test
     also fails if the `want=` line disappears).
- **Why Rust in `bolted-check`, not a shell script**: it gets unit tests, clippy `-D warnings`,
  and the no-`unwrap` discipline for free, and the coverage/cross-pin tests must live in the
  `check` graph anyway — one crate, one place. Pure std (PATH scan + `std::process::Command` for
  the one version probe + directory probes); **no new dependencies**, and nothing feature-gated
  (unlike `budget`, there is no heavy dep to keep out of the `check` graph).
- **The task guards stay.** They double as env *setup* (ANDROID_HOME defaulting, NDK selection,
  PATH extension) and doctor cannot export into a calling shell. Delegating guard checks to
  `doctor require <task>` is a possible follow-up once doctor's shape survives contact — not
  built now (no consumer, D20's posture).

**`bolted new` is not this step and gains its gate.** Scaffolding designed today would be
designed from **zero external consumers** — there is no publishing story and no product repo to
scaffold; every current shell lives in-tree against path deps. That is the D20 error, thrice
rejected (composites, capability registry, wire emitter). The ROADMAP row now records the gate:
`bolted new` becomes current with the first out-of-tree framework consumer / a publishing story.

## What the planning pass verified (by reading the code, 2026-07-17)

- **The guard inventory is complete and enumerable.** `mise.toml` (578 lines) holds every task;
  the machine-bound guards check: `boltffi` (+ version in `setup:boltffi`), `xcodebuild`,
  `swift`, `xcodegen`, `ANDROID_HOME` dir, `$ANDROID_HOME/ndk/*`, the
  `system-images/android-34/aosp_atd` dir, Chrome/Chromium, `dotnet` (mise-pinned per-task),
  `trunk`/`wasm-pack` (mise-pinned in `[tools]`), rustup targets (self-healing inside the tasks),
  adb device state (dynamic), Developer ID identity + GUI session (spike verbs / `test:apple:ui`).
- **`bolted-check` is the right host**: §5 gives it "build-time analyses"; it already ships two
  bins (`wasm-budget` feature-gated; the snapshot renderer as `-ffi` examples), is in the `check`
  graph as a dev-dependency, and depends only on `bolted-decl`/`bolted-core`. Doctor needs
  neither — it is std-only and touches no declaration. No boltffi dep enters (step-16 KC1's seam
  discipline holds).
- **Task-name extraction is line-shaped**: every task header in `mise.toml` is a line beginning
  `[tasks.` (quoted or bare name) — a std string scan suffices; no TOML parser dependency needed.
- **Naming does not collide**: `mise doctor` (the mise CLI's own health check) and
  `mise run doctor` (ours) are namespaced apart; VISION's verb list means the latter.

## Deliverables

1. **`crates/bolted-check/src/doctor.rs`**: check primitives (executable-on-PATH scan, directory
   probe, version probe honoring `CARGO_HOME`/`~/.cargo/bin` like the tasks do), the requirement
   table (row → tier → tasks it serves → remedy), the exemption manifest (task → reason), and a
   pure report renderer (checks in, text out — testable without an environment).
2. **`crates/bolted-check/src/bin/doctor.rs`**: thin main; always exits 0.
3. **The coverage manifest test** and **the version cross-pin test**, both running inside
   `mise run check`, both pinned from both sides (non-vacuous extraction asserted).
4. **`[tasks.doctor]`** in `mise.toml`.
5. **Evidence**: doctor's real output on this machine in the report.
6. **Falsification**: each new red watched — a dropped coverage mapping, a phantom task, a wrong
   version literal, and at least one runtime MISSING row (broken `ANDROID_HOME` / stripped PATH)
   showing its remedy. Restored green each time.
7. **Docs**: report + ROADMAP (22 done; 23+ row carries the gated items, `bolted new` now with
   its gate). **No ARCHITECTURE change** — doctor never touches §1–§7; recorded here, not in §8.

## Milestones

- **M0 — planning artifacts** (this pass): step doc, ROADMAP row split. Commit.
- **M1 — the doctor module + bin**: primitives + table + renderer + unit tests; `mise run check`
  green. Commit.
- **M2 — the two rung-3 pins**: coverage manifest + version cross-pin, inside `check`. Commit.
- **M3 — the verb + evidence**: `[tasks.doctor]`, real run captured. Commit.
- **M4 — falsification**: watched reds per deliverable 6; restore green. Commit.
- **M5 — report + ROADMAP.** Commit; PR.

## Kill criteria (real; if hit, stop and report)

1. **A requirement cannot be checked without duplicating task *logic*** (not a literal — logic,
   e.g. re-deriving NDK selection in a way that can silently disagree with the task). A literal
   duplicated under a cross-pin test is fine; un-pinnable duplicated judgement is the drift
   hazard itself. Stop, design session.
2. **The line-scan cannot enumerate `mise.toml` tasks reliably** (misses one, or cannot be shown
   non-vacuous). Fallback to price first: a `toml` dev-dependency in `bolted-check`. If that
   drags meaningful transitive weight into the `check` graph, stop and report.
3. **Doctor grows a judgement about the contract** (anything a shell/core invariant owns).
   Doctor reports environment; the moment it wants to know about drafts, fields, or
   declarations, the design is wrong. Stop.

## Non-goals (→ elsewhere)

- **`bolted new`** — gated on the first out-of-tree consumer / publishing story (ROADMAP 23+).
- **Rewiring existing task guards** to call doctor — follow-up candidate, not now (D20 posture).
- **The spike verbs** (`test:os:*`, `run:os:app`) — `spikes/os-integration/` is
  disposal-eligible; they get exemption rows ("spike — disposal-eligible"), not doctor rows.
- **Dynamic session state** (attached devices, GUI session, Accessibility, Developer ID
  identity) — named as manual notes in the output, never probed.
- **Windows** — no Windows hardware (step-07 KC4 precedent); the C# tier's dotnet is mise-pinned
  and exempt like the other pinned tools.
- **Anything upstream.**

## Inherited cautions

- Read check verdicts from an explicit exit-code echo (`>log 2>&1; echo "check exit=$?"`), never
  through a pipe; never chain a commit after a check with `;`.
- A forbidding/coverage test can cover nothing (step 10): show the mise.toml scan finds a known
  task and a known count floor before trusting it; watch every new test red once (M4).
- No `unwrap`/`expect`/`panic!` in library code; clippy `-D warnings`; edition 2024.
- Build/test only via `mise run check` / `mise run test`; never `git -C`; commit per milestone.

## Exit checklist

- [ ] `mise run doctor` prints the per-tier report on this machine, exits 0, and names the
      manual rows it cannot check.
- [ ] Every `mise.toml` task is mapped or exempted-with-reason; the manifest test fails on a
      phantom task, a dropped mapping, and a stale exemption — each watched red.
- [ ] The boltffi version literal is cross-pinned both ways; watched red.
- [ ] At least one runtime MISSING row watched with its remedy string.
- [ ] `mise run check` green; report + ROADMAP updated; no ARCHITECTURE edit.
