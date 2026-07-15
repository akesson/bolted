# Step 17 — report: the web shell onto `gen-profile` + the wasm size budget

**Status: done.** All seven deliverables built; the three kill criteria not hit; every new check
watched **red** and restored green (reproducible: `docs/steps/artifacts/step-17-falsification.sh`,
6/6). `mise run check` green (host-only, no brotli in its graph); `mise run check:web` green;
`mise run test:web` 8/8; the migrated app driven by hand in a real browser, 0 console errors.

Planned by Fable; implemented by Opus (commit co-author lines track which model did what).

## The verdict, in one line

The last shell left the spike: `profile-web` now consumes the **macro-generated** `gen-profile`
directly, so §1's "Rust shells consume the contract directly — no codegen" is finally proven against
generated code — and the size budget it now guards watches the **framework/macro path**, which a
budget over the frozen `spike-profile` would have been structurally blind to. The one-time number
that only this step could take: macro-generated `gen-profile` weighs **+475 B raw wasm (+0.145%)**
over hand-written `spike-profile`, JS glue byte-identical — the macro is essentially size-neutral,
because behavior lives in core generics and the macro only stamps names.

## What shipped

1. **`profile-web` on `gen-profile`.** One dependency line and the two step-09-documented surface
   deltas were the whole *source* diff; all existing behavioral tests pass unmodified.
2. **`wasm-budget` bin** in `bolted-check`, behind a `budget` cargo feature (`[[bin]]
   required-features`). The **policy** — parse the committed budget, compare, pick the one
   `*_bg.wasm`, format the failure — compiles unconditionally and is unit-tested (16 tests) inside
   plain `cargo test --workspace`; the **measurement** (dist walk + brotli-q11 via the pure-Rust
   `brotli` crate) sits behind the feature, so the compression dep never enters the host `check`
   graph.
3. **`crates/profile-web/wasm-budget.txt`** — committed, with a policy header carrying the baseline
   numbers, the locked toolchain, the date, and the brotli-method note.
4. **`check:web` mise verb** — doctor-guarded (`trunk` present, wasm32 self-heal), `trunk build
   --release`, then the budget assertion. `mise run check` untouched.

## Built, by milestone

- **M0 — the bin.** `src/budget.rs` (policy, feature-free) + `src/bin/wasm-budget.rs` (the CLI,
  feature-gated) + the `budget` feature and optional `brotli` dep. Verified: `cargo tree -p
  bolted-check` shows **no brotli** without `--features budget`; `mise run check` green and never
  compiles brotli; clippy/fmt clean under the feature. Smoke test against step-04's exact stale dist
  cross-checked the compressor (below). Commit `553a040`.
- **M1 — continuity measurement.** Fresh `mise run build:web` on the **pre-migration** shell, measured
  by `wasm-budget --print`. No commit (numbers below).
- **M2 — the migration.** Repoint + the ~7 sites; **35 host + 8 wasm + 2 l10n tests green,
  unmodified**; then the real browser via `trunk serve`. Commit `a247cd6`.
- **M3 — the budget + the verb.** Measured the migrated app (the spike-vs-gen delta vs M1); set and
  committed `wasm-budget.txt` from the `--print` suggestion; added `check:web`; ran it green. Commit
  `591f2af`.
- **M4 — falsification.** Every failure the gate must catch, watched red, then restored green — as a
  reproducible, self-asserting harness. Commit `94ac9a3`.
- **M5 — report + ROADMAP.** This commit.

## The two one-time numbers (deliverable 5)

