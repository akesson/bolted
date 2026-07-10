# Step 12 M0 — the passthrough probe (recorded answer)

**Question (step doc M0):** can BoltFFI 0.27.3 annotate the DTOs it emits — `@Parcelize` on
Android, `Codable` on Apple, or any attribute/conformance passthrough — so a shell can persist a
stash DTO without a hand-written codec?

**Answer: no.** Timeboxed probe, ~25 min, boltffi 0.27.3 (`~/.cargo/bin/boltffi`).

## What the probe found

1. **No annotation/derive/passthrough config exists.** `boltffi init`'s default `boltffi.toml`
   has no `parcel*`/`codable`/`serial*`/`derive`/`annot*`/`conform*` key under
   `[targets.android.kotlin]` or `[targets.apple.swift]` (or anywhere). `boltffi generate
   kotlin --help` exposes no such flag. The binary emits no `@Parcelize` / `Parcelable` /
   `kotlinx.serialization` / `@Codable` strings.

2. **Emitted DTOs already carry a binary wire ser/de — but it is unreachable.** Every emitted
   DTO (`ProfileStashFfi`, `TextFieldStashFfi`, …) has `wireSize()` / `writeTo()` /
   `toByteArray()` and a companion `fromReader()` / `fromByteArray()`, over boltffi's own
   `WireReader` / `WireWriter`. On Kotlin these are **`internal`**; `WireReader`/`WireWriter`
   are `internal class`, `WireWriterPool` is `private object`. `internal` is module-scoped, and
   the shell (`android/profile-app`) is a *different* Gradle module from the bindings — so it
   cannot call any of it. Confirmed: no `public` on any `toByteArray`/`fromByteArray`/`WireReader`.

3. **Swift DTOs are `Hashable, Equatable, Sendable` — not `Codable`,** and expose no public wire
   ser/de either. Apple has no stash codec today (nothing stashes on Apple), so this is moot for
   this step but relevant to step 13.

## What this decides

- **The annotation branch of deliverable 5 is dead.** boltffi 0.27.3 cannot stamp `@Parcelize` /
  `Codable`, so "persist the DTO directly via a platform annotation" is not on the table.

- **The ideal zero-codec path is one visibility change upstream away.** boltffi *already* has a
  total, versioned-by-shape wire ser/de for every DTO; it is `internal`. If it were `public`
  (or opt-in-public via config), the shell would base64 `stash.toByteArray()` into
  `SavedStateHandle` and StashCodec would delete itself with **no generated replacement at all**.
  → **Upstream filing** (M6 deliverable 9): ask for public/opt-in-public DTO wire ser/de. This
  is the smallest ask that fully retires per-feature codecs, and it composes with the existing
  `fun interface` / `__boltffi_closed` asks.

- **Deliverable 5 therefore takes branch B — generate the codec — with one caveat carried to
  M4.** Branch B ("the codec becomes a generated file beside the bindings, drift-checked") means
  emitting *Kotlin* from `bolted-ffi-gen`, which today emits only Rust (`generated.rs`).
  Standing up a foreign-language emitter is the axis step 13 is chartered to design
  ("informed by what this step learns about emitting foreign-language source at all"). The M4
  decision — build a minimal Kotlin codec emitter now vs. convert deliverable 5 to the upstream
  filing under KC1 — is deferred to M4 with the actual build cost in hand. **D27's version
  requirement does not depend on that call:** the schema version becomes a declaration-stamped
  field of the Rust stash DTO (boltffi round-trips it through the wire format for free), so the
  version is generated, not a hand-written Kotlin `FORMAT_VERSION`, either way.

## Scoping note for step 13

boltffi's wire ser/de is `internal`, so step 13's per-language contract tests cannot lean on it
to round-trip DTOs; step 13 will need either the upstream visibility change to have landed, or
its own emitted (de)serialization. Recorded here so step 13's doc starts from fact.
