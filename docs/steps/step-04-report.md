# Step 04 — Rust web spike app (Leptos) — Report

**Status: all five milestones complete and green. No kill criterion hit. No ARCHITECTURE §9 OPEN
question resolved** — several got sharper evidence (recorded below) and stay OPEN for the freeze.

Green:

- `mise run check` — green, **unchanged: host-only, Xcode-free, browser-free**. 13 test binaries;
  `profile-web` adds **29 host controller tests + 2 l10n unit tests**.
- `mise run build:web` — green from a clean `dist/`; self-heals the wasm target.
- `mise run test:web` — green: **8 headless wasm tests** in Chrome. **No GUI session required.**
- `mise run serve:web` — serves the app (HTTP 200, wasm delivered); the manual protocol ran against it.
- `git diff` vs step-03's HEAD on `crates/bolted-core` and `crates/spike-profile`: **empty**. The core
  is frozen, as the step doc demanded.

## What was built

### Deliverable A — wasm toolchain + mise wiring

- **Trunk** (`0.21.14`) is the bundler and **wasm-pack** (`0.15.0`) the headless test runner, both
  pinned in `mise.toml` via the **github backend**. *(Trap worth recording: mise's registry
  shortname `trunk` resolves to `npm:@trunkio/launcher` — trunk.io's code-quality tool, not the
  Rust bundler. Pin `github:trunk-rs/trunk`.)*
- `build:web` / `serve:web` / `test:web`, each with a doctor-fail guard (missing trunk / wasm-pack /
  Chrome) and a `rustup target add wasm32-unknown-unknown` self-heal, mirroring `pack:apple`.
  **Nothing was folded into `check`.**
- `crates/profile-web/dist/` gitignored.
- **Smallest-reversible call (recorded per the step doc):** a wasm-core build (`cargo build --target
  wasm32-unknown-unknown -p bolted-core -p spike-profile`) was **not** added to `check` — a bare box
  need not have the wasm target, and `check` is the "works everywhere" verb. The discipline gate's
  durable home is `bolted-check` (Phase 4). It is re-confirmed by hand below and by `build:web`.

### Deliverable B — `crates/profile-web`

A new workspace member. The wasm-only dependencies (`leptos` csr, `wasm-bindgen-futures`,
`gloo-timers`, `console_error_panic_hook`) are **target-gated to `wasm32`**, so `cargo
clippy/test --workspace` on the host never compiles Leptos. Three modules:

