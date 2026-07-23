#!/usr/bin/env bash
# Step 27, M0 — the note-08 runtime probe: does BoltFFI's bindgen evaluate #[cfg]?
#
# Note 08 (upstream/boltffi/08-bindgen-ignores-cfg-attributes.md) is SOURCE-verified: boltffi_bindgen
# has no cfg handling, so a #[cfg]-gated #[data] item should join every target's binding surface (the
# "union claim"). This runs it. The scratch crate beside this script has two #[data] items — one
# unconditional, one gated on target_os = "ios" — and is generated for the ANDROID/Kotlin target.
# If the ios-gated item lands in the Kotlin bindings, the union claim holds.
#
#   ./probe.sh          # run it, print the verdict
#   ./probe.sh --keep   # leave dist/android behind for inspection
#
# Requires: cargo + the pinned boltffi CLI (`mise run setup:boltffi`, boltffi_cli 0.28.0, no ?rev=).
# `boltffi generate kotlin` emits bindings from the source scan — NO NDK, no .so build (step-10
# `generate swift` precedent). ~15 s.
set -euo pipefail

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
command -v boltffi >/dev/null 2>&1 || { echo "error: boltffi CLI not found — run 'mise run setup:boltffi'." >&2; exit 1; }

# Precondition (memory: "askama symlinked-CARGO_HOME bug" / step-23 git-pin): the CLI must be the
# REGISTRY 0.28.0, never a ?rev= git build.
if ! boltffi --version 2>/dev/null | grep -q '0\.28\.0'; then
  echo "error: expected boltffi 0.28.0 — got '$(boltffi --version 2>/dev/null)'." >&2
  exit 1
fi
home="$(cd -P "${CARGO_HOME:-$HOME/.cargo}" 2>/dev/null && pwd || echo "${CARGO_HOME:-$HOME/.cargo}")"
if CARGO_HOME="$home" cargo install --list 2>/dev/null | grep -E '^boltffi_cli ' | grep -q '?rev='; then
  echo "error: boltffi_cli is a git (?rev=) build — reinstall the registry 0.28.0." >&2
  exit 1
fi

HERE="$(cd "$(dirname "$0")" && pwd)"
CRATE="$HERE/scratch"
KOTLIN="$CRATE/dist/android/kotlin"

echo "# probing in $CRATE"
rm -rf "$CRATE/dist"

echo
echo "## generate the Kotlin bindings for the android target (source scan; no .so)"
( cd "$CRATE" && boltffi generate kotlin; echo "   exit=$?" )

# The generated Kotlin lands under dist/android/kotlin/<package path>/*.kt. Find every .kt.
mapfile -t KTS < <(find "$KOTLIN" -name '*.kt' 2>/dev/null || true)
if [ "${#KTS[@]}" -eq 0 ]; then
  echo "error: no Kotlin bindings generated under $KOTLIN" >&2
  exit 1
fi

seen() { grep -qs "$1" "${KTS[@]}" && echo "present" || echo "MISSING"; }

echo
printf '| %-42s | %-8s |\n' "#[data] item" "in Kotlin"
printf '|%s|%s|\n' "--------------------------------------------" "----------"
printf '| %-42s | %-8s |\n' "AlwaysHere (unconditional control)"        "$(seen 'AlwaysHere')"
printf '| %-42s | %-8s |\n' "IosOnlyHint (#[cfg(target_os=\"ios\")])"    "$(seen 'IosOnlyHint')"

echo
echo "## verdict"
if [ "$(seen 'IosOnlyHint')" = "present" ]; then
  echo "  UNION CLAIM CONFIRMED: the ios-gated #[data] item appears in the android Kotlin bindings."
  echo "  bindgen ignores #[cfg]; the gated item joins every target's surface. Merge is safe."
else
  echo "  UNION CLAIM NOT CONFIRMED: the ios-gated item did NOT appear (cfg honoured, or generation"
  echo "  excluded it). Kill-criterion 1 territory — revisit note 08 before merging."
fi

[ "${1:-}" = "--keep" ] || rm -rf "$CRATE/dist"
