# `boltffi pack android` omits the binding-expansion environment → undefined JNI symbols

**Version:** boltffi 0.27.3 · **Severity:** high (`pack android` is unusable out of the box)

## Summary

`boltffi pack android --release` builds a `.so` whose `#[export]` symbols use **legacy short names**,
while the Kotlin/JNI glue and C header generated in the *same run* reference **crate-qualified long
names**. The library links with undefined symbols and ART fails at `System.loadLibrary`.

## Repro

Any crate with an `#[export]` impl. `boltffi pack android --release`, then load the `.so` on ART:

```
dlopen failed: cannot locate symbol
  "boltffi_register_callback_<crate>_<class>_<method>"
```

## Root cause (as far as we can see from the outside)

`pack apple` builds with the binding-expansion environment (the CLI's `build/expansion.rs` `env()`),
which switches the `#[export]` macro to emit crate-qualified symbols like
`boltffi_method_class_<crate>_<class>_<method>`. `pack android` (`pack/android/mod.rs`) passes
`env: Vec::new()`, so the macro falls back to legacy short names — but the generated glue/header still
expect the long names. The two halves of one `pack android` run disagree on the ABI.

## Expected

`pack android` builds the `.so` with the same binding-expansion environment `pack apple` uses, so the
emitted symbols match the glue that calls them.

## Workaround (this repo, `mise.toml` `pack:android`)

Replicate the Apple path's environment before `boltffi pack android`:

```sh
export BOLTFFI_BINDING_EXPANSION=1
export BOLTFFI_BINDING_EXPANSION_ROOT="$crate_dir"
export BOLTFFI_BINDING_EXPANSION_SOURCE="$crate_dir/src/lib.rs"
export BOLTFFI_BINDING_EXPANSION_SURFACE=native
export RUSTFLAGS="$RUSTFLAGS --cfg boltffi_binding_expansion"
```

## Acceptance test

**Deleting that block from `mise.toml` `pack:android` leaves `mise run test:android` green.** Today,
deleting it reproduces the `dlopen` failure above.
