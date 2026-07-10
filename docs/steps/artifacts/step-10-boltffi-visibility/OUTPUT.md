# `probe.sh` — recorded output

boltffi 0.27.3, rustc 1.95.0, macOS 15 (arm64). Re-run with
`./docs/steps/artifacts/step-10-boltffi-visibility/probe.sh`.

```
## 1. does rustc accept all five?
    Finished `dev` profile [unoptimized + debuginfo] target(s)

## 2. what does `boltffi generate swift` see?
   exit=0

| where the #[data]/#[export] items live         | rustc  | bindgen  |
|------------------------------------------------|--------|----------|
| hand-written in src/lib.rs                     | OK     | present  |
| emitted by a proc macro                        | OK     | MISSING  |
| a committed `mod generated;` file              | OK     | present  |
| include!d (i.e. from OUT_DIR)                  | OK     | MISSING  |
| a dependency crate that depends on boltffi     | OK     | present  |

## 3. kill criteria
  KC1  #[export] impl + #[ffi_stream] from a non-root mod : NOT HIT (both generated)
  KC2  a CheckId enum across #[data] (D18)                : NOT HIT (param + return)

## 4. what `generate` sees is not what `pack` can build
  `boltffi pack` compiles under BOLTFFI_BINDING_EXPANSION, where the first #[data]/#[export]
  item becomes a whole-crate metadata blob that names exported types FROM THE CRATE ROOT.
  exported classes in a submodule, no re-export : FAILS to build (E0425)
  ...the same crate, plus a root re-export     : builds

## 5. the sting
  `boltffi generate` exits 0 and prints nothing for the two MISSING rows.
  A silently absent FFI surface is worse than a refusal to generate one.
  And a crate can be green on `mise run check` and on `generate`, and still not pack.
```

## Why

`boltffi_scan::SourceTree::load` → `std::fs::read_to_string` → `syn::parse_file`, walking `mod`
declarations. The FFI surface is discovered from **source text**, never from expanded Rust. The
`BINDING_EXPANSION_*` machinery that `mise run pack:android` works around does not change this: its
`Request::render()` calls `scan_package(ScanInput::new(&self.source, …))` and re-reads the same file
off disk. Expansion mode governs where the metadata blob is *emitted*, not what bindgen can *see*.

Consequences for the design, both load-bearing:

1. **`#[bolted::feature_model]` was never possible as an attribute macro.** D21 cut it for the wrong
   reason (*"`bolted-macros` may not import boltffi"* — a macro emitting `#[data]` tokens imports
   nothing). D22 replaces it with committed generated source plus a drift check.
2. **Shared `#[data]` types can live in a dependency crate.** Row 5 is what makes D24 (one
   `TextFieldState` for all `Raw = String` values, hosted in `bolted-ffi`) mechanically possible rather
   than merely desirable.

## Two things the probe found that the design did not ask about

- **`#[boltffi::ffi_stream(item = …)]` in path form is not recognised** by `#[export]`. The method is
  then typed as returning `Arc<EventSubscription<T>>` and fails to compile with a `WireEncode` bound
  error pointing at the `#[export]` attribute. Generated code must `use boltffi::*;` and write a bare
  `#[ffi_stream]`. Recorded because a generator that emits fully-qualified paths everywhere else has
  no reason to expect this one exception.

- **Bindgen will happily generate Swift for Rust that does not compile.** During the KC1 run, `cargo
  build` failed on the `WireEncode` bound above while `boltffi generate swift` produced a complete,
  well-formed `GenStore` class in the same tree. Source-scanning cuts both ways: the binding can
  describe code that does not exist (rows 2 and 4) *and* code that does not build. Nothing downstream
  of `generate` can tell the difference.

---

## Correction, found later in the step (and it cost an afternoon)

The table above answers *"what can `boltffi generate` see?"*. It does **not** answer *"what can
`boltffi pack` build?"*, and the answers differ.

`boltffi pack` compiles the crate with `BOLTFFI_BINDING_EXPANSION=1`. Under that flag, the **first**
`#[data]`/`#[export]` item the compiler expands is replaced by a whole-crate metadata blob
(`boltffi_macros::experimental::expansion_build::item`, guarded by a one-shot `AtomicBool`), and that
blob names every exported type **from the crate root** — wherever it happens to be injected.

So a crate whose exported classes live in `mod generated;` compiles, generates Swift, and then dies
during `pack` with:

```
error[E0425]: cannot find type `ProfileStoreFfi` in this scope
  --> crates/gen-profile-ffi/src/generated.rs:24:1
   |
24 | #[data]
   | ^^^^^^^
```

— pointing at a `#[data]` on an unrelated enum, twenty lines from anything called `ProfileStoreFfi`.
When the module order put `custom.rs` first, it pointed there instead: a `#[data]` in a file that has
never heard of `ProfileStoreFfi`.

**The fix is one line per crate**, and nothing but a comment records why:

```rust
pub mod generated;
pub use generated::*;   // load-bearing: `pack` resolves the metadata blob's names from the root
```

Reproduce without a 5-target release pack:

```sh
cd crates/gen-note-ffi
BOLTFFI_BINDING_EXPANSION=1 \
BOLTFFI_BINDING_EXPANSION_ROOT="$PWD" \
BOLTFFI_BINDING_EXPANSION_SOURCE="$PWD/src/lib.rs" \
BOLTFFI_BINDING_EXPANSION_SURFACE=native \
RUSTFLAGS="--cfg boltffi_binding_expansion" \
  cargo build -p gen_note_ffi
```

Two things worth carrying forward:

1. **`mise run check` structurally cannot see this.** The blob exists only under the pack's
   environment variable, so a crate can be green on `check`, green on `boltffi generate swift`, and
   still fail to pack. It is the third distinct way this toolchain lets a broken FFI surface look fine.
2. **`boltffi generate swift` and `boltffi pack apple` fight over `dist/apple/Sources/`.** `generate`
   writes `Sources/Foo.swift`; `pack` writes `Sources/BoltFFI/Foo.swift`. Run both and SwiftPM refuses
   the package with `multiple producers`. `mise run pack:apple:gen` therefore deletes `dist/apple`
   first.
