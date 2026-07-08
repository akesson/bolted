# Step 02 — BoltFFI due-diligence probe (Apple) · Report

**Status: done. Verdict: all four BoltFFI features CONFIRMED. No kill criterion hit.**

The four features the whole architecture rests on — (1) classes with methods, (2) async streams,
(3) `Result` with typed error enums, (4) callback traits — were exercised against the *real*
step-01 `spike-profile` feature through BoltFFI and driven from Swift XCTests on the macOS slice.
All 23 probe tests pass. `bolted-core` and `spike-profile` are unchanged (`git diff` clean).

## Environment (VISION risk 5 — record the versions)

| Tool | Version |
|------|---------|
| `boltffi` crate | 0.27.3 |
| `boltffi_cli` (`boltffi`) | 0.27.3 (no skew with the crate) |
| rustc / cargo | 1.95.0 |
| Swift | 6.3.3 (swiftlang-6.3.3.1.3) |
| Xcode targets | macOS 26 SDK; xcframework slices ios-arm64, ios-arm64_x86_64-simulator, macos-arm64_x86_64 |

## What was built

- **`crates/spike-profile-ffi`** — the hand-written "as-if-generated" wrapper, the only crate
  importing `boltffi`. `dto.rs` (450 lines) is the monomorphic projection; `lib.rs` (791 lines)
  is the re-owned store + exported classes + capability trait + projections. `bolted-core` never
  sees boltffi.
- **`apple/profile-probe`** — a `swift test`-only SwiftPM package (no app target) depending on the
  generated local package at `crates/spike-profile-ffi/dist/apple`. 23 XCTests across the probe
  matrix + 2 `measure {}` benchmarks.
- **mise wiring** — `setup:boltffi` (pinned install + symlink workaround), `pack:apple`
  (`--release`), `test:apple` (packs then `swift test`). `check` stays Xcode-free. `dist/` and
  Swift `.build/` gitignored.

## Four-feature verdict (observed behaviour, not just pass/fail)

### Feature 1 — Classes / handles: **CONFIRMED**
- Methods on a returned draft object work; interior mutability via one `Mutex` (BoltFFI methods are
  `&self`-only, as documented).
- **Handle round-trip identity is PRESERVED.** `store.same_draft(other: draft)` returns
  `draft.id()`: BoltFFI passes an exported class parameter by forwarding its handle
  (`other.handle`) to the same Rust object. So **ids-not-instances is NOT forced** onto the §4
  contract — a `submit(draft)` shape is viable. (We still chose `draft.submit()` self-submit; that
  was a design choice, not a constraint.)
- **Deinit-deregistration WORKS.** Creating a draft raises `liveDraftCount()`; letting the Swift
  handle leave scope runs ARC `deinit` → the BoltFFI release shim → Rust `Drop` → the wrapper
  prunes the draft from its registry, and the count falls. **Directly relevant to the §9 `close()`
  question: on Apple/ARC, automatic deregistration is real — no manual `close()` needed.** (Step 05
  must confirm the same on Android's non-deterministic GC — this is exactly why the probe was
  cheap to run here first.)
