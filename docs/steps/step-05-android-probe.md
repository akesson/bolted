# Step 05 — Android headless probe (JNI + ART)

**Phase 1 · Spike.** Read first: [VISION.md](../VISION.md) (bet 1: BoltFFI is the boundary; risk 1:
BoltFFI is young; risk 5: host toolchains), [ARCHITECTURE.md](../ARCHITECTURE.md) (§4 drafts as
core-side handles, §5 crate layout + sans-io, §9 OPEN questions — **the first one is explicitly
"pending Step 5"**), [ROADMAP.md](../ROADMAP.md) (working agreement), and the step-02 handoff
([plan](step-02-boltffi-probe.md) · [report](step-02-report.md)) — this step re-runs step 02's
questions on the *other* runtime.

## Goal

Step 02 proved the four BoltFFI features on **Apple/ARC**. Its own step doc warned: *"Apple numbers
carry zero evidence for the JNI bet."* This step re-measures on the runtime VISION calls the worst
case — **ART, across JNI** — and answers the three questions ROADMAP reserved for it:

1. **JNI `try_set` round-trip cost at keystroke frequency.** *The* chattiness kill criterion.
   Perceptible per-keystroke latency means a shell-side write buffer, which is a design change to
   the "core owns every keystroke" contract (§2 tier-1 validation on every `try_set`).
2. **Draft-handle lifecycle in a GC language.** ARCHITECTURE §9, bullet one: *"Draft handle
   lifecycle in GC languages (`close()`? `use`-block?) — pending Step 5."* On Apple, ARC `deinit`
   ran Rust `Drop` and drafts deregistered themselves automatically. Does ART's GC do the same?
3. **Stream callback threading.** Which thread do `#[ffi_stream]` snapshots arrive on in Kotlin, and
   can a main-thread (Looper) consumer bind to them the way SwiftUI's `@MainActor` consumer could?

As in step 02, **a green test suite is necessary but is not the deliverable**. The deliverable is
evidence: the answered probe matrix, the measured numbers with their caveats, and the friction log
in `docs/steps/step-05-report.md`.

**Load-bearing principle (inherited from step 02):** *every restructuring the wrapper is forced into
is the probe's most valuable output.* Do not patch `bolted-core`, `spike-profile`, or
`spike-profile-ffi` to make Kotlin prettier — record what you had to work around.

## Reconnaissance already done (confirm it; do not take it on faith)

Read out of `boltffi_bindgen-0.27.3/src/render/kotlin/` before this doc was written. **These are
hypotheses to falsify on a running ART instance, not conclusions.** The whole point of the step is
that a template read is not evidence.

- **The generated Kotlin handle class is `AutoCloseable`, and `close()` is the *only* path that
  frees the Rust object:**
  ```kotlin
  class ProfileDraftFfi internal constructor(internal val handle: Long) : AutoCloseable {
      private val closed = AtomicBoolean(false)
      override fun close() {
          if (!closed.compareAndSet(false, true)) return
          Native.<ffi_free>(handle)
      }
  }
  ```
  There is **no `Cleaner`, no `finalize()`, no `PhantomReference`** anywhere in the Kotlin renderer.
  (The single `finalize()` in the Kotlin templates is on the *callback-trait bridge*, not on handle
  classes. The Java renderer does use `Cleaner`, but again only for callbacks.)
  ⇒ **Hypothesis H1:** dropping the last Kotlin reference to a draft *never* runs Rust `Drop`; the
  wrapper's registry keeps the draft forever and `apply_canonical` rebases zombies. This is the
  exact *opposite* of the Apple/ARC result, and it decides §9.
