#!/usr/bin/env bash
# Step 17 M4 — falsification of the wasm size budget tripwire.
#
# The doctrine: every new check is watched RED before it is trusted; a forbidding test that can
# forbid nothing is green forever. This script plants each failure the `wasm-budget check` gate is
# meant to catch and asserts the gate goes red (exit 1) with the right message, then confirms the
# real committed budget is green (exit 0). It also proves `mise run check` is indifferent to the
# budget's state — the host gate never reads dist or the budget file.
#
# Run from the repo root:  bash docs/steps/artifacts/step-17-falsification.sh
# Self-cleaning (temp budgets in a mktemp dir; any planted wasm is removed; the committed budget is
# restored from git). Exits non-zero if any assertion fails.
set -u

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"
DIST="crates/profile-web/dist"
REAL="crates/profile-web/wasm-budget.txt"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"; rm -f "$DIST"/planted-*_bg.wasm; git checkout -- "$REAL" 2>/dev/null || true' EXIT

RUN() { cargo run --release -q -p bolted-check --features budget --bin wasm-budget -- "$@"; }
pass=0; fail=0
expect() { # <label> <expected-exit> <actual-exit> <needle-in-out> <outfile>
  local label="$1" want="$2" got="$3" needle="$4" out="$5"
  if [ "$got" = "$want" ] && grep -qiF "$needle" "$out"; then
    echo "  PASS  $label (exit $got, message matched)"; pass=$((pass+1))
  else
    echo "  FAIL  $label (want exit $want got $got; needle '$needle')"; sed 's/^/        /' "$out"; fail=$((fail+1))
  fi
}

echo "== building a fresh release dist =="
( cd crates/profile-web && trunk build --release >/dev/null 2>&1 ) || { echo "trunk build failed"; exit 2; }

# The real measured baseline this budget was set from: wasm raw 327523, wire brotli 97703.
printf 'wasm_raw_max_bytes = 300000\nwire_brotli_max_bytes = 107520\nwasm_raw_min_bytes = 162816\n' > "$TMP/over.txt"
printf 'wasm_raw_max_bytes = 360448\nwire_brotli_max_bytes = 107520\nwasm_raw_min_bytes = 400000\n' > "$TMP/floor.txt"

echo "== 1) over-budget: max below measured -> RED =="
RUN check "$DIST" "$TMP/over.txt" >"$TMP/o1" 2>&1; expect "over-budget" 1 $? "exceeds budget" "$TMP/o1"

echo "== 2) under-floor: floor above measured -> RED =="
RUN check "$DIST" "$TMP/floor.txt" >"$TMP/o2" 2>&1; expect "under-floor" 1 $? "below the sanity floor" "$TMP/o2"

echo "== 3) missing dist -> RED (not a silent green) =="
RUN check "$TMP/no-such-dist" "$REAL" >"$TMP/o3" 2>&1; expect "missing-dist" 1 $? "No such file" "$TMP/o3"

echo "== 4) ambiguous dist: a second *_bg.wasm -> RED =="
cp "$(ls "$DIST"/*_bg.wasm | head -1)" "$DIST/planted-deadbeef_bg.wasm"
RUN check "$DIST" "$REAL" >"$TMP/o4" 2>&1; expect "ambiguous-dist" 1 $? "more than one" "$TMP/o4"
rm -f "$DIST"/planted-*_bg.wasm

echo "== 5) restore: real dist + real budget -> GREEN =="
RUN check "$DIST" "$REAL" >"$TMP/o5" 2>&1; expect "green-restore" 0 $? "budget OK" "$TMP/o5"

echo "== 6) mise run check is indifferent to the budget's state -> GREEN even when budget is broken =="
printf 'wasm_raw_max_bytes = 1\nwire_brotli_max_bytes = 1\nwasm_raw_min_bytes = 999999999\n' > "$REAL"
mise run check >"$TMP/o6" 2>&1; c6=$?
git checkout -- "$REAL"
if [ "$c6" = 0 ]; then echo "  PASS  check-indifferent (exit 0 with a broken budget present)"; pass=$((pass+1));
else echo "  FAIL  check-indifferent (exit $c6)"; fail=$((fail+1)); fi

echo
echo "== falsification summary: $pass passed, $fail failed =="
[ "$fail" = 0 ]