- **`controller.rs`** — a framework-light `ProfileController` over `ProfileStore` + `ProfileHandle`
  (the analog of step-03's `ProfileViewModel`). Buffers + focus/blur (the echo rule), per-keystroke
  `try_set_*`, debounce tickets, the single-flight drive, conflict resolution, submit, and the
  server simulator. Host-testable, no browser, no Leptos.
- **`l10n.rs`** — `ErrorData.key → English template`, `{param}` filled from core-supplied params.
- **`app.rs`** — the Leptos CSR view layer: the full form, conflict banners, submit report, orphan
  banner, and the server-simulator pane.

**No constraint literal in the shell.** `grep -nE '\b(20|30)\b' crates/profile-web/src/*.rs` returns
only two hits, both inside an `l10n` **unit test** that constructs a fixture `ErrorData` — zero hits
in shell code. Counters, required markers and the *absence* of a counter on `Email` (which declares
no `LenChars`) all derive from `ProfileField::constraints()`.

### Deliverable C — two test tiers

- **Host (`cargo test`, in `check`): 29 controller tests.** Echo rule (focused buffer never
  rewritten, `Invalid.raw` preserved, blur refresh, email lowercasing), live rebase (adopt /
  conflict / convergent / orphan / **focused-clean-stale-until-blur**), conflict resolution
  (keep-mine, take-theirs, **I13** check reset via `username_check_state()`, **F6**), the async check
  (debounce collapse, `Idle`↔`Pending`↔`Done`, stale-verdict discard, taken verdict), and submit
  (validation report, tier-2 rule, **conflicted → F3 → resolve → resubmit**, success + re-checkout,
  orphaned, **F2**, pending-blocks-submit).
- **Headless wasm (`test:web`): 8 tests** driving real DOM events into the real render tree.

## The headline results

### 1. wasm32 discipline holds — structurally, not by luck

```
$ cargo build --target wasm32-unknown-unknown -p bolted-core -p spike-profile
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s
$ cargo tree --target wasm32-unknown-unknown -p bolted-core -e normal
bolted-core v0.0.0
$ cargo tree --target wasm32-unknown-unknown -p spike-profile -e normal
spike-profile v0.0.0
└── bolted-core v0.0.0
```

Zero dependencies, no shim, no `tokio`, **after** the app layer exists. The shell's own deps
(`leptos`, `wasm-bindgen-futures`, `gloo-timers`) sit strictly above the core and leak nothing into
it. §5's "sans-io → wasm32 is structural" survives its first real test. **No kill.**

### 2. `bolted_core::Store` served a reactive shell — with one ergonomic scar

This is `Store`'s **first real consumer** (the FFI wrapper re-owned the loop and bypassed it). Its
public surface — `checkout` / `submit` / `apply_canonical` / `delete_canonical` + reading through the
`Draft` trait — was **sufficient**. `Rc<RefCell>` fits a single-threaded browser natively, exactly as
predicted. Step-03's fix A1 (`submit` returns the handle on refusal) was **exercised for the first
time against the real `Store`** and works: `submit_conflicted_is_refused_and_leaves_the_draft_alive`
resolves and resubmits on the same draft.

The scar is `submit(self, handle)` consuming a **`!Clone`** handle that lives in a struct field:

```rust
// The slot must be vacated with *something* — the handle cannot be moved out from behind &mut self.
let scratch = self.store.checkout();
let submitted = std::mem::replace(&mut self.handle, scratch);
match self.store.submit(submitted) { … }
```

The alternatives are worse: `Option<ProfileHandle>` introduces a `None` that is unreachable yet must
be handled at **every** read (or `expect`, forbidden in library code). A scratch checkout is a real
allocation and a real registration in the store's rebase list, thrown away moments later.

**No change-notification hook was needed** — see (3). But note the asymmetry the freeze should weigh:
`Store` is `!Send` by construction (`Rc`), which is *right* for the browser and *wrong* for the FFI
path, which is why step 02 bypassed it. **One `Store` cannot serve both** without a concurrency-model
decision. That is §9's store-concurrency question, now with evidence from both sides.

### 3. The reactivity pattern: read-direct + version tick. It is clean.

`Store`/`DraftHandle` are plain, non-reactive Rust; `apply_canonical` mutates the draft *underneath*
the shell. The shell keeps one `RwSignal<u64>`:

```rust
fn read<T: Default>(self, f: impl FnOnce(&ProfileController) -> T) -> T {
    self.version.get();                              // subscribe to the tick
    self.ctrl.try_with_value(f).unwrap_or_default()  // then read the LIVE draft
}
fn write<T: Default>(self, f: impl FnOnce(&mut ProfileController) -> T) -> T {
    let out = self.ctrl.try_update_value(f).unwrap_or_default();
    self.version.update(|v| *v = v.wrapping_add(1)); // …and tick
    out
}
```

Every view goes through `read`; every mutation through `write`. **No draft state is copied into
signals anywhere in this shell** — so nothing forks the logic §4 forbids. The observe-over-a-mutating-
core-draft kill criterion is **not** hit, and it isn't close.

**The freeze finding:** a **Rust shell does not need the snapshot stream** (§4). It reads the contract
directly and drives reactivity from an explicit tick. What it costs is exactly one discipline rule —
*every mutation goes through `write`* — which is enforced by making `write` the only way to get `&mut`
at the controller. Compare Swift, which needed a stream, a `version` stamp, a subscribe-first
ordering dance, and a reconcile function to close the get-then-subscribe race. **The Rust shell has
no race to close**: reads are synchronous against the same memory the store mutates.

Two honest caveats:

- **Coarse invalidation.** One tick invalidates every derived view. Leptos's `Memo` (PartialEq) stops
  the DOM writes, so this is cheap here (see the measurement), but it is *not* fine-grained
  reactivity — the shell is diffing, not subscribing. A generated shell could emit per-field ticks;
  nothing in the core would have to change.
- **`!Clone` + `!Send` handle vs. `'static + Copy` closures.** `StoredValue<_, LocalStorage>` is the
  parking spot. It works and is one line, but it is unavoidable gymnastics, and it means the handle's
  lifetime is now the arena's, not the shell's — relevant to §9's draft-lifecycle question (below).

### 4. The sans-io async check, driven from the browser

```rust
wasm_bindgen_futures::spawn_local(async move {
    TimeoutFuture::new(self.timing.debounce_ms).await;
    let Some((token, name)) = self.write(|c| c.fire_check_if_current(ticket)) else { return };
    TimeoutFuture::new(self.timing.check_latency_ms).await;   // simulated server
    self.write(move |c| c.complete_check(token, simulated_lookup(&name)));
});
```

The core produced a `CheckToken` (data) and nothing else. **The executor is the shell's.** Typing
through a pending check resets the verdict (I13) and the late completion is discarded by sequence
(I10) — the spinner behaviour **falls out of the contract**; the shell does no bookkeeping for it.
Proven both host-side and end-to-end in a headless browser
(`typing_through_a_pending_check_never_shows_a_verdict_for_the_wrong_text`). **No kill.** This is the
sharpest available demonstration of §5's central claim.

### 5. WASM bundle size — the baseline (no threshold)

Release, `wasm-opt -Oz` (Trunk), `wasm-bindgen 0.2.126`, `leptos 0.8.20`:

| Artifact | raw | gzip -9 | brotli -q11 |
|---|---:|---:|---:|
| `profile-web_bg.wasm` | **311 610 B** (304 KiB) | 112 071 B | **87 437 B** (85 KiB) |
| `profile-web.js` (glue) | 30 326 B | 6 186 B | 5 353 B |
| **total wire** | 341 936 B | 118 257 B | **92 790 B** |

`twiggy top` on the post-`wasm-opt` module is useless (names stripped), and on the pre-opt 1.8 MB
module it is misleading (no DCE yet). So the breakdown was measured **differentially** instead: a
bare Leptos CSR hello-world (same leptos version, same `wasm-opt -Oz`, a signal and a button) built
with the same toolchain:

| | raw wasm | brotli |
|---|---:|---:|
| Leptos CSR floor (hello-world) | 102 438 B | 32 524 B |
| Bolted profile app | 311 610 B | 87 437 B |
| **the whole feature costs** | **+209 172 B** | **+54 913 B** |

That delta is everything Bolted-shaped: `bolted-core` + `spike-profile` + the controller + the views
+ `l10n` + `gloo-timers`/`wasm-bindgen-futures` + the `core::fmt` machinery that `format!` drags in.
**What dominates is the Leptos/wasm-bindgen floor plus formatting, not the core's semantics** —
`bolted-core` is ~450 lines of `enum` and `match` with no allocation-heavy machinery. For the future
`bolted-check` size budget the useful shape is *floor + per-feature delta*, not one absolute number.

Cold `mise run build:web` (empty `dist/`, warm cargo cache): **28.1 s** wall.

### 6. Per-keystroke latency: not a concern in the browser

Measured in headless Chrome on the `name` field (no debounce path), driving real `input` events:

- **Synchronous handler** (`input` → `try_set` → tick): mean **22 µs**, p95 **100 µs** (Chrome clamps
  `performance.now()` to 0.1 ms, so most samples read 0), max 200 µs over 300 samples.
- **Full path to a repainted DOM** (`input` → `try_set` → tick → Leptos flush → the
  constraint-derived counter's text mutates, observed with a `MutationObserver`, no polling):
  p50 **0.2 ms**, p90 0.2 ms, max **0.6 ms** over 30 samples.

Two orders of magnitude under a 16 ms frame. **No jank, no batching needed.** As step 03 said of
Swift: this carries **no** evidence for JNI (step 05 is still the worst case).

### 7. Line counts (comments/blanks excluded)

| | Rust web shell | Swift shell |
|---|---:|---:|
| shell source | **779** (`app` 341 · `controller` 389 · `l10n` 40 · glue 9) | **658** |
| headless semantic tests | 369 (controller) | 207 (VM) |
| real-event UI tests | 219 (**wasm, headless**) | 145 (XCUITest, **GUI-gated**) |

The Rust shell is ~18 % *larger* than the Swift one for the same feature, and that is the honest,
slightly surprising result. It is not framework verbosity; it is that **the Swift shell got a DTO
layer for free from the FFI generator** (`ProfileSnapshot` with pre-projected `validity`/`sync`/
`dirty` per field), while the Rust shell must write those projections itself against generic
`Field<V>`. The monomorphization tax (step-03 friction #2) doesn't disappear on the zero-FFI path —
**it just moves from the generated binding into the hand-written shell.** For the eventual Leptos
generator (Phase 3+) that is the concrete sizing input: it must emit the per-field projection
helpers, ~200 lines for four fields.

## The four behaviours, compared to the Swift shell

| Behaviour | Rust-web verdict | vs. Swift |
|---|---|---|
| **Echo rule** | **Holds.** A `Memo` over the buffer feeds `prop:value`; the controller never rewrites the *focused* field's buffer, so the memo never changes while typing and no write reaches the DOM `value` that could move the caret. Verified by hand in a browser: `  bob_1  ` typed fast keeps every space and the caret; **mid-string insertion** (caret to index 4, insert `Z` then `Q`) leaves the caret at 5 then 6 — no jump; `Foo@BAR.com` lowercases **only on blur**. `try_set` still runs every keystroke. | **Same invariant, a cleaner mechanism.** Swift needed a `Binding` setter that fires `onEdit` only on user input. Leptos's `Memo` (PartialEq) achieves it structurally: an unchanged buffer produces no DOM write at all. **No kill:** the shell restates no constraint and re-implements no sanitization. |
| **Live rebase** | **Holds.** Clean+unfocused adopts silently (`InSync`); dirty conflicts with mine preserved and full `{base, theirs}` banner data. Reactivity came from the manual tick — no stream. | Same semantics. Swift needed a snapshot stream + a version-stamped reconcile to close a subscribe race; the Rust shell has **no race** (synchronous reads of the same memory). |
| **Conflict UI** | **Holds.** keep-mine / take-theirs from `Field` data alone; take-theirs on username also resets the check (I13, visible via `username_check_state()`). | Identical. |
| **Submit flow** | **Holds, honest.** Validation report / `Conflicted{fields}` / `Orphaned` / success-via-`store.canonical()`. **F3 proven on the real `bolted_core::Store::submit`** for the first time. | Swift proved F3 through the FFI wrapper's re-owned loop; this proves the store itself. The re-checkout hand-off is *worse* here (the scratch-checkout dance, above). |

## Friction log (findings for the freeze)

1. **`Store::submit(handle)` cannot be called on a handle that lives in a struct field.** The
   `!Clone` handle + by-value `submit` forces either a throwaway `checkout()` to vacate the slot, or
   an `Option` whose `None` is unreachable but must be handled everywhere. Every real shell will hit
   this, because every real shell stores its handle. **This is the same wound as step-03 friction #1
   from the other side:** there, `commit(self)` could not hand the draft back on failure; here,
   `submit(handle)` cannot take the handle without the caller dismembering itself. A `submit(&mut
   self, …)`-shaped store API — or `Draft::commit(self) -> Result<Entity, (Self, Report)>` — would
   close both. **Structural; left for the design session.**

2. **The monomorphization tax moves, it does not vanish.** Zero FFI means zero generated DTOs, which
   means the shell hand-writes `display(&Field<Username>)`, `display(&Field<PersonName>)`,
   `display(&Field<Email>)`, `conflict_info(&SyncState<V>)`, `date_bufs(&Field<DateRange>)` … Rust
   generics recover *some* of it (`fn display<V: Value<Raw = String>>` collapses three of them into
   one, which Swift could not do), but the per-field dispatch (`match field { Username => …, Name =>
   … }`) reappears verbatim in **ten** `match field` sites (six in the controller, four in the
   views). **Sizing input for the Leptos generator.**

3. **`Value::Error: Into<ErrorData>` again.** The shell's `invalid_error` helper needs exactly the
   bound step-01's Q2 proposed (`V::Error: Into<ErrorData>`), and once again the bridge lives outside
   the `Value` trait so the helper must restate it. **Second independent vote** for promoting it into
   `Value`.

4. **Leptos flushes DOM writes one tick after the mutation.** Core state is correct *synchronously*;
   only the paint is deferred (a microtask). This is invisible to users (0.2 ms) but it means **every
   DOM assertion in the wasm tier must yield first**. Recorded because a generated shell's conformance
   tests will trip on it.

5. **A test that clears `<body>` to re-mount kills `wasm-bindgen-test`.** The harness keeps its own
   output node in the body; wiping it makes the runner report "failed to detect test as having been
   run" and time out after 20 s with no useful error. The fix (`mount_into(container)`, one extra
   public entry point) is worth emitting from the generator, since every generated Leptos shell will
   want a per-test mount.

6. **mise's `trunk` shortname is the wrong tool** (trunk.io's launcher, not the Rust bundler). Pin
   `github:trunk-rs/trunk`. Cheap trap, will bite the scaffolder (`bolted new`) in Phase 4.

7. **An orphaned draft has no recovery path in this shell.** After `delete_canonical`, every setter is
   inert; `sim_reset_to_seed` restores canonical but the draft stays `Orphaned` (rebase skips it, per
   the core). The shell shows a banner and asks for a reload. That is *correct* — "submit on orphaned
   is a typed outcome the app decides" (§4) — but it means **the app must implement re-checkout or
   convert-to-create itself**, and both spike shells punted. Worth a battery, or at least a documented
   pattern.

## ARCHITECTURE §9 evidence (recorded, NOT decided)

- **Observe verb for Rust shells.** Answered with evidence: `snapshots()` is **not needed**. Read-direct
  + an explicit tick is clean, race-free, and forks nothing. §4's "drafts expose their own snapshot
  stream" should be re-scoped as *an FFI-boundary mechanism*, not a universal contract member. The
  cost of "Rust shells consume the contract directly — no codegen" (§1) is: one tick discipline, plus
  hand-written per-field projections (friction 2).

- **Store concurrency model.** Both sides now have evidence. The browser wants `Rc<RefCell>` and gets
  it natively (`Store` used as-is, unmodified). The FFI wants `Send + Sync` and therefore *bypassed*
  `Store` entirely (step 02). A single `Store` type cannot serve both; the extraction must pick a
  parameterised handle (`Rc` vs `Arc`) or ship two.

- **Draft lifecycle / `close()`.** In Rust there is **no lifecycle problem**: discard = drop, and
  `submit` consumes the handle. The web shell needs no `close()`. But `StoredValue` moved the handle's
  lifetime into Leptos's arena, so "drop" now means "dispose the arena slot". Weak evidence that an
  explicit `close()` for symmetry (the likely GC-language outcome) would **not** hurt the Rust shell —
  it would be a no-op it can ignore.

- **F2 — commit policy for a never-run check.** Confirmed again, and *worse* here.
  `f2_a_never_checked_username_submits_successfully` sets the username to `admin` (which the checker
  rejects), never triggers a check, and **submits successfully**. In the running app the default path
  is exactly this: any submit within the 400 ms debounce commits client-side unverified. Two shells,
  same finding. The freeze must decide.

- **F6 — a conflicted field edited to equal *theirs* stays `Conflicted`.** Mechanically confirmed
  (again), and this time with a **UX verdict from the running app**: the state is *actively confusing*.
  The banner reads "Server changed this field — theirs: `Server Name` (was `Alice Smith`)" while the
  input **also** reads `Server Name`, the dirty dot is lit (correct: the value differs from the *old*
  base), and two buttons — "Keep mine" and "Take theirs" — now do the same visible thing. A user
  cannot tell what is being asked. **Recommendation for the freeze: auto-converge** (`try_set` that
  lands on `theirs` clears the conflict, as convergent rebase already does — invariant I4 makes the
  identical judgement when the *rebase* arrives second, so the current asymmetry is hard to defend).
  Not resolved here.

- **Focused-but-untouched field during rebase.** New, sharper observation. With fine-grained signals
  the staleness is *more* visible than in Swift, not less: the field's input **and** its
  constraint-derived counter stay stale, no dirty dot, no conflict marker — while the canonical pane
  next to it, reading the same store, **updates immediately**. The user sees the server pane say
  `Server Name` and the focused field say `Alice Smith`, with nothing to explain the difference.
  Blur repaints. Fine-grained reactivity did **not** change the feel; it made the divergence easier
  to notice, because two views over the same store visibly disagree. The freeze should decide whether
  a focused *clean* field should adopt live (it can: nothing the user typed would be lost).

## Deviations from the step doc

- **Milestones 2–4 landed as one commit.** The controller subsumes all three (the step doc's split is
  by behaviour; the code's is by layer), and the host tests for 2–4 were written together. M1 and M5a
  (the wasm tier) are separate commits, as the doc invited.
- **`twiggy` breakdown is differential, not symbolic.** `twiggy top` gives no usable attribution on a
  `wasm-opt`-stripped module. The hello-world-baseline diff answers "what dominates" better; the raw
  twiggy buckets are recorded above as unreliable and were discarded.
- **`main.rs` compiles to an empty `main()` on the host** so the bin target does not drag Leptos into
  `check`. Trunk needs a bin; `check` must not build it for real.
- **`mount_into` was added** (friction 5) — a second public entry point the step doc did not
  anticipate. Smallest reversible choice; recorded.

## Kill criteria

**None hit.**

- *wasm32 discipline* — core reaches `wasm32-unknown-unknown` with zero deps, after the app layer
  exists. The app's own deps leak nothing downward.
- *Observe over a mutating core-draft* — read-direct + tick re-renders on rebase with **no draft state
  copied into signals**. Not close to the kill.
- *Async needs an executor in the core* — `spawn_local` is the shell's; the core only produced a
  `CheckToken`. Not close to the kill.
- *Echo rule* — the caret survives mid-string insertion under per-keystroke trim/lowercase
  sanitization; the shell restates no constraint and re-implements no sanitization.

*Not a kill, per the doc, and observed anyway:* the 304 KiB `.wasm` (baseline, no threshold);
`StoredValue` gymnastics for the `!Clone` handle; no per-keystroke jank whatsoever.

## Open questions handed to the freeze

- **Friction 1 (structural):** should the store's `submit` take `&mut self`-style access, or should
  `Draft::commit` return `Result<Entity, (Self, Report)>`? One change closes both this step's scratch-
  checkout wound and step-03's dead-branch wrinkle.
- **F6:** auto-converge on edit-to-equal-theirs? The running app says the current behaviour is
  confusing, and I4 already auto-converges the symmetric case.
- **F2:** still the default path, now on two shells. Decide the commit policy for a never-run check.
- **§4's snapshot stream:** demote to an FFI-boundary mechanism? A Rust shell demonstrably does not
  want it.
- **Store concurrency:** `Rc` for Rust shells vs `Arc + Send + Sync` for FFI — parameterise or ship two.
- **`Value::Error: Into<ErrorData>`** (step-01 Q2): second independent vote to promote it into the trait.
- **Focused clean field during rebase:** adopt live, or keep stale-until-blur? Two views over the same
  store now visibly disagree.

## If you want to see it

`mise run serve:web` opens the app (Trunk dev server, hot reload). The simulator pane on the right
drives every interesting state: push a canonical change while a field is clean (silent adopt) or dirty
(conflict), delete the profile (orphan), and type `admin`, `taken` or `root` into Username to see the
1 s uniqueness check fail. Type fast with leading spaces to feel the echo rule.
