# bindgen silently ignores macro-generated FFI items (`#[data]`/`#[export]` from a macro)

**Version:** boltffi 0.27.3 · **Severity:** medium (silent, not loud — the worst failure mode)

## Summary

BoltFFI discovers its FFI surface by reading **source files off disk** and parsing them with `syn`
(`boltffi_scan::SourceTree::load`). It never sees macro-**expanded** code — not even under its own
`BINDING_EXPANSION` mode, which re-scans the same source text. A `#[data]` struct or `#[export]` impl
emitted by a procedural or declarative macro is therefore **absent** from the generated bindings, and
`boltffi generate` **exits 0**. Nothing warns.

## Repro (step 10)

```rust
macro_rules! emit_dto { () => {
    #[data]
    pub struct Emitted { pub x: u32 }
}; }
emit_dto!();   // `Emitted` is invisible to bindgen; the Swift/Kotlin type never appears.
```

`boltffi generate swift` succeeds and produces no `Emitted` type. A consumer referencing it fails to
compile *downstream*, far from the cause.

## Why it matters

It quietly forecloses the natural design — a `#[bolted::feature_model]`-style macro that stamps the
FFI surface — and forces every FFI item to be written as literal source (this repo generates the FFI
crate as committed source text for exactly this reason). A tool that silently drops your API surface
is worse than one that errors.

## Ask (either is sufficient)

1. **Warn** when a source file contains a macro invocation in a scanned position, so the author knows
   the expansion is invisible; or
2. **Scan expanded output** (e.g. via `cargo expand`-equivalent) so macro-emitted `#[data]`/`#[export]`
   items are seen.

## Acceptance test

A `#[data]` struct emitted by a macro either appears in the generated bindings, or `boltffi generate`
emits a diagnostic naming the file and the invisible macro invocation. It must not exit 0 in silence.

## Reference

`docs/steps/artifacts/step-10-boltffi-visibility/probe.sh` builds the visibility table from scratch.
