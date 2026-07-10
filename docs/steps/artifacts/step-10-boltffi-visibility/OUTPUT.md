# `probe.sh` — recorded output

boltffi 0.27.3, rustc 1.95.0, macOS 15 (arm64). Re-run with
`./docs/steps/artifacts/step-10-boltffi-visibility/probe.sh`.

```
## 1. does rustc accept all five?
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.46s

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

## 4. the sting
  `boltffi generate` exits 0 and prints nothing for the two MISSING rows.
  A silently absent FFI surface is worse than a refusal to generate one.
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