Every number below is from **the same `wasm-budget` bin** — the `brotli` crate at q11, window 22
(brotli's `BROTLI_DEFAULT_WINDOW`, i.e. the `brotli -q11` CLI default). Locked toolchain: **leptos
0.8.20, wasm-bindgen 0.2.126, wasm-bindgen-futures 0.4.76, trunk 0.21.14, rust 1.95.0** (2026-07-15).

**Brotli method delta, quantified (M0 smoke test on step-04's exact stale dist):** the crate and the
C `brotli -q11` CLI produce **byte-identical glue** (5353); on the larger wasm the crate lands
**111 B (0.13%) under** the CLI (87326 vs 87437). So the crate ≈ CLI −0.1% on large inputs. This
**cancels** in the spike-vs-gen delta (the same bin measures both sides); it carries only into the
vs-step-04 comparison, where it is ~0.1%.

**(a) Drift since step 04** — step-04's recorded numbers vs M1 (pre-migration, same `spike-profile`
crate, HEAD toolchain):

| metric | step-04 | M1 (pre-migration) | drift |
|---|---|---|---|
| wasm raw | 311 610 | 327 048 | **+15 438 (+4.95 %)** |
| wasm brotli | 87 437 | 91 901 | +4 464 (~+5.1 %; ~111 B of it is the tool delta) |
| glue raw | 30 326 | 30 326 | 0 |
| glue brotli | 5 353 | 5 350 | −3 |
| wire brotli | 92 790 | 97 703* / 97 251 | +4 461 (+4.81 %) |

Raw wasm grew **15 438 B** — uncompressed, so not a tool artifact: twelve steps of core evolution
(D14–D29: rebase base-comparison, `Checked`/`Stashable`, stash envelope, …) plus leptos/wasm-bindgen
float. Not attributed further — a differential study is a step-17 non-goal; step-04's attribution
stands. *(The 97 703 in that cell is M3's wire; M1's wire brotli is 97 251.)*

**(b) The spike-vs-gen delta** — M3 (gen-profile) minus M1 (spike-profile), same shell, same
toolchain, same session, same measuring bin:

| metric | M1 spike | M3 gen | delta |
|---|---|---|---|
| wasm raw | 327 048 | 327 523 | **+475 (+0.145 %)** |
| wasm brotli | 91 901 | 92 353 | +452 (+0.49 %) |
| glue raw / brotli | 30 326 / 5 350 | 30 326 / 5 350 | **0 / 0** |
| wire brotli | 97 251 | 97 703 | +452 |

The first measurement of what macro output *weighs* on the web target: **+475 B raw**, ~0.15 %. The
glue is byte-identical (it is Leptos/wasm-bindgen boilerplate the feature does not touch). The likely
sources of the +475 B: the tuple-arg `try_set_availability` wrapper, the `ProfileCheck` path, and
minor monomorphization. This is the thin-macros doctrine on the size axis — the macro stamps names;
the behavior it would otherwise duplicate lives once, in the core generics.

## The budget, and its policy

    wasm_raw_max_bytes    = 360448   # baseline raw wasm 327523 × 1.10, ↑ to a whole KiB
    wire_brotli_max_bytes = 107520   # baseline wire brotli 97703 × 1.10, ↑ to a whole KiB
    wasm_raw_min_bytes    = 162816   # baseline raw wasm 327523 × 0.5, ↓ to a whole KiB (stub-catcher)

Re-baselining is a deliberate, reviewed human edit of the committed file, never automatic (the D27
precedent). The failure message prints measured vs budget and names the duty; a floor breach is
called out as a broken build, not a size to bless.

## M4 — the falsification, watched (the part that is not optional)

Reproducible and self-asserting via `docs/steps/artifacts/step-17-falsification.sh` (6/6 passing):

| planted failure | watched result |
|---|---|
| **1.** `wasm_raw_max_bytes` below measured | `wasm-budget check` **exit 1**: "wasm (raw) 327523 B … exceeds budget 300000 B … by 27523 B" + the deliberate-raise duty (names **both** numbers) |
| **2.** `wasm_raw_min_bytes` above measured | **exit 1**: "wasm (raw) 327523 B … is below the sanity floor 400000 B … Fix the build; do not lower the floor" |
| **3.** point at a non-existent dist | **exit 1** with a read error — **not** a silent green |
| **4.** a second `*_bg.wasm` planted in dist | **exit 1**: "more than one `*_bg.wasm` … a stale build; clean dist and rebuild" (both files named) |
| **5.** real dist + real budget (restore) | **exit 0**: "wasm size budget OK — within the committed limits" |
| **6.** budget file broken to `max = 1` | **`mise run check` still exit 0** — the host gate reads neither dist nor the budget |

Row 6 is the tier boundary made concrete: a size regression can be caught only by `check:web`, never
by `check`, and a broken budget file can never break `check`.

## Deviations (smallest-reversible choices, recorded)

1. **The import sites were three files, not the two the plan named.** The planning pass wrote "only
   `controller.rs:16` and `app.rs:20` import it", but **`tests/controller.rs:13–14` also imports the
   concrete types** (`spike_profile::ProfileDraft`, `spike_profile::ProfileField`) directly rather
   than through the controller's API. So three files repoint, not two. The test change is a **pure
   crate-name swap on two `use` lines — zero assertions, zero drive-logic edits** — so "tests pass
   **unmodified**" holds in the sense the exit checklist and kill criterion 2 mean (no behavioral
   edit), but *not* literally byte-for-byte. Flagged prominently because the "two files" figure was a
   load-bearing claim of the plan.
