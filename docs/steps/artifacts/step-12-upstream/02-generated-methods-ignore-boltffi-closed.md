# Generated methods never consult `__boltffi_closed` → use-after-close is silent UB

**Version:** boltffi 0.27.3 · **Severity:** high (memory-safety hole in generated bindings)

## Summary

A generated handle class (Kotlin `AutoCloseable`, Swift `deinit`) holds a `__boltffi_closed` flag set
by `close()`, guarded by an idempotent CAS. **No other generated method consults that flag.** After
`close()`, the foreign handle is a dangling pointer into freed Rust memory, and calling any method on
it hands that pointer straight to JNI/the C ABI — no Rust of ours runs before the dereference. It does
not crash reliably; it silently reads or writes freed memory.

## Repro (step 05, hazard H2 — Kotlin/ART)

```kotlin
val a = store.checkout()
a.close()                 // frees the Rust draft
a.trySetUsername("x")     // use-after-free: dereferences a dangling pointer
```

Observed: no exception. On ART the freed slot is often reallocated to the *next* live draft, so the
call silently mutates an unrelated draft — the worst kind of aliasing bug.

## Contrast with the store-side refusal

BoltFFI already models a *different* lifetime failure well: a draft the store released (submit/close)
but whose foreign handle survives makes `draft_mut(id) → None`, and a generated mutator can return a
typed error (this repo's `DraftClosedFfi`). That path is safe. The **foreign-side** raw-pointer hazard
above is untouched by it, and is not fixable from our side of the boundary.

## Ask

Have generated methods consult `__boltffi_closed` before dereferencing the handle, and raise the
binding's typed error (throw on Kotlin/Swift) instead of entering UB. A single flag check on entry.

## Adjacent asks (same root: the handle class is bindgen output we cannot reach around)

- **Kotlin `fun interface` for single-method capability traits.** BoltFFI emits `UsernameChecker` as a
  plain `interface`, which Kotlin cannot SAM-convert, so a shell writes `object : UsernameChecker { … }`
  instead of a lambda. Emitting `fun interface` for single-abstract-method capability traits restores
  the lambda ergonomics.
- **An opt-in `java.lang.ref.Cleaner`.** For teams that accept nondeterministic cleanup, an opt-in
  Cleaner registered *inside* the generated class (so it composes with the `__boltffi_closed` CAS,
  rather than an outside registration that would bypass the idempotence guard and risk a double-free)
  would backstop a forgotten `close()`. This repo declined an *external* Cleaner for exactly that
  reason (design decision D26) — an in-class opt-in is the only safe shape.

## Acceptance test

`a.trySetUsername("x")` after `a.close()` throws the binding's typed error rather than silently
succeeding or corrupting another handle.
