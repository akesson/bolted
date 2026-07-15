# DTO wire codecs are emitted but `internal` — consumers must hand-write duplicate serializers. Proposal: opt-in public visibility

## Summary

Every generated DTO already carries a complete binary wire codec — Kotlin `wireSize()` / `writeTo()` / `toByteArray()` / `fromReader()` / `fromByteArray()`, and the Swift `encode(to:)` / `decode(from:)` equivalents. All of it is `internal` (module-internal in Swift), and there is no configuration to widen it. A consumer in a different module — the normal case, since the app is not the bindings module — cannot call any of it.

The practical consequence: any app that needs to persist a DTO (Android `SavedStateHandle` for process-death survival, a disk cache, IPC) must hand-write a parallel codec — typically JSON — duplicating serialization the generator already emitted, one codec per persisted DTO. In our app every persisted DTO ships with a hand-maintained companion codec file that exists purely to route around the visibility modifier.

## Current behavior

Verified against generated output at 0.27.5 and against the templates on current `main` — the IR backend emits the same visibility, so this is not legacy-renderer debt that the migration retires.

Kotlin (`boltffi_backend/templates/target/kotlin/record.kt`):

```kotlin
    internal fun toByteArray(): ByteArray = ...
    internal fun fromByteArray(bytes: ByteArray): {{ record.name() }} { ... }
```

The wire runtime the codecs depend on is also internal (`templates/target/kotlin/runtime.kt`):

```kotlin
internal class WireReader(private val bytes: ByteArray) { ... }   // line 139
internal class WireWriter(initialCapacity: Int) { ... }           // line 332
```

Swift (`templates/target/swift/record.swift` + `wire.swift`): `@inlinable static func decode(from:)` / `@inlinable func encode(to:)` over a `@usableFromInline struct WireReader` — visible inside the module, unreachable from an importing module.

So from a module that merely depends on the generated bindings, `myDto.toByteArray()` does not compile, even though the method exists and is exercised on every FFI call.

## Why it is presumably `internal` today — and why opt-in still makes sense

The wire format is an FFI ABI, not a stable serialization format: it carries no version tag, and the project is free to change the layout between releases. Making the codecs unconditionally public would invite people to write those bytes to long-lived storage, where a later boltffi upgrade could misdecode them. That is a legitimate reason to keep the default exactly as it is.

But the dominant persistence use cases are short-lived: `SavedStateHandle` bundles, in-process caches, IPC between components shipped in the same binary. There the bytes never outlive the installed library version, so the version-skew risk is zero — and today those cases pay the full cost (a hand-written codec per DTO) for a stability guarantee they don't need. An opt-in flag keeps the safe default and moves the tradeoff to the consumer who asked for it.

## Proposal

An opt-in per-target config key, default off:

```toml
[targets.android.kotlin]
public_wire_codec = true
```

When enabled, the DTO codec methods (`toByteArray` / `fromByteArray` and friends) and the `WireReader` / `WireWriter` they require are emitted `public` instead of `internal`. Same shape for Swift (`public` on the codec methods, `public` on the wire runtime types) and any other target that grows the need.

Generated docs on the widened methods could carry the caveat explicitly: the byte format is stable only within a boltffi version; do not use it for storage that outlives a library upgrade.

---

This was done with the help of Claude Fable, but reviewed by myself. I'd be happy to provide a PR for it if you'd like.
