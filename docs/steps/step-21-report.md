# Step 21 — report: capability coverage, resolved by construction (D34)

**Status: done. No kill criteria hit.** [Plan](step-21-capability-coverage.md) · ARCHITECTURE
**v1.10** (D34, authored in this step's planning pass).

## What was built

The capability moved from a settable slot to an **explicit optional argument of the generated
draft entry points**, through the whole chain in one pass:

1. **Generator** (`bolted-ffi-gen/src/wrapper.rs`): `checkout`/`restore` emit one
   `Option<Box<dyn XChecker>>` parameter per declared `#[check]`; the slot is filled from it;
   `set_*_checker` is no longer emitted. The driver keeps its take-out-of-the-mutex reentrancy
   dance, so `Ok(false)` survives with a narrowed meaning: *declared absence, or a reentrant call
   during the outcall*. `golden.rs` pins the shape from both sides
   (`the_capability_is_a_checkout_argument_and_nothing_else`): positive needles read out of the
   parameter lists via `between` (wrap-proof), negative needle on the setter name — plus
   `gen-note`'s `checkout(&self)` pinned parameter-free.
2. **Emitted foreign suites** (`foreign.rs`): three per-feature argument spellings — declared
   absence, `passingChecker()`, restore's trailing absence — resolve into the templates
   (`@@CO_NONE@@` / `@@CO_CAP@@` / `@@RS_NONE@@`), empty for a check-less feature. The emitted
   C16 test now proves the **sharper** claim: a capability *wired at checkout but never run*
   still blocks a dirty pinned field — C16 binds on UNRUN, not on absent.
3. **Shells**: Swift (app VM, probe, smoke), Kotlin (app VM, probe, smoke) and the C# probe pass
   the capability at checkout/restore. The app VMs got *simpler*: one line where checkout + setter
   were two, and the Kotlin `recheckout()` leak comment now describes a shape that cannot recur.

## The empirical gate (kill criterion 1): cleared on all three backends

`Option<Box<dyn Trait>>` as an exported-method **parameter** had never crossed boltffi in this
repo. It does, on every live backend, with tests exercising both `nil` and non-nil paths:

- Generated Swift: `public func checkout(usernameChecker: UsernameChecker?) -> ProfileDraftFfi`
  (`restore(accepted:usernameChecker:)` likewise) — `test:apple` green, 0 failures.
- Kotlin: `checkout(usernameChecker: UsernameChecker?)` — `test:android` **80/0** (JUnit XML,
  fresh timestamp), `test:android:app` (36+ incl. l10n) and `test:android:gen` (6) green with
  `--rerun-tasks`.
- C#: `Checkout(UsernameChecker?)` — `test:csharp` **14/14**; the step-15 tripwire
  `TheCheckDriverIsBrokenOnThisBackend` stays green (the driver bug is upstream and orthogonal —
  registration through the vtable at checkout does not throw).

## Falsification (all watched red, then restored green)

| Planted | Watched |
|---|---|
| Swift call site written the old way (`store.checkout()`) | `error: missing argument for parameter 'usernameChecker' in call` — the rung-2 claim, verbatim |
| Kotlin call site same | `e: CallbackProbe.kt:48:31 No value passed for parameter 'usernameChecker'.` — `compileDebugAndroidTestKotlin FAILED`, exit 1 |
| Generator mutated to re-emit the setter | golden red (`the settable slot survived`) |
| Generator mutated to drop the parameter | golden red (positive needle missing from the parameter list) |

## Deviations from the plan

1. **`gen-note-ffi/src/generated.rs` is not byte-identical** — its *surface* did not move
   (`checkout(&self)`, pinned by golden), but the D34 doc comment on `checkout` is emitted
   unconditionally, so the file changed by comment lines. The exit checklist said "byte-identical";
   the honest reading ("no check ⇒ no parameter") holds, the literal one did not. Making the
   comment conditional bought nothing and was skipped.
2. **The checker-swap probe tests were restructured, not just re-argued.** Three probes (Swift
   `CallbackTests`/`CheckStateAndConstraintsTests`, Kotlin `CallbackProbe`) installed *different*
   checkers on one draft mid-test. Under D34 a draft's capability is fixed at checkout, so they
   now use a scripted checker whose *answers* evolve (`SequencedChecker`) — the semantic under
   test (a verdict belongs to the value, C13/C10) is unchanged, and kill criterion 2 (a real
   surface needing mid-draft swap) never fired: every swap site was test scaffolding.
3. **`LifecycleProbe`'s callback-lifetime probes** needed a new helper
   (`checkoutWithAbandonedChecker`): the checker is created *and wired* on the worker thread so
   its only remaining owner after `join()` is the bindings' strong map — same question as before,
   asked through the new entry point.
4. `test:android:app` and `test:android:gen` were run in addition to the planned tiers (the app
   VM and smoke trees changed, so their tiers owed evidence).

## Friction log (for the wire emitter and the next capability family)

1. **The Swift argument label is derived, not chosen**: boltffi labels a parameter by its Rust
   name lower-camelized (`username_checker` → `usernameChecker:`), so the suite emitter builds
   the label from the declaration (`c_label` in `foreign.rs`). Any emitter that names a call site
   in Swift must derive the same label or it emits code that does not compile.
2. **The slot stays `Mutex<Option<…>>` even though the default is gone** — the driver takes the
   checker out for the lock-free outcall, so `None` is transiently real (reentrancy). A future
   reader who "simplifies" the Option away reintroduces the deadlock the outcall discipline
   prevents.
3. **A capability's identity is checkout-scoped state.** Tests that want evolving verdicts write
   stateful checkers; nothing real wanted a second identity. If a product ever does, that is D34's
   revisit evidence — bring it to a design session, do not resurrect the setter locally.
4. A feature with **several** checks gets several parameters (one per `#[check]`); no such
   feature exists yet, so the shape is emitted-but-unexercised beyond n=1. The golden pins are
   per-check and generalize; the first two-check feature should watch its own platform compile.

## Open questions

None new for ARCHITECTURE §9. The "second capability family" and "capability registry" boundaries
are recorded in D34 itself (reopen with the second family, D20 discipline).