- **Post-submit tombstone:** after `submit()` consumes the core draft, the Swift handle lives on as
  an inert tombstone — `isLive()` is false, mutating calls are silent no-ops, a second `submit()`
  returns `AlreadySubmitted`. Because the handle outlives the draft, `submit` never needs to
  consume the foreign handle — which **partly dissolves step-01 F3/Q6 at the FFI layer** (recorded,
  not fixed; the core fix is still step 03's).

### Feature 2 — Async streams (snapshots): **CONFIRMED** (drop-newest is recoverable → not a kill)
- End-to-end delivery works: a mutation produces a snapshot the Swift `AsyncStream` consumer
  receives with the new value.
- **Two-layer buffering.** BoltFFI exposes each `#[ffi_stream]` as a Swift
  `AsyncStream(bufferingPolicy: .unbounded)` fed by a background poll loop that drains the Rust-side
  bounded, drop-newest ring in batches (batchSize 16). So the Rust ring's drop-newest only bites if
  the poll loop falls behind; once drained, the Swift side buffers unbounded.
- **Overflow / drop-newest is NOT a kill.** Current state is always recoverable two ways: (a) the
  `snapshot()` getter reads current state under the lock regardless of any ring drops
  (`testBurstIsRecoverableViaSnapshotGetter`), and (b) the eager-draining poll loop keeps the ring
  from overflowing under normal load. The `observe` verb's "always-valid current state" holds.
- **Subscribe-race is real and must be designed around.** A fresh subscription replays **nothing** —
  it delivers only future events (`testFreshSubscriptionIsFutureOnly`). A get-current-then-subscribe
  sequence (step 03's SwiftUI view) can miss an event in the gap. Mitigation shipped: every snapshot
  carries a `version` stamp so the caller can reconcile `snapshot()` against the first streamed
  event. **bolted-ffi will need this version-stamped pattern generally.**
- **Main-actor delivery works:** consuming from a `@MainActor` task delivers on the main thread
  (`Thread.isMainThread` true) — step 03 can bind directly.

### Feature 3 — `Result` with typed error enums: **CONFIRMED**
- Setters throw typed enums with associated values Swift reads structurally: `UsernameErrorFfi`
  `.tooShort(min: 3, actual: 2)`, `DateRangeErrorFfi.startAfterEnd(start:end:)`.
- **The nested `ValidationReport` payload survives.** `submit()` throws
  `SubmitErrorFfi.validation(report:)` whose report carries field ids + keyed `ErrorData` with
  params (`too_short` → `{min, actual}`) and tier-2 rule violations with pins — read structurally,
  never a message string (`testReportCarriesFieldErrorParams`, `testTier2RuleViolationInReport`).
- Encoding note: a unit-only `#[error]` enum (`EmailErrorFfi`) crosses as a C-style raw-value Swift
  enum; payloaded ones use a tag+decode wire form. Both are `Error` and catchable by type.

### Feature 4 — Callback traits (capabilities): **CONFIRMED** (no reentrancy deadlock)
- A Swift-implemented `UniquenessChecker` is invoked from Rust and drives the single-flight
  begin/complete: a `.taken` verdict blocks validation (a `username_unique` rule violation), a later
  `.unique` verdict unblocks it (`testCheckerBlocksThenUnblocks`).
- **Reentrancy does not deadlock.** The wrapper's rule — *never hold the `Mutex` across an
  outcall* — is what makes it safe: `run_username_check` takes the checker out of its slot, drops
  all locks, then calls it. A checker that synchronously re-enters the same draft with both a READ
  (`validate()`) and a MUTATION (`trySetName`) completes cleanly and the mutation takes effect
  (`testCheckerReentrancyDoesNotDeadlock`). BoltFFI does **not** hold an internal lock across the
  callback. A stream consumer that reacts to a snapshot by calling draft methods likewise does not
  deadlock (emissions happen outside the lock).

## The two headline measurements the step doc asked for

### DTO line count per field (the honest cost of "drafts core-side / snapshot-per-change")
- Because `#[data]` forbids generics, `Field<V>` was stamped into **one field-state family per
  value type**: a `*Validity` enum + a `*FieldSync` enum + a `*FieldState` struct.
  - String-raw fields (Username, PersonName, Email): **24 lines each** (with attrs/derives).
  - Composite field (Availability / `DateRange`): **29 lines**.
- **3 of the 4 families are structurally identical** (all `String` raw): a codegen dedup-by-raw-type
  opportunity, but a per-value-type macro stamps them out regardless — this is the real generator
  cost, so it is reported as-is.
- Totals: **24 `#[data]` + 5 `#[error]` = 29 hand-written FFI types**; `dto.rs` 450 + `lib.rs` 791 =
  **1,241 Rust lines → 1,663 generated Swift lines** (60 KB). This sizes step-09/10 codegen.

### How much store logic had to be re-owned (evidence for §9 store concurrency)
Step-01's `Store`/`DraftHandle` (`Rc<RefCell>`/`Weak`, `store.rs`, 157 lines) are **not `Send`**, so
a `Mutex` around them will not compile — and BoltFFI classes are shared across foreign threads. But
`ProfileDraft` is plain owned data and IS `Send`, so the wrapper **bypassed `Store` entirely** and
re-implemented its whole loop against `bolted-core`'s `Field`/`Draft`:
- an **id-keyed `HashMap<u64, DraftEntry>`** instead of `Vec<Weak<…>>` (handles are ids, not `Rc`
  clones — which is *why* handle-identity and deinit-deregistration were even testable);
- checkout registration, `apply_canonical` fan-out (rebase every rebasing draft), and the full
  submit path (pre-check → move-out → `commit` → adopt canonical → rebase others);
- a **"never emit / call out under the lock"** discipline (snapshot the data, drop the lock, then
  push) that step-01's single-threaded `Store` never needed.

**Recommendation for §9:** the store concurrency model behind FFI wants (a) `Send` state behind one
lock, (b) **id-keyed handles, not `Rc` clones**, and (c) the emit-outside-lock rule as a first-class
invariant. The step-01 `Store` is not reusable as-is behind FFI; ~all of its 157 lines were
re-owned.

## Measurements (recorded, NOT gated — Apple carries zero evidence for the JNI bet; step 05 re-measures)

- **`try_set` round-trip:** ~**2.4–3.6 µs/call** (`measure` of 1000 calls: 2.39–3.59 ms, encode raw
  → cross → validate → emit snapshot).
- **`snapshot()` readback** (marshals the whole `ProfileSnapshot` DTO): ~**1.9–2.6 µs/call**
  (1.85–2.58 ms per 1000).
- **`boltffi pack apple --release`:** a from-scratch **parallel build of all 5 release slices failed
  once** (transient — `error: build failed for targets [4 of 5]` at 25.8 s; the crate itself builds
  fine per-slice via `cargo build --release --target …`), and **succeeded on retry / with warm
  caches**. Likely resource contention building url/idna/icu in release ×5 concurrently. Warm
  `mise run test:apple` = **11.2 s** end to end. *CI note: give the release pack headroom or bound
  slice parallelism.*
- **Artifact size:** `dist/apple` **127 MB** (xcframework 95 MB across 3 slices, zip 32 MB; one macOS
  `.a` = 38 MB). Inflated by **unstripped release static libs statically linking `url → idna →
  icu`** — the ICU data is the bulk. Baseline for VISION's `bolted-check` size budget; stripping and
  trimming the `url`/icu dependency are obvious levers.

## Findings / friction log (input to the design freeze)

1. **Toolchain, symlinked `CARGO_HOME` (VISION risk 5).** `cargo install boltffi_cli` **fails to
   compile** when `~/.cargo` is a symlink (this machine → a dotfiles repo): `boltffi_bindgen` builds
   through **askama 0.16**, whose template-path resolution mangles the registry path behind the
   symlink (`… /src/render/c/../../../../../../../../Developer/…/dot-files/cargo/… askama.toml: No
   such file`). **Workaround:** install with a canonicalized `CARGO_HOME` (`cd -P`), a no-op on a
   normal setup. **Consequence:** the step doc's "pin `boltffi_cli` via mise's cargo backend"
   prescription does NOT work here (the cargo backend hits the same bug), so pinning is a
   `setup:boltffi` mise task instead, and `boltffi_cli` is deliberately kept OUT of `[tools]` so
   `mise install`/`check` do not try (and fail) to build it. `check` stays Xcode-free and green.
2. **Crate naming — no hyphens.** BoltFFI derives native symbol names from the crate name and
   rejects hyphens (`invalid native symbol name boltffi_function_spike-profile-ffi_ping`). This only
   fires during `pack` (the IR-lowering pass), not plain `cargo build`. **Package renamed to
   `spike_profile_ffi`** (directory kept as `spike-profile-ffi` per the step doc). A general
   bolted-ffi crate-naming rule.
3. **Platform-stdlib name collisions.** A `#[data] Date` lands next to `Foundation.Date` and shadows
   it; renamed to `PlainDate`. **bolted-ffi needs a name-collision policy** for `Date`, `URL`,
   `Data`, `Error`, etc. (the step doc anticipated this).
4. **No `#![forbid(unsafe_code)]` in the FFI crate.** `#[export]` expands to `extern "C"` shims
   containing `unsafe`. The no-unsafe discipline necessarily stops at the FFI boundary; the core and
   `spike-profile` keep it.
5. **`crate-type` needed `rlib`.** `["staticlib", "cdylib"]` alone cannot join the cargo workspace
   for `cargo test`/clippy; added `rlib` (the boltffi demo does the same). Minor deviation from the
   step doc's two-type sketch.
6. **An FFI-only lifecycle error.** `SubmitErrorFfi` needed an `AlreadySubmitted` variant with no
   analogue in core `SubmitError`, because the foreign handle outlives the core draft. The FFI
   boundary needs lifecycle errors the sans-io core does not.
7. **Observability gap in the core draft.** `ProfileDraft.username_check` is private with no getter,
   so the snapshot **cannot** project the async-check sub-state (Idle/Pending/Passed/Failed) — only
   its *effect* is visible via `validate()`. And with a **synchronous** checker, begin+complete are
   atomic inside one call, so a genuine `Pending` state is never observable between FFI calls. **For
   step 03's spinner UI, `bolted-core`'s draft must expose the check sub-state**, and a real pending
   state needs either an async trait method or a split begin/complete API across FFI.
8. **`&'static str` and tuples don't cross** (as expected): `ErrorData.key`/param keys projected to
   `String`; `(Date, Date)` projected to a `PlainDateRange` record and the setter takes two args.

## Open questions (feeding ARCHITECTURE §9)

- **Store concurrency model behind FFI (§9):** evidence now in hand — `Send` state behind one lock,
  id-keyed handles (not `Rc` clones), emit-outside-lock as an invariant. The step-01 `Store` is not
  reusable behind FFI.
- **Draft lifecycle / `close()` (§9):** on Apple/ARC, `deinit` reliably runs Rust `Drop` →
  automatic deregistration; **no manual `close()` needed here.** Open until step 05 confirms the same
  under Android GC (non-deterministic finalization is the risk).
- **Async capability vs sans-io (§5):** the **synchronous** trait method sufficed and kept the core
  sans-io. The async-trait stretch was **cut** (droppable per the step doc); whether an async
  callback forces an executor on the Rust side is **still open** — worth a targeted future probe,
  but no blocker surfaced.
- **Codegen dedup:** field-state families are per-value-type; 3/4 are structurally identical. Whether
  bolted-macros should dedup by raw type is a step-09 question.

## Exit checklist

- [x] `mise run check` — Xcode-free (fmt, clippy `-D warnings`, cargo test), green.
- [x] `mise run test:apple` — packs (release) then `swift test`, **23/23 green** (warm 11.2 s).
- [x] All four features have probe-matrix tests; observed behaviour recorded above.
- [x] `bolted-core` and `spike-profile` unchanged (`git diff` clean on those crates).
- [x] This report written.
- [x] ROADMAP updated (02 → done, 03 → ready).
