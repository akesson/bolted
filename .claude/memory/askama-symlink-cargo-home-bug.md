---
name: askama-symlink-cargo-home-bug
description: Upstream state of the askama 0.16 symlinked-CARGO_HOME build failure that breaks `cargo install boltffi_cli` (step-02 friction item 1)
metadata:
  type: reference
---

`cargo install boltffi_cli` fails on this machine because `~/.cargo` is a symlink into the
dotfiles repo and askama 0.16 (used by `boltffi_bindgen`) emits broken relative paths into
`include_bytes!` for `askama.toml`. Verified 2026-07-08 by reproduction (a scratch crate
depending on `boltffi_bindgen 0.27.3`: 77 errors with the symlink, clean with canonical
`CARGO_HOME`). Repo workaround: the `setup:boltffi` mise task (`cd -P` canonicalization).
Note: mise's rust tool exports `CARGO_HOME=$HOME/.cargo` (the symlinked spelling), which is
why mise's cargo backend hits the same bug.

Mechanism: `askama_derive` `read_config_file` (`config.rs:403`) **canonicalizes** the
`askama.toml` path but `caller_dir()` (from `Span::call_site().local_file()`) stays in the
symlinked spelling; the vendored lexical `diff_paths` then produces a `../..` chain that
crosses the symlink boundary and resolves to a nonexistent path.

Upstream (askama-rs/askama), as of 2026-07-08:
- Bug family: issue #704 (closed), fixed for *path deps* by PR #710 (0.15.5) and for
  *template files* by PR #720 (0.16.0, `canonicalize` → `std::path::absolute` in
  `find_template`).
- The `askama.toml` case was **missed**: `config.rs:403` still `canonicalize()`s on master
  (verified against raw master source). 0.16.0 is the latest release; no fixed version exists
  to bump to.
- Fix PR opportunity: one line at `config.rs:403` (`canonicalize()` → `absolute()`) + a
  symlink test mirroring `testing/tests/paths.rs`; reference #704/#720. Watch open PR #739
  (would remove `include_bytes!` entirely; contested). Maintainers (GuillaumeGomez, Kijewski)
  are responsive on this bug family.
- Not affected: askama ≤0.14 (emitted absolute canonical paths). Regression introduced by
  PR #546, first released in 0.15.0.

Once an askama release contains the fix and boltffi picks it up, `setup:boltffi`'s `cd -P`
workaround becomes a harmless no-op and can be simplified.
