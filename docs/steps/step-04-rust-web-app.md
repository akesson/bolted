# Step 04 — Rust web spike app (Leptos)

**Phase 1 · Spike.** Read first: [VISION.md](../VISION.md) (the **web target**: "Rust web
frameworks consuming the core as a plain crate, in the browser only. Never a webview." — bet 2;
the verification ladder — this shell is **rung 1**, a Rust consumer proven by rustc with no
codegen, and it seeds a **rung-3** build check: wasm32 discipline; the "no constraint literals in
shells" rule), [ARCHITECTURE.md](../ARCHITECTURE.md) (§1 the three verbs + "**Rust shells consume
the contract directly as a crate — no codegen**"; §2 validation timing + value-bound async
verdicts; §4 drafts + live rebase + the conflict ceiling; §5 **sans-io core → wasm32 is
structural**; §6 the text echo rule; §9 the OPEN items this step gathers evidence for),
[ROADMAP.md](../ROADMAP.md) (step-04 sketch + working agreement), and the three prior handoffs
([step-01](step-01-report.md) · [step-02](step-02-report.md) · [step-03](step-03-report.md)).

This step puts the **same profile feature on a second face** — a Leptos browser app — but reaches
it a completely different way from the Apple shell: **zero FFI, no codegen, no BoltFFI**. The Swift
shell (steps 02–03) went through generated bindings and a re-owned store loop; the web shell
consumes `bolted-core` + `spike-profile` **directly as crates** and drives everything in one
single-threaded wasm module. That difference is the whole point of the step.

## Goal

Prove the **zero-FFI Rust-shell path** end to end, and measure it. Four things are genuinely new
here (the profile *semantics* are already proven by steps 01–03 — do **not** re-litigate them):

1. **wasm32-unknown-unknown discipline holds for the whole core.** `bolted-core` + `spike-profile`
   compile to `wasm32-unknown-unknown` and run in a browser with **no added dependency, no runtime
   shim, no `tokio`**. *(Pre-verified at plan time: both crates build clean for the target in
   ~0.4 s. Milestone 1 confirms it stays true once the app layer is added.)* This is §5's
   "sans-io → wasm32 is structural, not aspirational" on trial, and the seed of a future
   `bolted-check` size/target gate (Phase 4).
