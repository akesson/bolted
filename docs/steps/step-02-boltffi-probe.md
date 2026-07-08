# Step 02 — BoltFFI due-diligence probe (Apple)

**Phase 1 · Spike.** Read first: [VISION.md](../VISION.md) (bet 1: BoltFFI is the boundary;
risk 1: BoltFFI is young), [ARCHITECTURE.md](../ARCHITECTURE.md) (§4 drafts as core-side
handles, §5 generics/macros/traits + crate layout, §9 OPEN questions this probe feeds),
[ROADMAP.md](../ROADMAP.md) (working agreement), and the step-01 handoff
([plan](step-01-core-semantics.md) · [report](step-01-report.md)) — this step exports the
feature step 01 built.

## Goal

**Falsify-or-confirm the four BoltFFI features the whole architecture rests on**, by exporting
the *real* step-01 `spike-profile` feature through BoltFFI and driving it from a Swift test
target:

1. **Classes with methods** — draft/store handles (ARCHITECTURE §4: "shells hold handles").
2. **Async streams** — the `observe` verb / snapshot delivery (§1, §4).
3. **`Result` methods with typed error enums** — `submit`/`commit` carrying a structured
   report (§2 errors-as-data, §4 commit).
4. **Callback traits** — capabilities implemented on the foreign side (the async
   username-uniqueness check; §2 async validation).

This is VISION risk #1 made concrete. The output that matters is **evidence**: every
restructuring the FFI wrapper is forced into, every awkward translation, every behavior that
differs from what §4/§5 assume, recorded in `docs/steps/step-02-report.md`. A green test suite
is necessary but not the deliverable — the friction log and the answered probe matrix are.

**Load-bearing principle:** *every restructuring the wrapper is forced into is the probe's most
valuable output.* Do not "helpfully" patch `bolted-core` or `spike-profile` to make the wrapper
prettier — record what you had to work around instead. (See Non-goals.)

## Non-goals (hard boundaries)

- **No changes to `bolted-core` or `spike-profile`.** They are the frozen subject of the
  experiment. The wrapper adapts to them; if it can't, that is the finding. (The one exception:
  if a crate genuinely *cannot* be wrapped without a trait/visibility change in core, **stop and
  record it as a structural open question** — do not resolve it here. Per CLAUDE.md, structural
  changes are a design session's call.)
- **The two core fixes decided after step 01 are NOT in scope here.** Value-bound async-verdict
  reset (invariant 13) and failed-`submit`-returns-the-handle (ARCHITECTURE §8) are **scheduled
  for step 03**. Wrap the current behavior as-is; if the FFI layer changes how F3 should be
  resolved, note it (see Probe matrix → handles), don't fix it.
- No UI (no SwiftUI, no views — that is step 03). No Kotlin/Android (step 05 re-measures
  everything; **Apple numbers carry zero evidence for the JNI bet**). No macros (still
  hand-written as-if-generated). No performance optimization. No published crates.

## Prerequisites & the BoltFFI facts you'll need

This is the first step that leaves the Rust sandbox, so it depends on host tooling `mise` cannot
fully pin (Xcode, the Apple SDKs — VISION risk 5). Treat a missing toolchain as a **doctor
failure**, not a code failure: record the versions you ran against.

Baked-in facts (from boltffi.dev — you should not need to re-research these; confirm them
against the version you install and record any drift):

- **Setup**: `cargo install boltffi_cli` (pin it — see mise wiring); `cargo add boltffi` in the
  wrapper crate; `[lib] crate-type = ["staticlib", "cdylib"]`; `boltffi init` writes
  `boltffi.toml`; `boltffi pack apple --release` produces `dist/apple/` = an `.xcframework` +
  `Package.swift` + a generated Swift module. Default slices are iOS device + simulator;
  **set `include_macos = true`** so a plain `swift test` runs on the Mac (the CLI test path).
