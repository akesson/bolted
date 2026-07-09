# Step 05 — Android headless probe (JNI + ART) · Report

**Status: done. No kill criterion hit.** The chattiness bar — the one this step existed to test —
clears with ~80× headroom. All four BoltFFI features are confirmed a second time on a different
codegen backend and a different runtime. 34 instrumented tests run green on a headless ART emulator,
plus 3 isolated hazard probes.

**Two hypotheses were confirmed, and both change the contract:**

- **H1 — on Android, GC never frees a draft.** `close()`/`use {}` is the *only* free path. This is the
  exact inverse of Apple/ARC and it **answers ARCHITECTURE §9's first open question**, which was
  explicitly marked *"pending Step 5"*.
- **H2 — use-after-close is silent undefined behaviour**, not a typed error and not a crash.

`bolted-core`, `spike-profile` and `spike-profile-ffi/src` are unchanged (`git diff` clean). The only
edit under `crates/` is `spike-profile-ffi/boltffi.toml`, which the step doc permits.

## Environment (VISION risk 5 — record the versions)

| Tool | Version |
|------|---------|
| `boltffi` crate / `boltffi_cli` | 0.27.3 / 0.27.3 |
| rustc / cargo | 1.95.0 |
| Android Gradle Plugin | 8.7.3 |
| Gradle | 8.11.1 (pinned by mise; **no wrapper jar committed**) |
| Kotlin | 2.1.0 |
| JDK | Temurin 21.0.11 (pinned by mise; AGP 8.7 rejects Homebrew's default JDK 26) |
| Android NDK | 27.0.12077973 |
| compileSdk / minSdk | 35 / 24 |
| Test device | Gradle Managed Device `dev34` — `aosp_atd`, API 34, **arm64-v8a**, headless |
| Host | Apple Silicon (arm64), macOS |

## ⚠ The measurement caveat, restated (read before any number below)

The emulator is **arm64 guest on an arm64 host**: ART, JNI and the GC are real, but the CPU is not
a phone's. Guest code runs natively on an M-series core, several times faster than the low-end device
VISION names as the worst case.

- A kill criterion that **fires** here is trustworthy — real hardware is only worse.
- A kill criterion that **clears** here means *"not obviously fatal"*, **not** *"confirmed safe"*.
  Clearing must be re-checked on physical hardware in step 07.

One comparison *is* hardware-fair: step 02's Apple numbers were measured on **this same Mac**. So the
Apple-vs-Android ratio below isolates the FFI mechanism (JNI + ART + Kotlin marshaling) from the CPU.

## What was built

- **`crates/spike-profile-ffi/boltffi.toml`** — enabled `[targets.android]`, `architectures = ["arm64"]`
  (one ABI: four would quadruple a release build that statically links `url → idna → icu`, for no
  extra evidence). **No Rust changed.** The same `#[export]` annotations that produced Swift produced
  Kotlin.
- **`android/profile-probe/`** — a `com.android.library` + Kotlin project *outside* the cargo
  workspace, whose `androidTest` source set is the probe matrix (1,086 Kotlin lines, 37 tests). It
  consumes `dist/android/{kotlin,jniLibs}` **in place** as `srcDir`s, so a `pack` drift breaks the
  build loudly rather than compiling against a stale vendored copy. A **Gradle Managed Device**
  provisions, boots and tears down the emulator headlessly.
- **mise wiring** — `pack:android`, `test:android`, `test:android:hazard`. `check` is untouched and
  its task body is byte-identical to step 04's.

### The headless contrast worth banking

Step 03's XCUITest tier needs Xcode, a logged-in GUI session, and Accessibility permission, so it can
never run in headless CI. This step's equivalent-fidelity tier — real ART, real JNI, real GC, real
coroutine dispatchers — runs from a single `mise run test:android` with **no GUI session at all**.
Same coverage class, none of the tax.

## The three ROADMAP questions, answered

### 1. JNI `try_set` at keystroke frequency — **the kill criterion CLEARS (~80× headroom)**

A realistic keystroke is `try_set` + `snapshot()` (the shell writes the character, then repaints from
the returned state — §4's snapshot-per-change). Median **12.1–13.0 µs** across runs, against a
**1.0 ms** bar. Even projecting a 10× slower low-end phone, that is ~0.13 ms: under 1 % of a 60 fps
frame, for one keystroke, before any UI work.

**The "core validates every keystroke" contract needs no shell-side write buffer.** No design change.

### 2. Draft-handle lifecycle in a GC language — **H1 confirmed; `close()` is mandatory**

The generated Kotlin handle class is `AutoCloseable`, and `close()` is the **only** call site of the
release shim. The generated bindings contain **zero** occurrences of `Cleaner`, `finalize()` or
`PhantomReference` (verified by grep on the emitted file). Measured on ART:

```
gc_control.plain_bytearray_collected = true    <- the harness CAN collect
h1.kotlin_handle_collected           = true    <- ART collected the Kotlin handle
h1.live_draft_count_after_gc         = 1       <- Rust never freed the draft
h1b.kotlin_handle_collected          = true    <- and the zombie still rebases
callback.kotlin_checker_collected    = false   <- checkers are held strongly (correct)
```

Dropping the last Kotlin reference collects the wrapper and **leaks the Rust draft**. Worse, the leaked
draft is unreachable from Kotlin — it can never be closed — yet stays in the wrapper's registry, so
every `apply_canonical` keeps rebasing a zombie. This is precisely the hazard step 02 anticipated when
it wrote *"if the count does not fall, drafts leak / `apply_canonical` rebases zombies forever."* On
Apple it did fall. On Android it does not.

`close()`, `use { }` and a triple `close()` all behave correctly (idempotent, no double free).

### 3. Stream callback threading — works, with a caveat

Snapshots arrive on `DefaultDispatcher-worker-N` (the `callbackFlow` poll loop), and the flow **can be
collected on the main Looper** (`stream.collected_on_main_looper = true`) while mutations happen off
it. That is the Compose binding shape, and it mirrors Apple's confirmed `@MainActor` delivery.

Caveat: `callbackFlow` starts its poll loop asynchronously and exposes **no "subscribed" signal**, so a
consumer cannot know when its subscription is live. The probe sleeps 400 ms. That is not a test smell —
it is the subscribe race, unavoidable from the consumer's side.

## Probe matrix verdicts

**A — Chattiness.** See measurements. `ping` is *not* a floor for `try_set` (it allocates and decodes a
return `String`; `try_set` returns void on success), so the two are reported as comparable crossings.

**B — Lifecycle.** H1 confirmed (above). H2 confirmed (below). Double `close()` is guarded by the
generated `AtomicBoolean` and does not double-free. The callback object is held **strongly** by the
bindings in a `ConcurrentHashMap<Long, UniquenessChecker>`, so an abandoned checker survives GC and is
still invoked from Rust — and it is released **deterministically** when the draft that owns it is
closed, with no finalizer (see friction 6). Callbacks are the case BoltFFI gets right; handles are not.

**H2 — use-after-close is silent UB.** Handles are raw pointers (`__BoltffiHandle::new(v) as usize as
u64`); `close()` frees, and every generated instance method reads `this.handle` without consulting
`__boltffi_closed`. Observed, reproducibly, with **no SIGSEGV**:

```
h2.id_while_live                             = 0
h2.id_after_close                            = 0      <- stale read, silent
h2.id_after_churn                            = 1      <- now ALIASES a different live draft
h2.after_churn_handle_aliases_another_object = true
h2.try_set_after_close = returned normally (no error, no crash)
```

The absence of a crash is the bad news. A use-after-close returns the right answer until the allocator
reuses the block, then silently reports **another object's state**. On Apple this was impossible: ARC
kept the object alive, and the wrapper's post-submit tombstone made stale calls inert no-ops.

Run isolated behind `@HazardProbe` + `mise run test:android:hazard`, excluded from the default suite,
so a native crash could not destroy the other 34 probes' results.

**C — Streams.** End-to-end delivery works; main-Looper collection works; a fresh subscription is
**future-only**, exactly as in Swift. A 300-mutation burst against a deliberately slow collector
delivered **66 of 300**, and `draft.snapshot()` still read the final state — so step 02's ruling holds:
*drop-newest is not a kill as long as current state is recoverable.*

**A new structural finding on the version stamp** (recorded, **not** fixed — CLAUDE.md sends structural
questions to a design session): a draft snapshot's `version` is its `base_version`, and
`ProfileDraft::rebase(&mut self, entity)` takes no version and never updates it. The stamp is
**frozen at checkout for the draft's entire life**. Measured: `draft: 1 → 1` while `store: 1 → 2`, with
the rebase *proven* to have happened (the clean `name` field adopted `"Server Renamed"`).

That makes it worse than static — it is **stale**. Step 02 shipped the version stamp precisely so a
consumer could reconcile `snapshot()` against the first streamed event and close the future-only
subscribe gap. That mitigation works when observing the **entity** (the store stamps the live version)
and **cannot work when observing a draft**. Not a kill: `snapshot()` always reads current state.

**D — Typed errors.** All structural, never message strings, on a *different codegen backend* than
step 02's — so errors-as-data (§8) is a property of BoltFFI, not of the Swift generator:
`UsernameErrorFfi.TooShort(min=3, actual=2)`; the nested `ValidationReport` (`too_short`,
`{min=1, actual=0}`); the tier-2 `corporate_email` violation with `pins=[EMAIL]` and
`{expected=corp.example, actual=example.com}`. A second `submit()` yields the typed
`AlreadySubmitted`.

**E — Callback traits.** A Kotlin `UniquenessChecker` drives begin/complete; a `TAKEN` verdict blocks
validation with a `username_unique` rule violation and a later `UNIQUE` verdict unblocks it. I13
(value-bound verdict reset) holds: editing after a verdict returns the check to `Unchecked`.
**No reentrancy deadlock** — a checker that synchronously calls both `validate()` (read) and
`trySetName()` (mutation) back into the same draft completes, and the mutation lands
(`Valid(value=Reentrant Name)`). As on Apple, this is safe *because* the wrapper never holds its
`Mutex` across the outcall. BoltFFI holds no internal lock across the callback on JNI either.

**F — Packaging.** See measurements.

## Apple (step 02) vs Android (step 05)

| | Apple / ARC | Android / ART |
|---|---|---|
| Handle freed by | `deinit` → Rust `Drop`, **automatic** | **`close()` only** — GC never frees |
| Abandoned handle | deregisters itself | unreachable zombie, rebased forever |
| Use after free | impossible (ARC) + tombstone no-ops | **silent UB**, aliases another object |
| Stale-call safety | typed `AlreadySubmitted` tombstone | typed only *before* `close()` |
| `try_set` round-trip | 2.4–3.6 µs | 5.4–5.5 µs |
| `snapshot()` readback | 1.9–2.6 µs | 4.0–4.5 µs |
| Snapshot stream | `AsyncStream`, future-only | `Flow` (`callbackFlow`), future-only |
| Main-thread delivery | `@MainActor` ✓ | main Looper ✓ |
| Reentrant callback | no deadlock | no deadlock |
| Typed error payloads | ✓ | ✓ |
| Shipped artifact | `dist/apple` **127 MB** (unstripped) | `.so` **485 KiB stripped** (5.36 MB unstripped) |
| Headless CI | UI tier impossible (XCUITest) | fully headless ✓ |

Both FFI overheads were measured on the same Mac, so the ~1.5–2× is the **JNI + Kotlin marshaling
path**, not hardware.

## Measurements

All latencies are medians over 2,000 timed calls after 200 warmup calls, on ART. **Lower bounds** —
see the caveat.

| What | p50 | p95 |
|---|---|---|
| `System.nanoTime()` loop (timer overhead) | 0.3 µs | 0.3–0.4 µs |
| `ping()` — a no-op crossing, String in/out | 6.5–6.6 µs | 7.4–10.4 µs |
| `try_set_username` | 5.4–5.5 µs | 6.4–6.6 µs |
| `snapshot()` readback (whole DTO) | 4.0–4.5 µs | 5.5–6.2 µs |
| **KEYSTROKE = `try_set` + `snapshot`** | **12.1–13.0 µs** | 17.8–21.1 µs |

- Warm 20-keystroke burst: **0.355–0.369 ms** total (≈ 18 µs/keystroke).
- **Cold** first keystroke on a fresh draft: **44.8–46.7 µs** (≈ 3.7× warm) — measured deliberately,
  because warming only `try_set` and not `snapshot` inflated an earlier burst figure ~6×.
- Stream burst: **66 of 300** snapshots delivered to a slow collector; current state recoverable.
- `libspike_profile_ffi.so`: **5,618,000 B** unstripped (with `debug_info`) → **496,760 B (485 KiB)**
  after AGP's strip. Test APK: **2,345,527 B**. Stripping is an **11×** lever — exactly the one step 02
  flagged for Apple's 127 MB.
- `mise run pack:android` (release): **14.0 s** cold, **2.6 s** incremental after touching `lib.rs`.
- `mise run test:android`: **~23 s** warm, including emulator boot, install and 34 tests.
- Generated: **2,769** Kotlin lines + **1,013** lines of `jni_glue.c` + an 88-line C header, from the
  same annotations that produced 1,663 Swift lines in step 02.

## Friction log (input to the design freeze)

1. **`boltffi pack android` is broken out of the box (BoltFFI 0.27.3) — VISION risk 1 again.**
   `pack apple` builds the crate with the *binding-expansion* environment (`build/expansion.rs::env()`:
   `BOLTFFI_BINDING_EXPANSION=1`, `…_ROOT`, `…_SOURCE`, `…_SURFACE=native`, plus
   `RUSTFLAGS=--cfg boltffi_binding_expansion`), which switches the `#[export]` macro to crate-qualified
   symbol names. **`pack/android/mod.rs` passes `env: Vec::new()`.** The macro therefore emits legacy
   short names (`boltffi_profile_store_ffi_checkout`) while the JNI glue and C header generated *in the
   same run* reference the long ones (`boltffi_method_class_spike_profile_ffi_profile_store_ffi_checkout`).
   The `.so` links with 43 undefined symbols and ART dies at `System.loadLibrary`:
   ```
   dlopen failed: cannot locate symbol "boltffi_register_callback_spike_profile_ffi_uniqueness_checker"
   ```
   Diagnosed by symbol diff (Apple `.a` defines the long names; Android `.a` defined only the short
   ones). Workaround: replicate the Apple env in `pack:android` — undefined `boltffi_*` symbols go
   **43 → 0** (and `boltffi_method_class_*` definitions go 0 → 19). Both counts verified with `llvm-nm`. *Minimal repro: `boltffi pack android` any `#[export]`-ing crate and `nm -u -D` the `.so`.*
   **Report upstream; delete the workaround when fixed.**

2. **GC probes are trivially easy to get wrong, in two independent ways.** Both produced *false*
   results before being caught, and one briefly made H1 look "confirmed" by luck:
   - An instrumented APK is `debuggable`, so ART treats **every dex register of a live frame** as a GC
     root. A dead local still pins the object. Fix: create the referent on a worker thread and `join()`.
   - Worse: **`Reference.get()` carries a read barrier** under ART's concurrent-copying collector — it
     *marks the referent reachable* for the cycle in progress. A `get()`-then-`System.gc()` polling loop
     keeps the object alive forever. It made even an abandoned `ByteArray` look uncollectable. Fix:
     detect collection by polling a **`ReferenceQueue`** and never call `get()` in the loop.

   A permanent `gc_control_aPlainObjectIsCollectable` test now guards both: no GC assertion in that file
   means anything unless the control passes first.

3. **No gradle wrapper; toolchain pinned by mise per-task.** AGP 8.7.3 needs Gradle 8.9–8.11 and a JDK
   17–21, and rejects Homebrew's default `openjdk` (26). Gradle's own `wrapper` task also failed to
   validate its distribution URL from this sandbox. Both solved by pinning `java` + `gradle` in mise —
   as **per-task `tools`**, not `[tools]`, so `mise run check` never drags a ~300 MB JDK onto a machine
   that only wants Rust. (Same spirit as step 02 keeping `boltffi_cli` out of `[tools]`.)

4. **Instrumented-test stdout is not captured by Gradle.** `println` vanishes. Measurements leave the
   emulator via `android.util.Log`, which AGP saves per-test to
   `build/outputs/androidTest-results/managedDevice/debug/dev34/logcat-<class>-<test>.txt`.

5. **`UniquenessChecker` is generated as a plain `interface`, not a `fun interface`**, so Kotlin will not
   SAM-convert a lambda and every call site needs `object : UniquenessChecker { override fun … }`. A
   one-word codegen fix; a papercut on every capability. (Step 09/10.)

6. **Callbacks are released deterministically; handles are not — and ownership direction explains it.**
   The bindings hold the Kotlin checker strongly in `ConcurrentHashMap<Long, UniquenessChecker>`. When
   Rust drops its `Box<dyn UniquenessChecker>` the callback vtable's `free(handle)` calls back into
   `UniquenessCheckerCallbacks.free` → `map.remove` — **no finalizer involved**. Verified: closing the
   draft that owns the checker makes an abandoned checker collectable
   (`callback.collected_after_draft_close = true`), while it is uncollectable before.
   So **Rust owns the callback** and can release it across the boundary, whereas **Kotlin owns the
   handle** and BoltFFI gives Rust no hook to release that — which is exactly why `close()` is
   unavoidable. (An earlier draft of this report claimed the checker was freed by a bridge
   `finalize()`. That is wrong: the Kotlin templates carry such a method, but it is not emitted here.)

7. **The Rust draft has no observable `Pending` between FFI calls** — unchanged from step-02 finding 7.
   With a synchronous checker, begin+complete are atomic inside `run_username_check`, so `Pending` is
   only ever seen on the *stream*, never by a `snapshot()` caller. A real spinner still needs either an
   async trait method or a split begin/complete API across FFI.

8. **The subscribe race has no consumer-side remedy on a draft stream.** `callbackFlow` gives no
   "subscribed" signal, and the draft's `version` stamp is frozen (see the structural finding). Today a
   draft consumer cannot detect a missed event; it can only re-read `snapshot()`.

## Deviations from the step doc

- **`architectures = ["arm64"]`** (one ABI) rather than the CLI's default four. Rationale in the doc;
  the probe device is arm64-v8a and four ABIs cost 4× a release build for zero evidence.
- **H2 was executed**, not skipped. The step doc allowed a *not-executed* row if isolation was unsafe.
  It proved safe to isolate (a `@HazardProbe` annotation + `notAnnotation` filter + its own mise verb),
  and executing it turned a predicted "probably crashes" into the far more useful "does **not** crash,
  and silently aliases another object."
- **Padded ~30-field snapshot benchmark: CUT** (explicitly droppable). `snapshot()` already marshals the
  full `ProfileSnapshot` (4 field-state families + check + conflicts), and the keystroke figure sits 80×
  under the bar, so payload-scaling would not change any decision this step feeds.
- **JDK/Gradle pinned as per-task `tools`** rather than `[tools]` (friction 3) — a stricter reading of
  "`check` stays JDK-free" than the doc's own sketch.
- The doc's fallbacks (pre-created AVD + `am instrument`, or compile-only verification) were **not
  needed**: GMD provisioned the device headlessly on the first attempt.

## Kill criteria — none hit

1. **Chattiness** — median per-keystroke round-trip **12.1–13.0 µs** vs the **1.0 ms** bar. **Clears**,
   ~80×. *(Clearing on the emulator means "not obviously fatal"; re-check on hardware in step 07.)*
2. **Streams** — deliver, and a stalled-then-resumed consumer reaches current state via `snapshot()`.
   Not hit.
3. **Callbacks** — usable from Kotlin; reentrancy does not deadlock. Not hit.
4. **Errors** — payload-carrying variants reach Kotlin as typed data. Not hit.

Explicitly **not** kills, as the doc pre-committed: mandatory `close()` (H1) and use-after-close UB (H2)
are contract findings — confirming them was the point.

## Open questions handed to the freeze (feeding ARCHITECTURE §9)

- **Draft lifecycle / `close()` (§9, "pending Step 5") — ANSWERED, and it is asymmetric.** Swift needs
  no `close()`; Kotlin cannot live without one. The contract in §4 ("shells hold handles") must
  therefore expose an explicit release, and the Kotlin/Compose story is `use { }` or a lifecycle owner
  (`ViewModel.onCleared`). Two follow-ups for the freeze:
  - Should `bolted-ffi` generate a `java.lang.ref.Cleaner` (API 33+) as a **backstop** so an abandoned
    draft is eventually freed rather than a permanent zombie? Note BoltFFI already achieves
    deterministic release for *callback* objects (Rust-owned), so only the Kotlin-owned handle lacks a
    release hook.
  - **Should generated instance methods check the `closed` flag and raise a typed error?** Today
    use-after-close is silent UB (H2). This is a `bolted-ffi` requirement for step 10, and arguably a
    BoltFFI upstream fix.
- **Draft snapshot `version` is stale after rebase (new).** `rebase` takes no version and never updates
  `base_version`. Either drop `version` from *draft* snapshots (admitting the reconcile pattern is
  entity-only), or thread the store version through `rebase` so drafts stamp the canonical they are
  actually based on. Binds §4's `observe` contract. **Do not resolve ad hoc.**
- **Store concurrency model behind FFI (§9).** No new evidence; step 02's recommendation (Send state
  behind one lock, id-keyed handles, emit-outside-lock) held up unchanged under JNI — including the
  reentrancy test, which is the one that would have exposed a violation.
- **Async capability vs sans-io (§5).** Still open. The synchronous checker sufficed again, and the core
  stayed sans-io. Whether an async callback trait forces an executor on the Rust side remains
  un-probed on both platforms.
- **Zombie drafts and `apply_canonical` cost.** With no `close()`, the wrapper's registry grows without
  bound and every canonical change rebases every leaked draft. Even *with* disciplined `close()`, the
  freeze should decide whether the store holds drafts weakly.

## Exit checklist

- [x] `mise run check` passes; its `[tasks.check]` body is **byte-identical** to step 04's (verified by
      `diff` against `c53c68b`). No JDK, no Android SDK, no emulator required.
- [x] `mise run test:android` packs, boots the GMD emulator **headlessly**, and runs 34 probes green.
- [x] `mise run test:android:hazard` runs the 3 isolated H2 probes; results recorded.
- [x] Every probe-matrix row has a test. Nothing was left un-executed; the padded-snapshot **stretch**
      was cut and recorded.
- [x] `bolted-core`, `spike-profile`, `spike-profile-ffi/src` unchanged; only `boltffi.toml` differs.
- [x] H1 and H2 have **empirical verdicts on ART**, each guarded by a control that would have caught a
      false positive (and did).
- [x] This report written, with the emulator lower-bound caveat attached to the measurements.
- [x] ROADMAP updated (05 → done, 06 → ready).
