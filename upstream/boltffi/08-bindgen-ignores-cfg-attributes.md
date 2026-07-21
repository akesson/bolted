# 08 — bindgen evaluates no `#[cfg]`: gated items join every target's surface

**Found 2026-07-21** during a crate-consolidation review (can the two `bolted-http-*-ffi`
bridge crates become one cfg-gated crate?). **Source-verified at released 0.28.0; no
runtime probe yet** — run one (step-10 `probe.sh` style, ~15 s) before filing upstream or
building anything on this.

## What the 0.28.0 source shows

The scan path for `generate`/`pack` is `boltffi_cli` → `boltffi_bindgen`, whose syn-based
source walker contains **no cfg handling at all**: the string `"cfg"` does not occur in
`boltffi_bindgen/src/` outside a `#[cfg(test)]` marker. `#[cfg(...)]` on a scanned item is
just an unrecognized attribute.

Meanwhile `boltffi_scan` 0.28.0 ships a **complete, tested cfg evaluator** —
`ActiveCfg` in `src/cfg.rs`: `all`/`any`/`not`, name and `target_os = "…"`-style value
predicates, `CARGO_FEATURE_*`/`CARGO_CFG_*` env ingestion, feature-name normalization
(`native-ffi` ⇄ `NATIVE_FFI`), unit tests. But it is **unwired**: `boltffi_scan` appears
only as a dev-dependency of `boltffi_backend` (tests). The machinery for cfg-aware
scanning exists upstream and nothing in the CLI path calls it.

## Consequence (inferred from the above, not yet probed)

With no cfg evaluation, the scanned surface is the **union of all items regardless of
target**. A crate with

```rust
#[cfg(target_os = "ios")]
#[data]
pub struct PriorityHint { /* … */ }
```

packed for Android gets Kotlin bindings for `PriorityHint` — whose native symbols the
`.so` does not export, because rustc *did* honor the cfg. Same silent family as note 03:
`generate` exits 0, breakage surfaces at link or run time.

## What this blocks for us, and the two exits

This is the load-bearing reason `bolted-http-apple-ffi` / `bolted-http-android-ffi` are
separate crates (step-26 M0 decision 1): their surfaces diverge (`PriorityHint` is
apple-only), one crate packing multiple targets is otherwise proven (`gen-profile-ffi`
packs apple+android+csharp), and cfg cannot express the divergence. Exits:

1. **Contract-side** — declare the surface uniform (freeze agenda Q10): a no-op hint on
   engines that can't honor it, and the bridge crates merge with no upstream change.
2. **Upstream-side** — wire `ActiveCfg` into the bindgen scan. Plausibly small given the
   evaluator is done; likely already tracked in the cfg-eval family (#630/#618) under
   RFC #665's re-scan umbrella. Check those before filing a duplicate.