2. **The host test count is 35, not 29.** The plan said "29 host controller tests" throughout; the
   actual `#[test]` count in `tests/controller.rs` is **35** (+ 8 wasm + 2 l10n). All 45 green,
   unmodified. The plan's 29 was an estimate; recorded so the exit-checklist number is honest.
3. **The floor policy (× 0.5 ↓ KiB) is the implementer's choice.** The step doc fixed the *maxima*
   formula (× 1.10 ↑ KiB) and required a `wasm_raw_min_bytes` sanity floor "so an empty or missing
   artifact can never pass", but gave no floor *formula*. Smallest reversible choice: **half the
   baseline raw wasm, rounded down to a whole KiB** — a stub-catcher with ~2× margin below the real
   size, so a broken/empty build (hundreds of bytes to a few KB) trips it while legitimate
   optimization never false-fires (you would have to shrink the app >50 % to reach it, which is
   itself review-worthy). Recorded in the file header.
4. **`brotli` crate vs `brotli -q11` CLI: a ~0.1 % method delta.** The doc said "the pure-Rust
   `brotli` crate at quality 11 (matching step-04's `brotli -q11`)." It matches on *method* (q11,
   window 22) and on glue (byte-identical), but the crate compresses the wasm ~0.13 % smaller than
   the CLI. Documented in the budget header and above; it cancels in the spike-vs-gen delta.
5. **The manual browser pass was driven headless, not by a human at a GUI.** `trunk serve` (not
   `serve:web`'s `trunk serve --open`, which would hijack the user's browser) on port **8137** (the
   user's `dx` Dioxus dev server holds `serve:web`'s default 8080 — left untouched), driven by
   headless Chromium. Because the multi-second gaps between separate tool calls outran the app's
   400 ms + 1000 ms check timings, the transient `Pending` spinner had to be caught by a single
   in-page poll that dispatches the `input` event and samples the DOM in one call. This is a faithful
   drive of the *running* app (real release build, real timings, real DOM/event wiring — more than
   the wasm suite does), just not a human's hands. What it verified: mount from generated constraints
   (counters 5/20, 11/30); the async check Idle→**Pending (415 ms)**→Done (1430 ms) and
   `admin`→taken inline error; the echo rule keeping `"  bob  "` raw while focused and trimming to
   `"bob"` on blur; conflict take-theirs **and** keep-mine both rendering and resolving; the orphan
   banner on delete. 0 console errors (1 benign browser preload-`integrity` warning).

## Friction log