- **Classes**: `#[export]` goes on the **impl block**; methods take **`&self` only** — no
  `&mut self`, no consuming `self` — because foreign threads may call concurrently. State
  changes require **interior mutability** (`Mutex`/`RwLock`). Methods may accept and return
  other exported class instances. The Rust object is dropped when the foreign reference
  deallocates — **there is no manual `close()`**.
- **Data** (`#[data]`): structs with named fields and enums with payloads. **No generics, no
  tuples, no borrowed data, no trait objects, no `HashSet`** — everything monomorphic and
  owned. (This is why `Field<V>` cannot cross as-is; see Deliverable 1.)
- **Errors** (`#[error]`): enum variants with named fields become Swift enum cases with
  associated values; a `Result`-returning export becomes a Swift `throws` function.
- **Streams** (`#[ffi_stream(item = T)]`): a method returns `Arc<EventSubscription<T>>`; the
  producer side is `StreamProducer<T>`; Swift gets `AsyncStream<T>`. **No tokio required**
  (sans-io compatible — good, this preserves the §5 sans-io core bet). The subscription is a
  **bounded ring buffer (default 256) that drops the NEWEST events when full and never blocks
  the producer.** That drop-newest policy is hostile to latest-wins snapshot semantics — it is a
  first-class probe target, see Kill criteria.
- **Callbacks**: `#[export]` on a plain trait generates a Swift protocol the app implements and
  passes into Rust; async trait methods are possible; **which thread callbacks arrive on is
  undocumented** — measure and record it.

## Deliverables

### 1. `crates/spike-profile-ffi` — the wrapper crate (the only crate importing boltffi)

New workspace member. This is the hand-written stand-in for the future `bolted-ffi` crate
(ARCHITECTURE §5 crate layout) — written as-if-generated, as plainly as possible. It imports
`bolted-core` and `spike-profile` and `boltffi`; **`bolted-core` still never sees boltffi.**

**1a. DTOs (`#[data]`).** `Field<V>` is generic, so it cannot cross. Hand-write the monomorphic
projection the macros would emit:

- `ProfileSnapshot` — the always-valid observable state plus per-field display state.
- **One field-state enum per value type** (no generics): e.g. `UsernameFieldState`,
  `PersonNameFieldState`, `EmailFieldState`, `DateRangeFieldState`, each carrying its own
  `Invalid { raw, error }` shape (the `raw` type differs per value: `String` vs the date pair).
  **Count the hand-written DTO lines per field and report the total** — this number is the
  honest cost of the "drafts core-side / snapshot-per-change" decisions (§8) and sizes the
  step-09/10 codegen.
- `ProfileFieldId` (mirrors `spike_profile::ProfileField`).
- `ErrorData` as a `#[data]` record — the core's `Vec<(&'static str, String)>` params use tuples
  and `&'static str`, neither of which crosses; project to `Vec<Param>` with
  `Param { key: String, value: String }`.
- `ValidationReport` DTO (field errors + rule violations, each carrying `ErrorData`).
- **`PlainDate`, not `Date`.** A `#[data] Date` lands in the same Swift module as
  `Foundation.Date` and shadows it — rename in the DTO layer and **record the general finding:
  bolted-ffi needs a platform-stdlib name-collision policy.** Confirm `(Date, Date)` raw becomes
  two arguments or a record on the setter — **never a tuple**.

**1b. Exported classes (`#[export]` impl blocks) — store + draft, with interior mutability.**
The step-01 `Store`/`DraftHandle` use `Rc<RefCell<…>>`/`Weak` (see `crates/bolted-core/src/store.rs`)
and are **not `Send`** — a `Mutex` around them won't compile (`Mutex<T>: Send` needs `T: Send`,
and `Rc` isn't). But `spike_profile::ProfileDraft` itself is plain owned data (no `Rc` — see
`crates/spike-profile/src/profile.rs`). So the prescription is:

