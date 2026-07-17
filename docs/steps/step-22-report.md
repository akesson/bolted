# Step 22 — report: `doctor`, the last missing verb

**Status: done. No kill criteria hit.** [Plan](step-22-doctor.md) · no ARCHITECTURE change
(doctor is harness tooling, not contract — recorded in the plan, held to).

## What was built

`mise run doctor`: a read-only, per-tier report of exactly what `mise install` cannot pin
(VISION risk 5), warning and never failing (always exit 0), with the requirements knowledge
guarded against drift by two rung-3 pins inside `mise run check`:

1. **`crates/bolted-check/src/doctor.rs`** — the requirement table (8 rows: boltffi CLI @
   pinned version, xcodebuild, swift, xcodegen, Android SDK/NDK/system image, Chrome), the
   exemption manifest (15 tasks, each with its reason: mise-pinned, pure-cargo, spike —
   disposal-eligible, the remedy verb itself), and the manual-notes list (GUI
   session/Accessibility, attached devices) — printed, never silently omitted. Evaluation is a
   pure function of a `Machine` struct, so every judgement is unit-tested against synthetic
   environments (planted executables on a synthetic PATH, planted SDK trees); the one
   subprocess (the boltffi version probe) runs in the thin bin and arrives as data. Pure std —
   no new dependencies, nothing feature-gated.
2. **`tests/doctor_manifest.rs`** — the two pins, both non-vacuous by construction:
   - **Coverage manifest, both directions**: every `mise.toml` task maps to ≥1 doctor row or an
     exemption reason, and every mapped/exempted name still exists. The task scan is held from
     the other side (must find `test:android`, count floor ≥25). Adding a machine-bound verb
     without deciding its doctor row now fails the build.
   - **Version cross-pin**: `BOLTFFI_PINNED` must equal `setup:boltffi`'s `want="…"`, and the
     extraction must find exactly one `want=` line.
3. **`[tasks.doctor]`** in `mise.toml` (with the `mise doctor`-vs-`mise run doctor`
   disambiguation in its comment).

## Evidence (this machine, 2026-07-17)

`mise run doctor` → **8/8 ok** (boltffi 0.27.5, Xcode, xcodegen, full Android stack, Chrome),
both manual notes printed, exit 0. Against a deliberately broken environment
(`ANDROID_HOME=/nowhere CARGO_HOME=/nowhere PATH=/usr/bin:/bin`): **3/8 ok, five MISSING rows
each with its remedy line** (`mise run setup:boltffi`, `brew install xcodegen`, the two
`sdkmanager` commands, the SDK install) — and still exit 0.

## Falsification (all watched red, then restored green)

| Planted | Watched |
|---|---|
| Chrome row's mapping dropped (`tasks: &[]`) | manifest red: `mise.toml tasks with no doctor row and no recorded exemption: ["test:web"]` |
| Phantom `[tasks."probe:phantom"]` in mise.toml | manifest red naming `["probe:phantom"]` |
| Exemption renamed `doctor` → `doctor:gone` | **both directions at once**: `exemption names task 'doctor:gone', which mise.toml no longer declares` + `["doctor"]` uncovered |
| `BOLTFFI_PINNED` → `"0.27.4"` | cross-pin red: `doctor::BOLTFFI_PINNED (0.27.4) != setup:boltffi's want= (0.27.5) — a version bump must move both, in one commit` |
| Broken environment (above) | five MISSING rows with remedies; exit stayed 0 (warn-never-fail held under failure, not just success) |

`mise run check` green after every restore; final tree clean.

## Deviations from the plan

1. **The `doctor` verb landed in M2, not M3.** The manifest's both-directions check requires
   `[tasks.doctor]` to exist (its own exemption row would otherwise name an unknown task), so
   the verb rode the M2 commit and M3 reduced to the evidence run. Ordering artifact, not scope
   change.
2. None otherwise — the plan's shape survived contact unchanged.

## Friction log

1. **Doctor restates guard literals beyond the one that is cross-pinned.** The SDK default
   path, the image dir, the remedy strings all appear in both `mise.toml` and the table; only
   the boltffi version has a both-ways pin, because it is the one literal whose silent drift
   changes a *judgement* (ok vs MISSING on a healthy machine). The rest are stable strings
   whose drift degrades a message, not a verdict — accepted, recorded here. If a second
   judgement-bearing literal appears, pin it the same way before shipping it.
2. **The Chrome probe is macOS-shaped** (`/Applications/Google Chrome.app` as an absolute
   path, mirroring `test:web`'s guard) and untestable synthetically; the command-name arm
   (`google-chrome`/`chromium`) carries Linux. A future Linux dev machine will want the bundle
   check behind a cfg — not needed while `check` runs on this Mac and Linux is a container.
3. **The boltffi probe must extend PATH with `${CARGO_HOME:-~/.cargo}/bin` exactly as the task
   guards do** — otherwise doctor and the tasks can disagree about *which* boltffi is "the"
   binary. The bin does; the judgement (substring match, the guard's own `grep -q` predicate)
   lives in the tested module.
4. **`doctor require <task>` (guards delegating to doctor) stays unbuilt** — the guards double
   as env setup (ANDROID_HOME defaulting, NDK selection, PATH extension) and doctor cannot
   export into a calling shell; delegation would split each guard in two for no rung gain. The
   coverage manifest already forces the decision the delegation would enforce. Revisit only if
   a guard and a doctor row are ever caught disagreeing in practice.

## Open questions

None for ARCHITECTURE §9. `bolted new`'s gate (first out-of-tree framework consumer / a
publishing story) is recorded in the ROADMAP 23+ row.
