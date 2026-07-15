# DTO wire ser/de (`toByteArray`/`fromByteArray`) is `internal` — unreachable from a shell

**Reported against:** boltffi 0.27.3 · **Severity:** medium (ergonomics; forces a hand-written codec
per DTO) · **Disposition at 0.27.5: ALIVE.**

> **Upstream status (2026-07-15):** filed as
> [boltffi/boltffi#666](https://github.com/boltffi/boltffi/issues/666) (open, no maintainer
> response yet). The as-filed text — reworked from this draft into an opt-in-visibility proposal,
> re-verified against the IR-backend templates on `main` — is `04-issue.md` in this directory.

## Summary

Every emitted DTO already carries a complete binary wire ser/de — Kotlin `wireSize()` / `writeTo()` /
`toByteArray()` and `fromReader()` / `fromByteArray()` over `WireReader`/`WireWriter`; the Swift
equivalent. **All of it is `internal`/module-private.** A shell in a *different* module (the normal
case — the app is not the bindings module) cannot call any of it. So a shell that needs to persist a
DTO (Android `SavedStateHandle`, disk cache) must **hand-write a codec** — typically JSON —
duplicating serialization the tool already emitted, one file per persisted DTO.

## Re-verification at 0.27.5 (step 15 M4) — ALIVE

In the freshly generated 0.27.5 Kotlin (`boltffi generate kotlin`), **every** DTO codec is still
`internal`, including the persistence DTO the original draft named:

```
GenProfileFfi.kt:1400:    internal fun toByteArray(): ByteArray { …          // ProfileStashFfi
GenProfileFfi.kt:1424:        internal fun fromByteArray(bytes: ByteArray): ProfileStashFfi { …
```

(and the same `internal` modifier on all ~24 other DTO codecs — `ProfileValues`, `ProfileSnapshot`,
`AvailabilityStash`, …). Swift DTOs expose only module-internal `@inlinable func encode(to:)` /
`WireReader` initializers. There is no config to widen them.

## Ask

Expose the existing DTO wire ser/de as **public**, or add an opt-in config
(`[targets.*.<lang>] public_wire_codec = true`) that widens `toByteArray`/`fromByteArray` (and the
`WireReader`/`WireWriter` they need) to public.

## Impact

The **smallest change that retires per-feature persistence codecs entirely**: a shell would
`base64(stash.toByteArray())` into its state container and decode symmetrically, with no generated or
hand-written codec at all. (This repo still ships a generated `ProfileStashCodec.kt` purely to work
around this.)

## Acceptance test

From a module that only *depends on* the generated bindings,
`MyDto.fromByteArray(myDto.toByteArray()) == myDto` compiles and round-trips.
