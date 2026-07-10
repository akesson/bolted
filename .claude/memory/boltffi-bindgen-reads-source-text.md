---
name: boltffi-bindgen-reads-source-text
description: "BoltFFI discovers its FFI surface by parsing source files with syn, never expanded Rust — so a proc macro cannot emit #[data]/#[export], and the omission is silent"
metadata:
  node_type: memory
  type: reference
---

BoltFFI (0.27.3) finds its FFI surface with `boltffi_scan::SourceTree::load`: `std::fs::read_to_string`
plus `syn::parse_file`, walking `mod` declarations from the crate root. **It never sees expanded Rust.**
Not even under `BOLTFFI_BINDING_EXPANSION`, whose `Request::render()` re-reads the same file from disk;
that mode governs where the *metadata blob* is injected, not what bindgen can see.

Consequences, each verified from scratch by
`docs/steps/artifacts/step-10-boltffi-visibility/probe.sh` (≈15 s, no Xcode, no NDK):

| where the `#[data]`/`#[export]` items live | rustc | `generate` | `pack` |
|---|---|---|---|
| hand-written in `src/lib.rs` | ✅ | ✅ | ✅ |
| emitted by a proc macro | ✅ | ❌ **silent** | ❌ |
| a committed `mod generated;` file | ✅ | ✅ | ❌ *until a root re-export* |
| `include!`d (i.e. from `OUT_DIR`) | ✅ | ❌ **silent** | ❌ |
| a dependency crate that depends on boltffi | ✅ | ✅ | ✅ |

**Three ways this toolchain hides a broken FFI surface**, all silent:
1. macro-emitted and `include!`d items are omitted, and `boltffi generate swift` exits **0**;
2. `generate` will emit Swift for Rust that does not compile (the two are independent);
3. a crate can pass `cargo build` *and* `generate` and still fail `pack`. Under
   `BOLTFFI_BINDING_EXPANSION=1` the **first** `#[data]`/`#[export]` item the compiler expands is
   replaced by a whole-crate metadata blob that resolves every exported type **from the crate root**.
   A crate whose classes live in `mod generated;` dies with `error[E0425]: cannot find type
   ProfileStoreFfi in this scope`, pointed at an unrelated `#[data]` twenty lines away. The fix is
   `pub use generated::*;` in `lib.rs`. **`mise run check` structurally cannot see this** — the blob
   only exists under `pack`'s environment.

Two smaller traps: `#[boltffi::ffi_stream(item = T)]` in **path form is silently not recognised** by
`#[export]` (write `use boltffi::*;` and a bare `#[ffi_stream]`), and BoltFFI derives native symbol
names from the crate name and **rejects hyphens** (package `gen_profile_ffi`, directory
`gen-profile-ffi`).

**How to apply:** never plan an FFI surface as macro output — it will compile, generate, and be absent.
This is why the FFI layer is generated *source*, committed and drift-checked (ARCHITECTURE D22), and
why `#[bolted::feature_model]` could never have existed. See [[thin-macros-push-behavior-into-the-core]]:
committed generated source gets rustc, `clippy -D warnings` and a review diff — three rungs of the
ladder that macro output gets none of.