2. **`bolted_core::Store` gets its first real UI consumer.** The FFI wrapper **bypassed `Store`
   entirely** (`Rc<RefCell>` isn't `Send`, so it re-owned the whole loop — step-02 report). A
   browser is single-threaded, so `Rc<RefCell>` **fits natively**. Step 03's core fix A1
   (`Store::submit` returns the handle on refusal) was made **explicitly for this shell**
   (step-03 doc, Deliverable A1: "keeps the pure-Rust `Store` honest for the Rust-web shell that
   consumes it directly"). Step 04 is where `Store` either earns its keep or reveals its gaps.
3. **The sans-io async check, driven from the browser with no executor in the core.** Username
   uniqueness runs as `begin_username_check()` → `spawn_local(async move { …await… })` →
   `complete_username_check(token, verdict)`. **The shell owns the executor
   (`wasm-bindgen-futures`); the core stays pure.** This is the web analog of the Swift capability
   trait, and it is the sharpest demonstration of the sans-io claim.
4. **The WASM bundle size, measured.** The release `.wasm` (post `wasm-opt`) and its gzip/brotli
   wire size become the **baseline** for VISION's `bolted-check` size budget — continuity with
   step-02's `dist/apple` 127 MB and step-03's LOC counts. **No threshold; this IS the baseline.**

On top of those, the shell **faithfully reproduces the step-03 feature** (all four behaviours + the
server-simulator pane) so the freeze gets a real **Swift-shell-vs-Rust-shell friction comparison**.
The four behaviours are on trial again only insofar as the *shell pattern* is new (a signal
framework, not `@Observable`):

- **Echo rule** — the cursor survives per-keystroke `try_set` + core sanitization, now via an
  `<input>` bound to an `RwSignal` (§6). Same litmus test: the shell adds *when*, never *what*.
- **Live rebase** — a simulator mutation flows into the open draft; clean fields adopt, dirty
  fields conflict — but reactivity now comes from a **manual version tick**, not a snapshot stream.
- **Conflict UI** — keep-mine / take-theirs from `Field` data alone.
- **Submit flow** — validation report / conflict refusal / success-via-canonical, with **F3** (a
  refused submit leaves the draft alive) proven on the real `bolted_core::Store::submit`.

**The output that matters is evidence.** A running app and a green suite are necessary but not the
deliverable — the answered probe matrix, the bundle-size number, and the friction log in
`docs/steps/step-04-report.md` are. Every place the shell must fork draft state into signals,
restate a constraint, or fight the framework to observe a core-owned mutation is a **freeze
finding** — record it, don't paper over it.

## Non-goals (hard boundaries)

- **No core changes. None.** `bolted-core` and `spike-profile` are **frozen** — proven by steps
  01–03, and step 03's A1 already made `Store` web-ready. If the shell seems to need a core change,
  that is a **finding to record** (smallest-reversible elsewhere, or stop-and-report if structural),
  **not** a change to make. `git diff` on `crates/bolted-core` and `crates/spike-profile` must be
  **empty** at exit. (`spike-profile-ffi` is irrelevant here — the web path never touches it.)
- **Browser CSR only.** Leptos with the **`csr`** feature. **No SSR, no server functions, no
  hydration, no backend** — those reintroduce a server runtime and break "browser only, plain
  crate, zero FFI". The "server" is the same in-app **simulator pane** step 03 used
  (`apply_canonical(...)` buttons).
- **No create-flow, no persistence, no real network, no i18n infrastructure.** A `key → English
  template` map in Rust renders errors (params come from `ErrorData`; the shell owns only the
  sentence, never the numbers) — the direct analog of step-03's `Localization`. No stash/restore
  (Phase 2).
- **`mise run check` stays Xcode-free AND host-only.** Do not fold any wasm build or browser test
  into `check` (a bare box may lack the wasm target / a browser). New web work lives behind
  `build:web` / `test:web`, exactly as the Apple work lives behind `pack:apple` / `test:apple`.
- **No macros, no published crates, no perf tuning, no second framework.** Leptos only (chosen this
  session). If Dioxus ever wants a look, that is a *separate* step, not a fork of this one.
- **Do not resolve any ARCHITECTURE §9 OPEN question.** Several get *evidence* here (store
  concurrency, draft lifecycle, the observe verb for Rust shells, focused-field-during-rebase) and
  must be left OPEN — recorded in the report, decided at the freeze.

## Deliverable A — wasm toolchain + mise wiring (the Xcode-free-equivalent checkpoint)

The web analog of `pack:apple`: get the core-as-a-crate building and running in a browser, behind
mise verbs, without touching `check`.

- **`build:web`** — release-build the Leptos app to wasm and bundle it. Recommended tool: **Trunk**
  (`trunk build --release`, the idiomatic Leptos CSR bundler; runs `wasm-opt`). *Tool choice is
  smallest-reversible latitude* — plain `wasm-bindgen` + `wasm-opt` + any static server is
  acceptable if Trunk proves awkward; record what you used and why. Doctor-fail clearly if the
  wasm target / Trunk is absent (mirror `pack:apple`'s guard messages), and **self-heal the target**
  (`rustup target add wasm32-unknown-unknown`) the way `pack:apple` self-heals the Apple targets.
- **`serve:web`** — `trunk serve` (or equivalent) for the manual protocol; opens the app in a
  browser. The web analog of `run:apple`.
- **`test:web`** — the headless browser suite (Deliverable C). `wasm-bindgen-test` via
  `wasm-pack test --headless` (Chrome or Firefox) — doctor-fail if the browser/driver is absent.
  **Contrast to bank:** unlike XCUITest (step 03: GUI-session + Accessibility-gated, never
  headless), `wasm-bindgen-test` **runs headless in CI** — record this as a genuine advantage of the
  Rust-web tier over the Apple UI tier.
- **Gitignore** the wasm build output (`dist/`, `target/`, `.trunk/` as applicable). Whether a
  lightweight wasm-core build (`cargo build --target wasm32-unknown-unknown -p bolted-core
  -p spike-profile`) should also live in `check` is a **smallest-reversible call** — the durable
  home for that discipline gate is `bolted-check` (Phase 4); note your choice in the report.

## Deliverable B — the Leptos app (`crates/profile-web`, a new crate)

A browser CSR app that is the **hand-written stand-in for a future generated Leptos shell** — the
same "write what the codegen would emit" discipline steps 01–03 used, now for a Rust web face. It
sizes the eventual Leptos/Dioxus generator (a Phase-3+ concern) and, more immediately, tells the
freeze whether a Rust shell wants the snapshot-stream abstraction at all.

### B1. Crate shape

- A new workspace member `crates/profile-web` (a `cdylib`/`rlib` as the bundler needs), depending on
  `spike-profile` (which re-exports `Profile`, `ProfileStore`, `ProfileHandle`, `ProfileField`, the
  value types) and `bolted-core` (for `Field`, `Validity`, `SyncState`, `CheckState`, `ErrorData`,
  `Constraint`, `ValidationReport`, `SubmitError`/`SubmitFailure`, and the `Draft` trait for
  `dirty_fields`/`conflicts`/`validate`/`status`). Plus `leptos` (`csr`),
  `wasm-bindgen-futures`, and a wasm timer (`gloo-timers` or equivalent) for the debounce + the
  simulated async check. **These are the *shell's* dependencies — they must not leak a wasm-hostile
  requirement back into the core** (the core stays zero-dep; the kill criterion below is about the
  *core*, not the app's own crates).
- **No constraint literal anywhere in `profile-web`.** `maxLength`, char counters, and required
  markers derive **only** from `ProfileField::constraints()` (`Required` / `LenChars{min,max}` /
  `Custom(key)`). A magic `20` or `30` in the web code is a defect — the same greppable rule as the
  Swift shell (ARCHITECTURE §1).

### B2. The reactivity pattern (the crux — read carefully)

`bolted_core::Store` + `DraftHandle` are **plain, non-reactive** Rust. Reads happen through
`handle.borrow()`; the store mutates the draft **underneath** the shell on `apply_canonical`
(live rebase). Leptos won't know a `borrow()` changed. So the shell needs an explicit reactive
**tick**:

- Hold the `ProfileStore` and the `ProfileHandle` for the app's lifetime. **`DraftHandle` is
  `!Clone` by design** (single ownership is what lets `submit` move the draft out via
  `Rc::try_unwrap`), and Leptos closures want `'static`/`Copy` captures — so the handle almost
  certainly parks in a `StoredValue`/`RwSignal` (or an `Rc`-shared cell). **Record the ergonomics of
  that head-on:** does a single-owner `!Clone` handle compose with a signal framework, or does it
  force gymnastics? This is direct evidence for the §9 draft-lifecycle question and for whether the
  `Store`/`DraftHandle` public shape serves a reactive shell.
- Keep a `version: RwSignal<u64>`. **After every operation that can change the draft** — a
  `try_set_*`, a `resolve_*`, a check begin/complete, a `store.apply_canonical`/`submit` — bump it.
  Every derived read (`Memo`/`Signal` over a field's `validity()` / `sync()` / `is_dirty()`, the
  dirty/conflict lists, the check state) **depends on `version`** and re-reads `handle.borrow()`.
- **The freeze finding this produces:** a Rust shell may not need the **snapshot-stream** at all
  (§4: "drafts expose their own snapshot stream") — it can read the contract directly and drive
  reactivity from `version`/an explicit tick. Record whether read-direct-plus-tick is *clean* or
  whether the missing change-notification hook is real friction. That is the answer to "what does
  '**Rust shells consume the contract directly — no codegen**' (§1) actually cost?"

### B3. The echo rule (ported to signals)

Per text field: an `<input>` whose value is an editing `RwSignal<String>` the user types into
freely. On each `input` event, call the matching `try_set_*` (so validation, counters, and the
debounced check all run **per keystroke** — the bet is exercised, not bypassed), **but never write
the core's sanitized value back into the input's signal while the field is focused** — that
write-back is what would move the cursor. Refresh the buffer from the field (`Valid` value, or
`Invalid.raw` to keep the user's rejected text) **only** on blur or on an external change
(rebase / take-theirs). This is §6 in a signal framework; the *mechanism* is latitude, the
invariant "the focused buffer is never overwritten from core" is not. **Litmus test:** if keeping
the cursor stable forces the shell to re-implement trim/lowercase or restate a length, that is a
**kill** (below), not a friction.

### B4. The async check via `spawn_local` (the sans-io headline)

Debounce the username (a shell-taste constant — allowed; it's *when*). When the username is valid
and dirty, drive the single-flight from the browser:

```rust
let token = handle_borrow_mut().begin_username_check();   // Pending; bump version → spinner shows
let handle = /* shared Rc/StoredValue clone of the cell */;
wasm_bindgen_futures::spawn_local(async move {
    gloo_timers::future::TimeoutFuture::new(1000).await;  // simulated "server" latency
    let verdict = simulated_lookup(&name);                // Result<(), ErrorData> from a taken-set
    handle.borrow_mut().complete_username_check(token, verdict); // stale token → ignored (I10)
    bump_version();
});
```

Bind the spinner to `username_check_state() == CheckState::Pending`; render `Done { verdict:
Err(e) }` as the `username_unique` message. Because a value change **resets** the check
(`with_username_guard`, I13), typing through a pending check invalidates the in-flight verdict and
the late `complete` returns `false` — **the spinner behaviour falls out of the contract, not shell
bookkeeping**, exactly as it did in Swift. This proves the async model on wasm with **no executor
in the core**: `spawn_local` is the shell's; the core only produced a `CheckToken` (data).

### B5. Views (the full form)

The profile form — username / name / email inputs + the availability **date pair** (two date
inputs → `try_set_availability(start, end)`) — with, per field: the error text (from the
`key → template` map + `ErrorData` params), a char counter + required marker **derived from
`constraints()`**, and a dirty indicator. Plus:

- A **conflict banner** per conflicted field showing mine (the field's own validity) vs theirs
  (and base, from `SyncState::Conflicted { base, theirs }`) with **keep-mine / take-theirs** buttons.
- A **submit** button rendering the returned `SubmitFailure` per-field (validation report) and the
  `Conflicted { fields }` / `Orphaned` outcomes. On success the canonical/server pane updates **via
  `store.canonical()`** (not the shell's own input echoed back), and the shell re-`checkout()`s and
  re-binds (record the ergonomics of that hand-off — it informs §9 draft lifecycle, just as the
  Swift re-checkout did).
- A **server-simulator pane**: shows `store.canonical()` and offers buttons calling
  `store.apply_canonical(preset)` and `store.delete_canonical()` — the live-rebase / conflict /
  orphan driver, standing in for a backend.

## Deliverable C — tests (host-side logic + headless wasm)

Two tiers, mirroring step-03's "VM tests on host + a UI tier that needs the real platform":

- **Host-side (`cargo test`, no browser).** Factor the shell's logic into a thin, framework-light
  **controller** over `ProfileStore` + `ProfileHandle` (the analog of step-03's `ProfileViewModel`,
  which ran headless). Test the four behaviours at that level — echo-rule buffer discipline
  (focused buffer not rewritten; `Invalid.raw` preserved; blur refresh), live rebase (clean adopts /
  dirty conflicts), conflict resolution (keep-mine / take-theirs incl. the check reset, I13 visible
  via `username_check_state()`), and submit (invalid → report; conflicted → refusal that **leaves
  the draft alive**, resolve + resubmit succeeds — **F3 on the real `bolted_core::Store`**; clean →
  success + canonical updates). These run in `check`-adjacent host builds and are the bulk of the
  automated coverage.
- **Headless wasm (`test:web`).** A small `wasm-bindgen-test` suite exercising the DOM path: an
  input `input` event runs `try_set_*` and updates validity/counter without the focused buffer being
  rewritten; a simulator click drives a rebase into a rendered field; the async check shows and
  clears a spinner. **This is the tier XCUITest couldn't be (step 03): it runs headless in CI** —
  prove that and record it. Keep it lean — the host controller tests own the semantics; the wasm
  tier only proves the DOM binding is wired.

The **focused-but-untouched field during rebase** (§9) is the one case that resists deterministic
end-to-end driving (step-03 finding 6, same reason: focus/blur can't be ordered against the async
rebase). Pin it at the **host/controller level** (the analog of step-03's
`testLiveRebaseFocusedCleanFieldStaleUntilBlur`); the wasm tier owns the unfocused adopt.

## Ordered milestones (milestone 1 is a clean, standalone checkpoint)

Treat **milestone 1 as a self-contained unit** — it proves the toolchain + the core-as-a-crate in a
browser, and could stand as its own commit before the full UI. If the session runs long, stop after
a completed milestone and report — a partial-but-clean result beats a rushed whole.

1. **wasm toolchain + skeleton.** `crates/profile-web` builds to wasm via `build:web`; a minimal
   Leptos CSR app checks out a draft from a seeded `ProfileStore` and renders **one field's value**
   from `handle.borrow()` in a browser. Confirms core-consumed-as-a-crate-in-wasm end to end +
   `build:web`/`serve:web` wired. *(The core's wasm compile is already pre-verified; this proves it
   survives the app layer + bundler.)*
2. **Skeleton + the echo-rule binding.** All fields bound; per-keystroke `try_set_*`; the focused
   buffer is never rewritten from core; blur refresh; `Invalid.raw` preserved. Host controller tests
   green.
3. **Live rebase + conflict UI + the simulator pane.** The version-tick reactivity; clean adopt /
   dirty conflict; keep-mine / take-theirs banners; `apply_canonical` / `delete_canonical` drivers.
4. **Submit flow + the async check.** `store.submit` with F3 recovery + re-checkout on success; the
   debounced `spawn_local` uniqueness check + spinner; `ProfileField::constraints()`-derived
   affordances (no literal in the web code).
5. **Bundle-size measurement + the manual protocol + the friction log.** Record the numbers; run the
   manual protocol in a browser (`serve:web`); write the report.

## Probe matrix

Automated rows are host-side controller tests unless marked **wasm** (headless `test:web`) or
**Manual** (a human at `serve:web`, recorded as observations).

**wasm32 discipline (new)**
- *Auto:* `bolted-core` + `spike-profile` build to `wasm32-unknown-unknown` **with no added
  dependency** (pre-verified; re-confirm after the app exists — the app's own deps must not drag a
  wasm-hostile crate into the *core*). Record the command and that it's clean.

**`bolted_core::Store` as a real consumer (new)**
- *Auto:* checkout → edit → submit → the committed entity becomes `store.canonical()` and a second
  live draft rebases onto it. Record whether `Store`'s public surface
  (`checkout`/`submit`/`apply_canonical`/`delete_canonical` + reading via the `Draft` trait) was
  **sufficient for a reactive shell** or wanted a change-notification hook (feeds §9 store /
  observe-verb).

**Echo rule**
- *Auto:* an `input` event calls `try_set_*` and updates validity/counter, but the focused buffer is
  not rewritten from core; blur refreshes to the sanitized value; a rejected edit keeps
  `Invalid.raw`.
- **wasm:** the same, through a real DOM `input` event.
- **Manual (headline):** type fast into username with leading/trailing spaces and mixed case into
  email — the **cursor never jumps**, no character is eaten. §6 on trial in a signal framework.

**Live rebase**
- *Auto:* simulator mutation while a field is clean → adopts silently (still `InSync`); while dirty
  → `Conflicted`, mine preserved, banner data present.
- **Manual:** a focused-but-untouched field under a rebase — record how the **stale-until-blur** feel
  compares to Swift (§9), and whether fine-grained signals change it.

**Conflict resolution**
- *Auto:* `resolve_keep_mine` → value=mine, base=theirs, still dirty, `InSync`; `resolve_take_theirs`
  → value=theirs, clean; take-theirs on username also **resets the check** (I13 visible via
  `username_check_state()`).
- **Manual:** edit a conflicted field until it equals *theirs* — it **stays `Conflicted`** (F6);
  record whether that reads as correct or surprising in this shell (compare to the Swift verdict).

**Async check**
- *Auto:* a debounce collapses a burst into a single `begin`; a value change during `Pending` resets
  the check (stale `complete` returns `false` — no late endorsement); `Done(Err)` surfaces
  `username_unique`; `Idle`↔`Pending`↔`Done` transitions are observable.
- **Manual:** with a ~1 s simulated latency, the spinner appears and clears; typing through it never
  shows a verdict for the wrong text.

**Submit flow**
- *Auto:* invalid → `SubmitError::Validation { report }` rendered per-field + rule errors;
  conflicted → `Conflicted { fields }`; success → the canonical pane updates via `store.canonical()`
  and the shell re-checks-out; a **failed** submit leaves the draft alive and editable (**F3 on
  `bolted_core::Store::submit`** — the first time this path runs against the real store).
- Record how often a **never-checked** username reaches a passing submit naturally (F2 evidence —
  compare to step-03's finding that it's the *default* path).

## Measurements (record; **no pass/fail thresholds** — a web baseline, not a gate)

- **WASM bundle size (the headline).** Release `.wasm` after `wasm-opt`, **and** its gzip + brotli
  wire size. If cheap, a rough breakdown (`twiggy top` / `cargo bloat`) of leptos-runtime vs
  wasm-bindgen glue vs core+feature. This is the baseline for VISION's `bolted-check` size budget —
  continuity with step-02 (`dist/apple` 127 MB) and step-03 (LOC). Note what dominates.
- **Cold `build:web` time** and the tool used (Trunk / manual) — the web analog of step-02's pack
  time.
- **Per-keystroke `try_set_*` → version-tick → view-update:** does per-keystroke `try_set` feel
  instant in the browser? (No threshold; Apple carried no evidence for JNI, and the browser carries
  none either — but record the *feel*, and any jank, per the latency kill below.)
- **Line counts:** `profile-web` source vs step-03's Swift app (787 LOC VM+views+localization) — the
  honest Rust-shell-vs-Swift-shell size comparison, and an input to the eventual Leptos generator.

## Kill criteria ("broken" = no reasonable shell-side pattern fixes it)

Framework awkwardness, an ugly binding, `StoredValue` gymnastics for the `!Clone` handle, or a
design rule you had to adopt = **friction finding**, not a kill. A kill is one of these; on hitting
one, **stop, write the minimal repro, and report** — do not engineer around it.

- **wasm32 discipline** — `bolted-core` or `spike-profile` **cannot** reach
  `wasm32-unknown-unknown` without adding a dependency or a runtime shim (something drags in
  `std::time`/threads/`getrandom`/an executor into the **core**). That falsifies §5's structural
  wasm claim and VISION bet 2 → architecture session. *(Pre-verified false today; the kill is if the
  app layer forces a core change to keep the core wasm-clean.)*
- **Observe over a mutating core-draft** — the shell **provably cannot** re-render on a rebase
  (`apply_canonical` mutating through `Rc<RefCell>`) without **copying the draft's field state out
  into signals** — i.e. forking the very logic §4 forbids ("a detached value-copy would fork
  logic"). Read-direct + version-tick failing to close this → design session on the **observe verb
  for Rust shells** (does §4's snapshot stream become mandatory here?).
- **Async needs an executor in the core** — driving the single-flight from the browser **cannot** be
  done shell-side (`spawn_local`) and forces the *core* to host async. That breaks sans-io on wasm →
  stop and report (it is the falsifiable form of §5's central claim).
- **Echo rule** — keeping the cursor stable is impossible without the shell **re-implementing
  sanitization or restating a constraint** (the §2 litmus test fails). Same kill as step 03, now for
  the Leptos binding.

**Not a kill:** the WASM bundle size (baseline, no threshold); a large `.wasm`; `StoredValue`/`Rc`
gymnastics to park the handle; per-keystroke jank that a shell-side pattern (batching the tick)
resolves. Perceptible, unfixable per-keystroke lag *is* a soft misfire worth flagging (previews the
step-05 JNI concern), but the browser is not the JNI worst case — record it, don't treat it as the
bet failing.

## Exit checklist

- [ ] `mise run check` green — **unchanged, host-only, Xcode-free** (no wasm/browser task folded in).
- [ ] `git diff` on `crates/bolted-core` and `crates/spike-profile` is **empty** (core frozen).
- [ ] `mise run build:web` builds the release wasm bundle from a clean clone (self-heals the wasm
      target; doctor-fails clearly if the toolchain is absent).
- [ ] `mise run test:web` green — the headless wasm suite, **demonstrably runnable without a GUI
      session** (the XCUITest contrast, recorded).
- [ ] `mise run serve:web` launches the app in a browser.
- [ ] Host-side controller tests green for all four behaviours + F3 on the real `Store`.
- [ ] All probe-matrix rows answered with *observed* behaviour; the **manual protocol** executed and
      recorded; the **WASM bundle size** measured and written down.
- [ ] `docs/steps/step-04-report.md` written: what was built; the wasm-discipline result; whether
      `bolted_core::Store` served a reactive shell or showed gaps; the reactivity pattern
      (read-direct + tick vs. wanting the snapshot stream); the `!Clone` handle ergonomics; the four
      behaviours' verdicts **compared to the Swift shell**; §9 evidence (store concurrency, draft
      lifecycle/`close()`-not-needed-in-Rust, focused-field-during-rebase, observe verb for Rust
      shells); the bundle size + line-count comparison; any friction consuming the core directly.
- [ ] ROADMAP.md status updated (04 → done; 05 stays pending — it "may run parallel with 03/04 once
      02 is done", so no promotion is forced).

## If you hit a wall

Same rule as steps 01–03 (CLAUDE.md): an omitted decision → the smallest reversible choice, recorded
in the report. A **structural** conflict — needing a core change (the diff must stay empty), a trait
or invariant change, an ARCHITECTURE edit, or resolving a §9 OPEN question — means **stop and record
the question** for a design session; do not resolve it here. The kill criteria above are the
explicit stop-and-report triggers, and hitting one is a *successful* probe outcome (it falsified a
design bet cheaply), not a failure of the step.
