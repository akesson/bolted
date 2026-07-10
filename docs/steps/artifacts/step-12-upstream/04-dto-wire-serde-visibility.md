# DTO wire ser/de (`toByteArray`/`fromByteArray`) is `internal` — unreachable from a shell

**Version:** boltffi 0.27.3 · **Severity:** medium (ergonomics; forces a hand-written codec per DTO)

## Summary

Every emitted DTO already carries a complete binary wire ser/de — Kotlin `wireSize()` / `writeTo()` /
`toByteArray()` and `fromReader()` / `fromByteArray()` over `WireReader`/`WireWriter`; the Swift
equivalent. **All of it is `internal`/`private` to the emitted module.** A shell in a *different*
module (the normal case — the app is not the bindings module) cannot call any of it. So a shell that
needs to persist a DTO (Android `SavedStateHandle`, disk cache) must **hand-write a codec** — typically
JSON — duplicating serialization the tool already emitted, one file per persisted DTO.

## Repro (step 12 M0)

`ProfileStashFfi.toByteArray()` exists in the generated Kotlin but is `internal`; from
`android/profile-app` (a separate Gradle module) it is not visible. There is no config to widen it.
Swift DTOs are `Hashable, Equatable, Sendable` but expose no public wire ser/de either.

## Ask

Expose the existing DTO wire ser/de as **public**, or add an opt-in config
(`[targets.*.<lang>] public_wire_codec = true`) that widens `toByteArray`/`fromByteArray` (and the
`WireReader`/`WireWriter` they need) to public.

## Impact

This is the **smallest change that retires per-feature persistence codecs entirely**: a shell would
`base64(stash.toByteArray())` into its state container and decode symmetrically, with **no generated
or hand-written codec at all**. In this repo it directly closes step-12 deliverable 5
(`StashCodec.kt` deletes itself) — see `docs/steps/artifacts/step-12-m4-codec.md`.

## Acceptance test

From a module that only *depends on* the generated bindings, `MyDto.fromByteArray(myDto.toByteArray())
== myDto` compiles and round-trips.