- ⇒ **Hypothesis H2 (memory-safety hazard):** `close()` frees the Rust object, but the generated
  instance methods do **not** consult `closed` — they pass the stale `handle` straight to JNI. So
  **use-after-close is undefined behaviour**, not a typed error. On Apple this was impossible (ARC
  kept the object alive and the wrapper's tombstone made post-submit calls inert no-ops).
- Streams render as `callbackFlow { … }` driven by a `BoltFFIStreamContext` **poll loop** (batchSize
  16) — the Kotlin analogue of the Swift poll loop step 02 found. Threading is therefore a property
  of the scope the flow is collected in *and* the context's internal dispatcher. Measure it.
- `[targets.android] architectures = ["arm64"]` restricts the build to one ABI (the CLI otherwise
  builds four). `min_sdk = 24`; `[targets.android.kotlin] error_style = "throwing"`.

## Non-goals (hard boundaries)

- **No changes to `bolted-core`, `spike-profile`, or `spike-profile-ffi`.** All three are the frozen
  subject of the experiment. `git diff` on those three crates must be **empty** at exit. The only
  permitted edit under `crates/` is `spike-profile-ffi/boltffi.toml` (enabling the Android target),
  which is configuration, not code.
  - If the wrapper genuinely cannot be driven from Kotlin without a Rust change → **stop and record
    it as a structural open question.** Do not resolve it here.
- **Do not "fix" the lifecycle finding.** If H1 confirms, the temptation is to hand-write a `Cleaner`
  into the wrapper or a Kotlin subclass. Don't: the wrapper is hand-written *as-if-generated*, so
  inventing generator behaviour here would fabricate evidence for step 10. Record the recommendation
  in the report and leave it to the freeze.
- **No UI.** No Compose, no Activity, no views — that is step 07. Instrumented tests only.
- No macros, no KMP target, no performance optimization, no published crates.
- **No host-JVM (HotSpot) test tier.** It would be cheap and it would be worthless: HotSpot on an
  M-series Mac carries no evidence about ART, and step 02 already proved the *semantics* once.
  Everything in this step runs on ART or it does not count.

## The measurement caveat, stated up front (this is the honest part)

The only Android runtime available here is an **`aosp_atd` arm64-v8a emulator on an arm64 host**.
That means ART is real, the JNI boundary is real, the GC is real — but the **CPU is not**. Guest
arm64 code runs natively on an M-series core, i.e. several times faster than the low-end phone VISION
names as the worst case.

Consequently every latency number in this step is a **lower bound**:

- If a kill criterion **fires** on the emulator, it is trustworthy — real hardware is only worse.
- If a kill criterion **clears** on the emulator, that means *"not obviously fatal"*, **not**
  *"confirmed safe"*. Clearing must be re-checked on physical hardware in step 07.

State this in the report next to every number. Do not let a green benchmark read as a settled bet.

## Deliverables

### 1. `crates/spike-profile-ffi/boltffi.toml` — enable the Android target

The only change under `crates/`. Enable `[targets.android]`, pin `architectures = ["arm64"]` (one
ABI: the emulator's; four ABIs of a crate that statically links `url → idna → icu` is the same
release-build cost step 02 measured at 25 s and one transient failure). Keep the existing
`[targets.android.kotlin]` block (`package = "com.example.spike_profile_ffi"`,
`error_style = "throwing"`, `api_style = "top_level"`).

`boltffi pack android --release` then emits generated Kotlin sources + `jniLibs/arm64-v8a/*.so` under
`dist/android/` (already gitignored via `crates/spike-profile-ffi/dist/`). Record the exact emitted
layout in the report — the Gradle `srcDirs` wiring depends on it.

### 2. `android/profile-probe/` — a Gradle instrumented-test project

Lives under `android/` (mirroring `apple/`), **outside the cargo workspace**, so `mise run check`
stays Rust-only and Android-free.

- `com.android.library` + `org.jetbrains.kotlin.android`, an empty `main` source set and an
  `androidTest` source set carrying the whole probe matrix.
- Source wiring: the generated Kotlin under `dist/android/…` as a `srcDir`; `jniLibs.srcDirs` at the
  packed `jniLibs/`. No copying, no vendoring — the probe must break loudly if `pack` drifts.
- **Gradle Managed Device** (`dev34` → `Pixel 2`, api 34, `aosp-atd`, arm64-v8a) so the emulator is
  provisioned and torn down **headlessly** by Gradle. This is the *headless* in "headless probe": no
  GUI session, no Accessibility permission, none of the XCUITest tax step 03 paid. The contrast is
  worth banking in the report.
- `kotlinx-coroutines` (the generated `callbackFlow` needs it) + `androidx.test` runner/JUnit4.

Toolchain versions observed on this machine (pin them; record drift): AGP **8.7.3**, Gradle
**8.11.1** (wrapper), Kotlin **2.1.0**, JDK **21** (AGP 8.7 rejects JDK 26 — the Homebrew default
`openjdk` is 26, so `JAVA_HOME` must be pinned), NDK **27.0.12077973**, `compileSdk` 35.

### 3. mise wiring

- `[tasks."pack:android"]` — `depends = ["setup:boltffi"]`; doctor-fail with a clear message if the
  NDK / `ANDROID_HOME` is missing; `rustup target add aarch64-linux-android` self-heal (as
  `pack:apple` and `build:web` do); then `boltffi pack android --release`.
- `[tasks."test:android"]` — `depends = ["pack:android"]`; doctor-fail if a JDK 17–21, the Android
  SDK, or the `aosp_atd` system image is absent; then `./gradlew dev34DebugAndroidTest`.
- **`mise run check` is untouched.** It must remain byte-identical: no Android, no JDK, no emulator.
  A box with neither Xcode nor an Android SDK still runs `check` green. Verify with `git diff`.
- `.gitignore`: `android/profile-probe/build/`, `android/profile-probe/.gradle/`.

## Ordered milestones (walking skeleton first — the toolchain is the riskiest part)

The Rust semantics are proven twice over. The risk here is the *pipeline*
(`pack android` → jniLibs + generated Kotlin → Gradle → GMD emulator → instrumented test), and
after that, the two hypotheses. Order so a toolchain quagmire still yields a partial verdict — and
so the **kill criterion is probed early**, not last.

1. **Skeleton.** `pack android`, then one instrumented test calling the trivial exported
   `ping()` across JNI, green via `mise run test:android` on the headless GMD emulator. *Prove the
   pipeline before writing any probe code.*
2. **Chattiness benchmarks (the kill criterion).** Fail fast: this is the only row that can stop the
   step.
3. **Lifecycle probes (H1, H2) — the §9 deliverable.**
4. **Streams (threading, subscribe race) + callback trait (+ reentrancy) + typed errors.** The
   step-02 matrix, re-run on ART.
5. **Report + ROADMAP.** (Droppable if cut, and recorded as cut: the padded-snapshot benchmark.)

## Probe matrix (each row ⇒ ≥1 instrumented test; record *observed* behaviour, not just pass/fail)

**A — Chattiness / JNI cost** *(kill criterion; see below)*
- `ping()` no-op round-trip — the **floor**: pure JNI crossing + `String` marshal, no core work.
  Without this baseline a `try_set` number cannot be attributed between "JNI is expensive" and "our
  DTO marshaling is expensive", and those two have opposite remedies.
- `try_set_username(raw)` round-trip, median + p95 over ≥1000 calls.
- `snapshot()` readback — marshals the whole `ProfileSnapshot` DTO. **This is the real per-keystroke
  cost**, because a shell repaints after every keystroke (§4 snapshot-per-change).
- A realistic **keystroke** = `try_set` + `snapshot()`. Report that pair as one number; it is what the
  kill bar is set against.
- **Stretch (droppable):** a padded ~30-field snapshot, to see whether marshaling scales with payload.

**B — Handle lifecycle under ART GC** *(ARCHITECTURE §9, bullet one)*
- `checkout()` raises `liveDraftCount()`. Baseline.
- **H1:** drop the only Kotlin reference, then force collection (`System.gc()` +
  `Runtime.getRuntime().runFinalization()` + allocation pressure, repeated). Assert what *actually*
  happens to `liveDraftCount()`. Predicted: it does **not** fall. If it does fall, H1 is falsified
  and Apple's automatic story generalizes — a much better outcome, and a surprising one.
- `close()` → count falls (Rust `Drop` ran → registry pruned).
- Kotlin `use { }` → count falls at block exit. The idiomatic shape a Compose ViewModel would use.
- Double `close()` is idempotent (the generated `AtomicBoolean` should make it so) — a double free
  would be a serious finding.
- **H2, use-after-close.** Call a method on a closed handle. Predicted: **UB** (dangling handle
  passed to JNI), likely a native crash. Run this **in its own test class, last**, and treat a
  process death as the *observation*, not a test failure — a SIGSEGV kills the whole instrumented
  run, so it must not be able to take the other probes down with it. If it cannot be isolated
  safely, **do not run it**: record the template-level reasoning and mark the row *not executed*.
- **Callback-object lifetime.** Rust holds the Kotlin `UniquenessChecker` across calls. Drop the
  Kotlin reference, force GC, then trigger a check. Does the checker survive (global ref) or has it
  been collected (weak/local ref → crash or missed callback)? The Kotlin bridge has a `finalize()`;
  find out what it frees and when.

**C — Streams (the `observe` verb on ART)**
- End-to-end: a mutation produces a snapshot the Kotlin `Flow` consumer receives, carrying the new value.
- **Delivery thread.** Record `Thread.currentThread().name` inside the collector. Can it be collected
  on the **main Looper** (`Dispatchers.Main`) while mutations happen on the main thread — the Compose
  binding shape? Contrast with Apple's confirmed `@MainActor` delivery.
- **Subscribe race.** Step 02 found a fresh Swift subscription replays *nothing* (future-only), which
  forced the version-stamped snapshot pattern. Confirm the same on Kotlin — a get-current-then-collect
  sequence can miss an event in the gap. The `version` stamp is already on the DTO; prove it reconciles.
- **Overflow.** Burst N snapshots against a stalled collector, resume, and confirm current state is
  recoverable (via the `snapshot()` getter if not via the stream). Step 02's ruling: drop-newest alone
  is not a kill *if* current state is recoverable. Re-confirm on the Kotlin poll loop.

**D — Typed errors on Kotlin** *(`error_style = "throwing"`)*
- `try_set_username("ab")` throws a typed exception whose `min`/`actual` are readable **structurally**,
  not parsed from a message string.
- A refused `submit()` throws `SubmitErrorFfi.Validation` carrying the nested `ValidationReport`
  (field ids + keyed `ErrorData` params + tier-2 rule violations). This is the same assertion step 02
  made in Swift; it must hold across a different codegen backend or the errors-as-data decision (§8)
  is language-specific.

**E — Callback traits (capabilities) on ART**
- A Kotlin-implemented `UniquenessChecker` drives begin/complete: a `taken` verdict blocks validation,
  a later `unique` verdict unblocks it.
- **Reentrancy / deadlock.** The checker, when invoked from Rust, synchronously calls back into the
  *same* draft (one read, one mutation). Must not deadlock — step 02 showed the wrapper's
  "never hold the `Mutex` across an outcall" rule is what makes this safe. If it deadlocks *here*
  despite that rule, the cause is JNI-side locking and that is kill-bar territory for feature 4.

**F — Packaging**
- `jniLibs/arm64-v8a/libspike_profile_ffi.so` size (stripped? unstripped?), `pack android --release`
  wall-clock, emulator boot + test wall-clock. Baselines for VISION's `bolted-check` size budget —
  step 02's Apple `dist/` was **127 MB**, inflated by unstripped static libs linking `url → idna → icu`.
  Expect the same bulk here; record it.

## Measurements (record numbers; **no pass/fail thresholds except the kill bar** — this is a baseline)

- `ping()` no-op JNI round-trip: median.
- `try_set_username`: median, p95.
- `snapshot()` readback: median.
- **per-keystroke (`try_set` + `snapshot`)**: median. ← the kill bar is set against this.
- `.so` size per ABI; `pack android --release` wall-clock; `test:android` cold and warm wall-clock.
- Versions: `boltffi`/`boltffi_cli`, AGP, Gradle, Kotlin, JDK, NDK, system image + API level, and the
  emulator/host CPU (so the lower-bound caveat is reproducible).

## Kill criteria (per ROADMAP: hitting one is a *successful* probe outcome — stop and report)

"Broken" = **cannot be made to work with reasonable hand-written Kotlin.** Awkwardness, extra
boilerplate, or a discipline you had to adopt = *friction finding*, not a kill.

1. **Chattiness (the one this step exists for).** Median **per-keystroke round-trip
   (`try_set` + `snapshot`) > 1.0 ms on the emulator.**
   Rationale for the bar: a 60 fps frame is 16.7 ms. A low-end phone runs perhaps 5–10× slower than
   this emulator's host core, so 1.0 ms here projects to 5–10 ms there — over half a frame consumed by
   *one keystroke*, before any UI work. Crossing that means the "core validates every keystroke"
   contract needs a shell-side write buffer, which is a design change, not an optimization.
   *(Calibration: Apple measured 2.4–3.6 µs for `try_set`. The bar is ~300× that. It should not fire.
   A kill criterion that fires on a coin flip is not a kill criterion.)*
2. **Streams.** Snapshots cannot be consumed from Kotlin at all, or a stalled-then-resumed collector
   **cannot reach current state by any means**. (Drop-newest alone is not a kill — step 02's ruling.)
3. **Callbacks.** Kotlin→Rust trait implementations are unusable, or JNI-side locking makes reentrancy
   deadlock unavoidable.
4. **Errors.** Payload-carrying variants are flattened to strings; no typed associated data reaches Kotlin.

**Explicitly NOT kill criteria** — these are contract findings, and confirming them is the point:
- Manual `close()` / `use {}` being **mandatory** on Android (H1). This changes ARCHITECTURE §4's
  handle story and answers §9; it does not stop the step.
- Use-after-close being UB (H2). A hazard the generator must close, recorded for step 10.
- Snapshot marshaling being materially more expensive than on Apple, so long as it stays under the bar.

**On hitting a kill criterion: stop.** Write a minimal standalone repro and report. Do not engineer
around it.

## Exit checklist

- [ ] `mise run check` passes and its `mise.toml` task body is **unchanged** — no JDK, no Android SDK,
      no emulator needed for a green `check`.
- [ ] `mise run test:android` packs, boots the GMD emulator **headlessly**, and runs the probe suite green.
- [ ] Every probe-matrix row has a test **or** an explicit, justified "not executed" note (H2 may be one).
- [ ] `bolted-core`, `spike-profile`, and `spike-profile-ffi/src` are **unchanged** (`git diff` clean;
      only `boltffi.toml` may differ).
- [ ] H1 and H2 have **empirical verdicts on ART**, not template readings.
- [ ] `docs/steps/step-05-report.md` written: what was built; the answered probe matrix; the three
      ROADMAP questions answered; the measurements **with the emulator lower-bound caveat attached to
      each**; the Apple-vs-Android contrast table; deviations; friction log; open questions feeding §9.
- [ ] `docs/ROADMAP.md` updated (05 → done, 06 → ready), including the §9 `close()` bullet's resolution
      status.

## If you hit a wall

Same rule as steps 01–04 (CLAUDE.md): an omitted decision → smallest reversible choice, recorded in the
report. A **structural** conflict (a change to `bolted-core`/`spike-profile`/`spike-profile-ffi`, a new
invariant, an ARCHITECTURE change, or one of the four features failing) → **stop and record the
question** for a design session; do not resolve it here.

The emulator is the single point of failure. If GMD cannot provision the device headlessly, try in
order: (a) a pre-created AVD driven by `adb` + `am instrument`; (b) `boltffi pack android` verified by
`cargo build --target aarch64-linux-android` + a Gradle *compile* of the generated Kotlin (proves
codegen and linkage, not runtime). If runtime evidence is unobtainable, **say so plainly in the
report** and mark every ART-dependent row *not executed*. A step that honestly reports "no ART evidence"
is worth far more than one that quietly substitutes HotSpot numbers.
