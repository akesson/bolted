# Generated methods never consult `__boltffi_closed` → Kotlin use-after-close is silent UB

**Reported against:** boltffi 0.27.3 · **Severity:** high (memory-safety hole) · **Disposition at
0.27.5: ALIVE.**

> **Upstream status (2026-07-15):** fixed on `main` by our PR
> [boltffi/boltffi#663](https://github.com/boltffi/boltffi/pull/663) ("Guard JVM handle reads
> against use-after-close"), **merged 2026-07-14** — not yet in a release (latest is 0.27.5).
> The residual *concurrent* use-after-close race (close() racing an in-flight call) is filed as
> [#664](https://github.com/boltffi/boltffi/issues/664) (open), with a prior-art comment on the
> atomic in-flight-counter design.

## Summary

A generated handle class (Kotlin `AutoCloseable`) holds a `__boltffi_closed` flag set by `close()`,
guarded by an idempotent CAS. **No other generated method consults that flag.** After `close()`, the
foreign handle is a dangling pointer into freed Rust memory, and calling any method on it hands that
pointer straight to JNI — no Rust of ours runs before the dereference. It does not crash reliably; it
silently reads or writes freed memory.

## Repro (in this repo: `android/profile-probe`, `mise run test:android:hazard`)

`UseAfterCloseProbe` (annotated `@HazardProbe`, run in isolation because it may crash the process):

```kotlin
val draft = store.checkout()
val idWhileLive = draft.id()   // 0
draft.close()                   // frees the Rust draft; liveDraftCount() -> 0
val idAfterClose = draft.id()   // reads the freed handle
```

## Re-verification at 0.27.5 (step 15 M4) — ALIVE

`test:android:hazard` logcat (per-test, saved under `androidTest-results/.../logcat-*`):

```
h2.id_while_live = 0
h2.id_after_close = 0
h2.read_after_close_returned_stale_value_silently = true    ← still silent stale read
h2.after_churn_handle_aliases_another_object = true         ← dangling handle now aliases ANOTHER draft
h2.try_set_after_close = threw: UsernameErrorFfi$DraftClosed
```

`id()` — a method that reads the raw handle directly — **still returns the freed value silently**, and
after allocator churn the same dangling handle **aliases a different live draft** (the worst kind of
aliasing bug). The generated methods still do not consult `__boltffi_closed` before dereferencing.
**The memory-safety hole is unchanged.**

(`trySetUsername` after close threw a typed `DraftClosed` this run — but that is the *store-side*
refusal (`draft_mut(id) → None`) catching a still-valid-enough freed `Arc`, plus the non-determinism
of UB; at 0.27.3 the same call "returned normally". `id()` is the clean probe of the raw-pointer
hazard, and it is still unsound. The store-side typed refusal was always the safe path; the
foreign-side raw-pointer hazard is the bug.)

## Ask

Have generated methods consult `__boltffi_closed` before dereferencing the handle, and raise the
binding's typed error (throw on Kotlin) instead of entering UB. A single flag check on entry.
(The C# backend already does exactly this — `ThrowIfDisposed()` before every native call — so
use-after-dispose there is a typed `ObjectDisposedException`, not UB. Kotlin should match it.)

## Adjacent asks (same root: the handle class is bindgen output we cannot reach around) — both ALIVE at 0.27.5

- **Kotlin `fun interface` for single-method capability traits.** `UsernameChecker` is still emitted as
  a plain `interface` (`GenProfileFfi.kt`), which Kotlin cannot SAM-convert — a shell must write
  `object : UsernameChecker { … }` instead of a lambda. Emit `fun interface` for single-abstract-method
  capability traits.
- **An opt-in `java.lang.ref.Cleaner`.** No Cleaner is registered in the generated Kotlin handle class.
  For teams that accept nondeterministic cleanup, an opt-in Cleaner registered *inside* the generated
  class (so it composes with the `__boltffi_closed` CAS rather than an external registration that would
  bypass the idempotence guard) would backstop a forgotten `close()`. (The C# backend gained a
  finalizer that reaches the store-side close — a precedent for the in-class shape.)

## Acceptance test

`draft.id()` (and any method) after `draft.close()` throws the binding's typed error rather than
silently returning a stale value or aliasing another live handle.