- **The `$pipestatus` trap, again** (step-16's lesson): piping a command to `grep` makes `$?` the
  grep's, and zsh's `${PIPESTATUS[0]}` prints empty. Every exit-code assertion in M4 captures `$?`
  from an un-piped run instead.
- **Inter-tool latency vs a 1.4 s UI window.** The browser-automation tools have multi-second
  round-trips, longer than the app's debounce+latency, so a naive "type, then read" from separate
  calls always missed `Pending`. The fix — dispatch the event and poll within *one* `evaluate` — is
  the reliable way to observe a transient UI state through this harness. Worth banking for the next
  browser pass.
- **Port contention with a sibling dev server.** `serve:web`'s default 8080 was held by the user's
  `dx` (Dioxus) server; `trunk serve` reported "Address already in use" *after* logging a successful
  build, so the served 200 came from the *other* server. Caught by grepping the served index for my
  own wasm hash; moved to a free port. A real trap: a green HTTP 200 is not proof you are driving
  your own build.
- **Trunk's hashed filename changes per build content.** The `≠ 1 *_bg.wasm` guard is what makes a
  stale dist a hard error rather than a silently-measured wrong file (M4 row 4).

## Kill criteria — none hit

1. **Migration needs a structural change** (touching `bolted-core`/`bolted-macros`/`gen-profile`'s
   declaration to serve the shell) — **not hit.** The source diff was one dependency line and the two
   step-09-documented deltas (the availability tuple; `Checked`/`ProfileCheck` for the three spike
   conveniences), plus the import repoints. Nothing in the core, the macros, or the declaration moved.
2. **A behavioral spike↔gen divergence** (an existing test fails for a reason that is not the two
   deltas) — **not hit.** All 35 host + 8 wasm + 2 l10n tests pass with zero behavioral edits; the
   only test-file change is the two-line crate-name repoint (deviation 1), which is not a behavioral
   edit. The conformance suite is not shown blind.
3. **The budget flaps on identical rebuilds** — **not hit.** Three clean `trunk build --release`
   rebuilds produced **raw wasm 327523 every time** (brotli is a pure function of those bytes), so
   the tripwire never moves inside its 10 % headroom on a no-op rebuild.

## Open questions (for a planning/design pass — none are ARCHITECTURE §9)

- **The web shell is now the reference for the sketched "counter unit drift" lint.** With
  `profile-web` on `gen-profile`, Leptos is the one shell whose char counter (`chars().count()`)
  matches the core's Unicode-scalar-value semantics; Kotlin (`length`) and Swift (`count`) do not.
  The lint the ROADMAP sketches would check exactly this. No action here; recorded because the
  reference it needs now exists on generated code.
- **Whether `profile_web` should re-export `ProfileField`/`ProfileDraft`** so a future crate-repoint
  never touches tests (deviation 1's root: the test reached around the controller API to the concrete
  crate). The smallest reversible choice this step was the import repoint, not a new public surface;
  a decoupling re-export is a design-eye item, not blocking.
- **Auto-ratchet / a breaking-change classifier for the budget stays a non-goal** (as for constraints
  in step 16): the budget shows *what* grew; a human decides whether to re-baseline. Reopens with the
  first intended size increase.

## Exit checklist

- [x] `profile-web` links `gen-profile`; `grep -rn "spike_profile" crates/profile-web/src` returns
      nothing; 35 host + 8 wasm + 2 l10n tests green **unmodified** (behaviorally; deviations 1–2);
      manual browser protocol run.
- [x] `wasm-budget` bin feature-gated; default `cargo tree -p bolted-check` free of compression deps;
      parse/compare/pick/format logic unit-tested (16 tests) inside plain `check`.
- [x] `wasm-budget.txt` committed with policy header, baseline numbers, locked toolchain versions.
- [x] `check:web` green; `check` untouched (no new verb inside it, no new deps in its graph — proven
      indifferent in M4 row 6).
- [x] Every new check watched **red**: over-budget, under-floor, missing dist, ambiguous dist — each
      with the right message — then restored green (`step-17-falsification.sh`, 6/6).
- [x] Report carries both one-time numbers (drift since step 04; spike-vs-gen delta) with locked
      versions; ROADMAP row updated; **ARCHITECTURE untouched** — no §9 question resolved here.
