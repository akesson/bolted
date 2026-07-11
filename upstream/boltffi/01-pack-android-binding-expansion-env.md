# `boltffi pack android` omits the binding-expansion environment → undefined JNI symbols

**Reported against:** boltffi 0.27.3 · **Severity:** high · **Disposition at 0.27.5: RETIRED (fixed).**

## Original summary (0.27.3)

`boltffi pack android --release` built a `.so` whose `#[export]` symbols used **legacy short names**,
while the Kotlin/JNI glue and C header generated in the *same run* referenced **crate-qualified long
names**. The library linked with undefined symbols and ART failed at `System.loadLibrary`:

```
dlopen failed: cannot locate symbol "boltffi_register_callback_<crate>_<class>_<method>"
```

Root cause (external view): `pack apple` built with the binding-expansion environment (the CLI's
`build/expansion.rs` `env()`), switching `#[export]` to crate-qualified symbols; `pack android`
(`pack/android/mod.rs`) passed `env: Vec::new()`, so the macro fell back to short names while the glue
still expected the long ones. This repo carried a workaround that replicated the Apple path's env.

## Re-verification at 0.27.5 (step 15 M4)

**Fixed.** BoltFFI 0.27.4/0.27.5 (the Android JNI packaging fixes) now build the `.so` with
crate-qualified names matching the generated glue, with **no** workaround.

Red/green `nm` control on the same crate (`gen-profile-ffi`), packed **without** the workaround env:

| | `.so` exports (`nm -gU`) | JNI glue (`jni_glue.c`) expects | result |
|---|---|---|---|
| **0.27.3** | `boltffi_profile_store_ffi_checkout` (short) | `boltffi_method_class_gen_profile_ffi_generated_profile_store_ffi_checkout` (long) | mismatch → dlopen fails |
| **0.27.5** | `boltffi_method_class_gen_profile_ffi_generated_profile_store_ffi_checkout` (long) | same long name | **match** |

And the acceptance test as written passed: with the workaround block **deleted** from
`mise.toml` `pack:android`, `mise run test:android` ran **80 tests, 0 failures** (JUnit XML) on the
headless Gradle-managed emulator — the `.so` loaded cleanly.

**Action taken in this repo:** the workaround env block was removed from `pack:android`
(its comment already said "Drop this block when boltffi fixes pack android"). Nothing to file.

(One residual, cosmetic, not filed: the standalone C-API header `dist/android/include/gen_profile_ffi.h`
still declares the short names, while the JNI-path header `dist/android/kotlin/jni/gen_profile_ffi.h`
and the `.so` use the long names. The JNI load path — what ART uses — is self-consistent; a pure-C
consumer of the `include/` header would mismatch. Noted for completeness only.)
