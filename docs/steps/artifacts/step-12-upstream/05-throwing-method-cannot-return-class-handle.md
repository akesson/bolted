# A throwing / `Result`-returning `#[export]` method cannot return a class handle

**Version:** boltffi 0.27.3 · **Severity:** medium (forces an awkward two-step API for fallible factories)

## Summary

An `#[export]` method returning `Result<T, E>` (with `error_style = "throwing"`) works when `T` is a
`#[data]` value or `()`. When `T` is a **class handle** it fails to compile: the whole `Result<Handle,
E>` is required to implement `WireEncode`, and a class handle is not `WireEncode`. So you cannot write
a *fallible constructor / factory that returns a handle* — e.g. `fn restore(..) -> Result<Draft, E>`.

## Repro (step 12 M3)

```rust
#[export]
impl Store {
    // does NOT compile: `Result<DraftFfi, StashRefusedFfi>: WireEncode` is not satisfied,
    // because `DraftFfi` (a class handle) is not `WireEncode`.
    pub fn restore(&self, stash: StashFfi) -> Result<DraftFfi, StashRefusedFfi> { … }
}
```

```
the trait bound `DraftFfi: WireEncode` is not satisfied
required for `Result<DraftFfi, StashRefusedFfi>` to implement `WireEncode`
required by a bound in `FfiBuf::wire_encode`
```

## Expected

A throwing method may return a class handle: on `Ok`, return the handle; on `Err`, throw. The handle
is returned by the same mechanism a non-throwing method already uses.

## Workaround (this repo, D27)

A two-type dance: `accept_stash(stash) -> Result<StashAcceptedFfi, E>` gates into a `#[data]` token,
and `restore(accepted: StashAcceptedFfi) -> DraftFfi` (infallible) consumes it. This is actually a
decent *parse-don't-validate* shape — the token proves the gate ran — but it is a workaround for a
toolchain limitation, not a design preference, and it costs an extra FFI call and a wrapper DTO.

## Acceptance test

`fn make(..) -> Result<SomeClass, SomeError>` in an `#[export]` impl compiles and, from the binding,
either returns the object or throws the typed error.
