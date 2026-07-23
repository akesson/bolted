# 08 — bindgen evaluates no `#[cfg]`: gated items join every target's surface

**Found 2026-07-21** during a crate-consolidation review (can the two `bolted-http-*-ffi`
bridge crates become one cfg-gated crate?). **Source-verified at released 0.28.0, and now
runtime-probed** — the probe ran 2026-07-23 (step-27 M0) and **confirmed the union claim**:
a `#[cfg(target_os = "ios")]`-gated `#[data]` struct in a scratch crate generated for the
android target lands in the Kotlin bindings as a real `data class`, exactly as inferred
below. Artifact + reproducer: [`docs/steps/artifacts/step-27-bindgen-cfg-probe/`](../../docs/steps/artifacts/step-27-bindgen-cfg-probe/)
(`probe.sh` + a two-`#[data]`-item scratch crate; `boltffi generate kotlin`, source scan, no
NDK, ~15 s).

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

## Consequence (probed 2026-07-23 — the union claim holds)

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

The step-27 probe demonstrated exactly this: the ios-gated struct emitted a `data class`
in the android Kotlin output (a `WireReader`/`fromByteArray` decoder and all), with
`generate` exiting 0. So the failure mode is the inferred one — a silent surface item with
no backing symbol — **not** an abort or a correct exclusion. For our merge that is the
benign direction: we chose the contract-side exit (declare the surface uniform, Q10), so no
gated item exists to leak in the first place.

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