> The wrapper **re-owns the store loop.** Hold `HashMap<DraftId, ProfileDraft>` + `canonical:
> Option<Profile>` + `version: u64` behind **one `Mutex`**, and re-implement checkout
> registration, `apply_canonical` fan-out (rebase every live draft), and submit **directly
> against `bolted-core`'s `Field`/`Draft`**, bypassing `Store` entirely.

**Report how much of `store.rs`'s logic had to be re-owned to get a `Send` store** — that is the
named evidence for ARCHITECTURE §9's "store concurrency model behind FFI" question. Wrapper
design rule, stated so the implementer internalizes it: **never emit a stream event or invoke a
foreign callback while holding the `Mutex`** (snapshot the data, drop the lock, then emit) — the
reentrancy tests below exist to punish violations of this rule.

Exported surface (illustrative — adjust names, record deviations):

```
ProfileStoreFfi::new(initial: Option<ProfileSnapshot>) -> ProfileStoreFfi
  .checkout() -> ProfileDraftFfi
  .snapshots() -> Arc<EventSubscription<ProfileSnapshot>>   // #[ffi_stream]
  .apply_canonical(next: ProfileSnapshot)                    // simulates a background change
  .submit(draft: ProfileDraftFfi) -> Result<(), SubmitErrorFfi>
  .live_draft_count() -> u32                                 // for the deinit probe
ProfileDraftFfi
  .try_set_username(raw: String) -> Result<(), UsernameErrorFfi>   // + name/email
  .try_set_availability(start: PlainDate, end: PlainDate) -> Result<(), DateRangeErrorFfi>
  .set_uniqueness_checker(checker: Arc<dyn UniquenessChecker>)     // capability, 1d
  .snapshots() -> Arc<EventSubscription<ProfileSnapshot>>          // draft's own stream (§4)
  .validate() -> ValidationReport
  .resolve_keep_mine(field: ProfileFieldId) / .resolve_take_theirs(field: ProfileFieldId)
```

**1c. Snapshot stream (`#[ffi_stream(item = ProfileSnapshot)]`).** Both the store and the draft
expose one (a draft is a mini feature-model, §4). Emit a fresh snapshot on every mutation. The
overflow, initial-value, and threading behavior of this stream is the bulk of the Streams probe.

**1d. Username-uniqueness capability as an `#[export]` callback trait.** A `UniquenessChecker`
trait implemented in Swift, wired to the draft's `begin_username_check` /
`complete_username_check` (`crates/spike-profile/src/profile.rs`). Start with a **synchronous**
trait method (matches the deterministic begin/complete design). **Stretch:** add one **async**
trait method and see whether it forces an executor on the Rust side (a collision with the §5
sans-io bet would be a significant finding).

**1e. Typed submit error (`#[error]`).** `SubmitErrorFfi` mirroring the core's
`SubmitError<FieldId>` — critically, the `Validation(ValidationReport)` variant must carry the
**structured report payload**, to prove error variants survive as typed cases with associated
data, not flattened messages.

### 2. `apple/profile-probe/` — a SwiftPM test package

A `swift test`-runnable package (XCTest only, no app target) depending on the generated local
package at `crates/spike-profile-ffi/dist/apple`. Contains the probe-matrix tests and the
`measure {}` benchmarks below. Lives under `apple/` (not `crates/`) to keep it out of the cargo
workspace.

### 3. mise wiring

- **Pin `boltffi_cli`** via mise's cargo backend (record the exact version in the report).
- `[tasks."pack:apple"]` → `boltffi pack apple --release` in the wrapper crate.
- `[tasks."test:apple"]` → runs `pack:apple` first (via `depends`), then `swift test` in
  `apple/profile-probe`. If Xcode/SDK is absent, fail with a clear doctor-style message.
- **`mise run check` stays Xcode-free**: the new crate joins the cargo workspace and is covered
  by the existing `fmt`/`clippy -D warnings`/`test` — but Swift packaging is a *separate* verb,
  so a Linux/CI box without Xcode still runs `check` green. Do **not** fold `pack:apple` into
  `check`.
