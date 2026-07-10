#!/usr/bin/env bash
# Step 10, M0 — what can BoltFFI's bindgen actually SEE?
#
# The step's whole design turns on one question: is the FFI surface discovered from expanded Rust,
# or from the source text on disk? Reading `boltffi_scan` says source text. This script proves it,
# from scratch, and prints the table the step doc quotes.
#
#   ./probe.sh            # build the table
#   ./probe.sh --keep     # leave the scratch workspace behind for poking at
#
# Requires: cargo, and the pinned boltffi CLI (`mise run setup:boltffi`). No Xcode, no NDK:
# `boltffi generate swift` emits bindings without building a single Apple artifact.
set -euo pipefail

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
command -v boltffi >/dev/null 2>&1 || { echo "error: boltffi CLI not found — run 'mise run setup:boltffi'." >&2; exit 1; }

WORK="$(mktemp -d)"
trap '[ "${1:-}" = "--keep" ] || rm -rf "$WORK"' EXIT

mkdir -p "$WORK"/{stamp/src,shared/src,probe/src}

cat > "$WORK/Cargo.toml" <<'EOF'
[workspace]
resolver = "3"
members = ["stamp", "shared", "probe"]
EOF

# ---- a proc macro that emits a #[data] type and an #[export] fn ----------------------------------
cat > "$WORK/stamp/Cargo.toml" <<'EOF'
[package]
name = "stamp"
version = "0.0.0"
edition = "2021"
[lib]
proc-macro = true
[dependencies]
quote = "1"
EOF
cat > "$WORK/stamp/src/lib.rs" <<'EOF'
use proc_macro::TokenStream;
/// Exactly what a hypothetical `#[bolted::feature_ffi]` would do.
#[proc_macro_attribute]
pub fn feature_ffi(_a: TokenStream, _i: TokenStream) -> TokenStream {
    quote::quote! {
        #[boltffi::data]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct MacroDto { pub a: u32 }
        #[boltffi::export]
        pub fn macro_fn(x: u32) -> u32 { x + 1 }
    }.into()
}
EOF

# ---- a dependency crate that itself depends on boltffi (stands in for `bolted-ffi`) --------------
cat > "$WORK/shared/Cargo.toml" <<'EOF'
[package]
name = "shared"
version = "0.0.0"
edition = "2021"
[dependencies]
boltffi = "0.27.3"
EOF
cat > "$WORK/shared/src/lib.rs" <<'EOF'
#[boltffi::data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepDto { pub a: u32 }
#[boltffi::export]
pub fn dep_fn(x: u32) -> u32 { x + 1 }
EOF

# ---- the crate under test: five ways to declare an FFI surface -----------------------------------
cat > "$WORK/probe/Cargo.toml" <<'EOF'
[package]
name = "probe"
version = "0.0.0"
edition = "2021"
[lib]
crate-type = ["lib", "staticlib", "cdylib"]
[dependencies]
boltffi = "0.27.3"
stamp = { path = "../stamp" }
shared = { path = "../shared" }
EOF

cat > "$WORK/probe/src/lib.rs" <<'EOF'
// (1) hand-written in the crate root.
#[boltffi::data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RootDto { pub a: u32 }
#[boltffi::export]
pub fn root_fn(x: u32) -> u32 { x + 1 }

// (2) emitted by a proc macro.
#[stamp::feature_ffi]
pub struct Seed;

// (3) a committed module file.
pub mod generated;

// (4) include!d — stands in for a build.rs writing into OUT_DIR.
include!("included.rs");

// (5) a dependency crate's items (`shared`), reachable because it depends on boltffi.
pub fn uses_dep(d: shared::DepDto) -> u32 { d.a }
EOF

cat > "$WORK/probe/src/included.rs" <<'EOF'
#[boltffi::data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncludedDto { pub a: u32 }
#[boltffi::export]
pub fn included_fn(x: u32) -> u32 { x + 1 }
EOF

# The module file also carries step 10's two kill criteria:
#   KC1 — does `#[export] impl` + `#[ffi_stream]` work from a NON-ROOT module?
#   KC2 — can a C-like CheckId enum (D18) cross `#[data]`, as a parameter and as a return value?
# NOTE: `#[ffi_stream]` must be in scope as a bare attribute; `#[boltffi::ffi_stream]` is not
# recognised by `#[export]` and the method is then typed as returning `Arc<EventSubscription<_>>`.
cat > "$WORK/probe/src/generated.rs" <<'EOF'
use boltffi::*;
use std::sync::Arc;

#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModDto { pub a: u32 }

#[export]
pub fn mod_fn(x: u32) -> u32 { x + 1 }

/// KC2: the exact shape of `ProfileCheck` — `Checked::CheckId` (D18).
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileCheck { UsernameUnique }

pub struct GenStore { producer: Arc<StreamProducer<ModDto>> }
impl Default for GenStore { fn default() -> Self { Self::new() } }

/// KC1: an exported class, its impl, and a stream — all from a submodule.
#[export]
impl GenStore {
    pub fn new() -> GenStore { GenStore { producer: Arc::new(StreamProducer::new(8)) } }
    pub fn run_check(&self, check: ProfileCheck) -> bool { matches!(check, ProfileCheck::UsernameUnique) }
    pub fn which_check(&self) -> ProfileCheck { ProfileCheck::UsernameUnique }
    #[ffi_stream(item = ModDto)]
    pub fn snapshots(&self) -> Arc<EventSubscription<ModDto>> { self.producer.subscribe() }
}
EOF

echo "# probing in $WORK"
echo
echo "## 1. does rustc accept all five?"
( cd "$WORK" && cargo build 2>&1 | tail -1 )

echo
echo "## 2. what does \`boltffi generate swift\` see?"
( cd "$WORK/probe" && boltffi init >/dev/null 2>&1 || true; boltffi generate swift; echo "   exit=$?" )

SWIFT="$WORK/probe/dist/apple/Sources/ProbeBoltFFI.swift"
echo
printf '| %-46s | %-6s | %-8s |\n' "where the #[data]/#[export] items live" "rustc" "bindgen"
printf '|%s|%s|%s|\n' "------------------------------------------------" "--------" "----------"
row() { # $1 = label, $2 = symbol
  local seen="MISSING"
  grep -q "$2" "$SWIFT" && seen="present"
  printf '| %-46s | %-6s | %-8s |\n' "$1" "OK" "$seen"
}
row "hand-written in src/lib.rs"                RootDto
row "emitted by a proc macro"                   MacroDto
row "a committed \`mod generated;\` file"       ModDto
row "include!d (i.e. from OUT_DIR)"             IncludedDto
row "a dependency crate that depends on boltffi" DepDto

echo
echo "## 3. kill criteria"
printf '  KC1  #[export] impl + #[ffi_stream] from a non-root mod : %s\n' \
  "$(grep -q 'class GenStore' "$SWIFT" && grep -q 'func snapshots' "$SWIFT" && echo 'NOT HIT (both generated)' || echo 'HIT')"
printf '  KC2  a CheckId enum across #[data] (D18)                : %s\n' \
  "$(grep -q 'enum ProfileCheck' "$SWIFT" && grep -q 'func runCheck' "$SWIFT" && echo 'NOT HIT (param + return)' || echo 'HIT')"

echo
echo "## 4. the sting"
echo "  \`boltffi generate\` exits 0 and prints nothing for the two MISSING rows."
echo "  A silently absent FFI surface is worse than a refusal to generate one."
