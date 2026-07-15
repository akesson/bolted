# bindgen silently ignores macro-generated FFI items (`#[data]`/`#[export]` from a macro)

**Reported against:** boltffi 0.27.3 · **Severity:** medium (silent — the worst failure mode) ·
**Disposition at 0.27.5: ALIVE.**

> **Upstream status (2026-07-15):** not filed standalone. Folded into RFC
> [boltffi/boltffi#665](https://github.com/boltffi/boltffi/issues/665) (per-invocation metadata
> capture), which attacks the root cause — the whole-crate source re-scan — and lists this
> symptom in its bug-family table as "(unfiled, repro to follow)". **TODO: file the standalone
> repro issue** (this repo's `probe.sh` is the repro) and link it back to #665.

## Summary

BoltFFI discovers its FFI surface by reading **source files off disk** and parsing them with `syn`
(`boltffi_scan::SourceTree::load`). It never sees macro-**expanded** code — not even under its own
`BINDING_EXPANSION` mode, which re-scans the same source text. A `#[data]` struct or `#[export]` impl
emitted by a procedural or declarative macro is therefore **absent** from the generated bindings, and
`boltffi generate` **exits 0**. Nothing warns.

## Repro (self-contained: `docs/steps/artifacts/step-10-boltffi-visibility/probe.sh`)

The probe builds a scratch workspace from scratch and prints a visibility table. One command,
cargo + the boltffi CLI only (no Xcode/NDK):

```
./docs/steps/artifacts/step-10-boltffi-visibility/probe.sh
```

## Re-verification at 0.27.5 (step 15 M4) — ALIVE (table identical to 0.27.3)

```
## 2. what does `boltffi generate swift` see?
   exit=0

| where the #[data]/#[export] items live         | rustc  | bindgen  |
|------------------------------------------------|--------|----------|
| hand-written in src/lib.rs                     | OK     | present  |
| emitted by a proc macro                        | OK     | MISSING  |
| a committed `mod generated;` file              | OK     | present  |
| include!d (i.e. from OUT_DIR)                  | OK     | MISSING  |
| a dependency crate that depends on boltffi     | OK     | present  |
```

Proc-macro-emitted and `include!`d items are still **MISSING**, and `boltffi generate swift` still
**exits 0 in silence** — no diagnostic naming the invisible items.

## Ask (either is sufficient)

1. **Warn** when a scanned source file contains a macro invocation in a position that could emit FFI
   items, so the author knows the expansion is invisible; or
2. **Scan expanded output** so macro-emitted `#[data]`/`#[export]` items are seen.

## Acceptance test

A `#[data]` struct emitted by a macro either appears in the generated bindings, or `boltffi generate`
emits a diagnostic naming the file and the invisible macro invocation. It must not exit 0 in silence.