- **`dist/` is a gitignored build artifact** — add `crates/spike-profile-ffi/dist/` (and Swift's
  `.build/`) to `.gitignore`.

## Ordered milestones (walking skeleton first — the toolchain is the riskiest part)

The Rust semantics are already proven; the risk here is the *pipeline* (`cargo install` →
annotate → `pack apple` → XCFramework → local SwiftPM dep → `swift test` on a macOS slice). Order
the work so a packaging quagmire still yields a partial verdict:

1. **Skeleton**: one trivial exported ping method crossing into Swift, `mise run test:apple`
   green end-to-end. *Prove the pipeline before writing any real wrapper code.*
2. **The four features** (Deliverables 1a–1e) with the probe-matrix tests below.
3. **Lifecycle test + benchmarks.**
4. **Stretch (droppable, record if cut)**: the async callback method (1d), the padded-snapshot
   benchmark.

## Probe matrix (each row ⇒ ≥1 XCTest; record *observed* behavior, don't just assert success)

**Feature 1 — Classes / handles**
- Methods on a returned draft object work (basic crossing).
- **Handle round-trip identity**: pass the draft back into `submit(draft)` — does Rust receive
  the *same* instance (id) or a re-wrapped clone? If instances don't survive round-trips,
  ids-not-instances is forced into the public contract (`submit(draft_id)`), which changes the
  §4 handle story — record it. Also record what calling a method on an **already-submitted**
  draft does (tombstone → typed error?). Note: with id-keyed drafts, `submit` never "consumes
  the handle," which *partly dissolves* step-01 F3/Q6 at the FFI layer — **record that
  observation; do not fix core** (the fix is still step 03's).
- **Deinit-deregistration**: create a draft → assert `live_draft_count()` rose → drop the Swift
  reference (nil it) → assert the count falls. Proves whether ARC dropping the handle actually
  runs Rust `Drop` and unregisters the draft. Direct, cheap evidence for the §9 `close()` OPEN
  question — far cheaper on ARC now than discovered on Android GC in step 05. **If the count
  does not fall, drafts leak / `apply_canonical` rebases zombies forever** — a real finding.

**Feature 2 — Async streams (snapshots)**
- End-to-end: a mutation produces a snapshot the Swift consumer receives, with the mutated
  value.
- **Initial-value / subscribe race**: does a fresh subscription replay current state or only
  future events? Can `snapshot()`-then-`subscribe()` miss an event in the gap if another thread
  mutates between the two calls? If there's a gap with no version to detect it, **design the
  version-stamped snapshot pattern now** — bolted-ffi will need the same pattern, and step 03's
  SwiftUI view does exactly get-current-then-subscribe.
- **Overflow / drop-newest**: emit N snapshots fast against a deliberately stalled consumer, then
  let it resume — **can the last-delivered snapshot ever reach the final state?** Probe whether
  capacity is configurable, whether there's coalescing, or a recovery getter. This is the
  kill-bar test (see below), not a footnote.
- **Main-actor consumption**: consume the stream from `@MainActor` while mutating from the main
  actor (step 03 lives on the main actor; XCTest's default background threading would pass
  trivially and hide this). Record the delivery thread.

**Feature 3 — Typed errors**
- A failed `submit` throws a `SubmitErrorFfi` whose `Validation` case carries the structured
  `ValidationReport` (field ids + `ErrorData` params) — assert Swift can read the payload, not
  just a message string. Do the same for a tier-1 field error variant with params
  (`TooLong { max, actual }` → associated values).

**Feature 4 — Callback traits (capabilities)**
- A Swift-implemented `UniquenessChecker` is invoked from Rust and drives begin/complete;
  pending/failed blocks validate, success unblocks (mirror the step-01 behavior tests).
- **Reentrancy / deadlock** (the biggest risk the wrapper's forced interior-mutability creates):
  (i) the Swift checker, when called, synchronously calls a method back into the *same* exported
  object; (ii) a stream consumer, on receiving a snapshot, immediately calls a draft method.
  Both must not deadlock. If they do and the cause is the *wrapper* holding its `Mutex` across
  the outcall, the fix is the "never call out under the lock" rule (friction). **If the cause is
  BoltFFI holding an internal lock across the callback, that is kill-bar territory for feature
  4** — record a minimal repro.

## Measurements (record numbers; **no pass/fail thresholds** — this is a baseline, not a gate)

Apple overhead is *not* evidence for the JNI per-keystroke bet — step 05 re-measures on Android,
which VISION calls the worst case. Record anyway:

- `try_set` round-trip median via XCTest `measure {}` (the per-keystroke `try_set` cost).
- Snapshot end-to-end latency (mutation → Swift receives). **Stretch**: a padded ~30-field (or
  `Vec`-filled) snapshot variant to see whether marshaling scales with payload — the cost side of
  the "snapshot-per-change streams" decision (§8).
- `boltffi pack apple --release` wall-clock + `dist/` artifact size (future CI size-budget
  baseline, per VISION's `bolted-check`).
- The `boltffi` crate version **and** `boltffi_cli` version, plus any skew between them.

## Kill criteria (per ROADMAP: any of the four missing/broken → architecture session first)

"Broken" = **cannot be made to work with reasonable hand-written wrapper code.** Wrapper
awkwardness, extra DTOs, or a design rule you had to adopt = *friction finding*, not a kill.

- **Classes** — methods on returned object instances fundamentally don't work (can't hand back a
  usable draft handle at all).
- **Streams** — a subscriber that stalls then resumes **cannot reach current state by any means**
  (no configurable capacity, no coalescing, no recovery getter). Drop-newest *alone* is not a
  kill if final state is recoverable; unrecoverable staleness breaks the `observe` verb's
  "always-valid state, flows continuously" contract (§1) and **is** a kill.
- **Errors** — payload-carrying variants are flattened to strings (no typed associated data
  reaches Swift).
- **Callbacks** — Swift→Rust trait implementations are unusable, or BoltFFI-internal locking
  makes reentrancy deadlock unavoidable.

**On hitting a kill criterion: stop.** Write a minimal standalone repro (the architecture session
needs it to weigh the exit — VISION bet 1's "narrow seam is the exit") and report. Do not
engineer around it.

## Exit checklist

- [ ] `mise run check` passes (workspace incl. `spike-profile-ffi`; fmt, clippy `-D warnings`,
      cargo tests) — **Xcode-free**, so CI without Apple tooling still goes green.
- [ ] `mise run test:apple` passes from a clean clone on a Mac with Xcode (packs, then
      `swift test`).
- [ ] All four features have probe-matrix tests; every matrix row's *observed behavior* is
      recorded (not just pass/fail).
- [ ] `bolted-core` and `spike-profile` are **unchanged** (`git diff` clean on those crates).
- [ ] `docs/steps/step-02-report.md` written: what was built; the four-feature verdict
      (confirmed / broken, with the deadlock, overflow-recovery, deinit, and handle-identity
      findings called out); the **DTO-line count per field** and **how much store logic was
      re-owned**; deviations; friction log (input to the freeze); open questions (feeding §9);
      measurements; boltffi/CLI versions.
- [ ] ROADMAP.md status updated (02 → done, 03 → ready).

## If you hit a wall

Same smallest-reasonable-choice rule as step 01 (CLAUDE.md): omitted decision → smallest
reversible choice, record it. **Structural** conflict (a trait/visibility change to
`bolted-core`, a new invariant, an ARCHITECTURE change, or one of the four features failing) →
**stop and record the question** for a design session; do not resolve it here. The kill criteria
above are the explicit "stop and report" triggers — hitting one is a *successful* probe outcome,
not a failure of the step.
