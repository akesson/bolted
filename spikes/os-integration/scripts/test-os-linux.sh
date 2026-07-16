#!/bin/sh
# Step 20 M1 — the contract on Linux (probe rows P1 + D1-D4).
#
# Machine-bound and docker-gated, never inside `check`. Builds the step-20 image, re-runs the
# spike crates' whole test surface (incl. step 18's probe matrix rows B/E/F and the values-only
# discipline tests) on linux/arm64, then measures row D with syncctl in the same container.
#
# Exit-code trap (inherited caution): docker's exit status is trusted nowhere below — every
# verdict is read out of captured output.
set -eu

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
IMAGE=bolted-step20
LOG=$(mktemp -t step20-linux)
ok() { echo "ok  - $1"; }
fail() {
    echo "FAIL - $1"
    echo "--- captured output tail ---"
    tail -30 "$LOG"
    exit 1
}

command -v docker >/dev/null 2>&1 || {
    echo "SKIP: docker not installed — this tier needs Docker Desktop (see step-20 doc)"
    exit 1
}
docker info >/dev/null 2>&1 || {
    echo "SKIP: docker daemon not running — start Docker Desktop and re-run"
    exit 1
}

echo "# building the step-20 image (cached after first run)"
docker build -q -t "$IMAGE" "$ROOT/spikes/os-integration/linux" >"$LOG" 2>&1 \
    || fail "image build"
ok "image built"

# The container writes build artifacts to target-linux/ (gitignored) so the host target/ stays
# host-only; the cargo registry cache rides a named volume across runs.
RUN="docker run --rm -v $ROOT:/work -w /work \
    -v bolted-step20-cargo:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/work/target-linux $IMAGE"

echo "# P1 — the spike crates' full test surface, on Linux"
$RUN cargo test -p syncd -p sync-wire -p sync-settings >"$LOG" 2>&1 \
    || fail "cargo test exited non-zero"
grep -q "FAILED" "$LOG" && fail "a test binary reported FAILED"
SUITES=$(grep -c "^test result: ok\." "$LOG") || true
PASSED=$(grep "^test result: ok\." "$LOG" \
    | sed 's/.*ok\. \([0-9]*\) passed.*/\1/' \
    | awk '{s+=$1} END {print s}')
# The floor pins the grep from both sides: fewer suites than the known binaries, or a paltry
# pass count, means the harness matched nothing (the vacuous-green trap).
[ "${SUITES:-0}" -ge 5 ] || fail "expected >=5 suite results, saw ${SUITES:-0}"
[ "${PASSED:-0}" -ge 20 ] || fail "expected >=20 tests passed, saw ${PASSED:-0}"
ok "P1: $SUITES suites, $PASSED tests passed on linux (unmodified sources)"

echo "# D1-D4 — row D from the Rust client, in-container (debug build, matching step 18)"
$RUN sh -c '
    set -eu
    cargo build -q -p syncd --bins
    target-linux/debug/syncd --socket /tmp/step20-d.sock >/dev/null 2>&1 &
    i=0
    while [ ! -S /tmp/step20-d.sock ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done
    target-linux/debug/syncctl /tmp/step20-d.sock latency 1000
' >"$LOG" 2>&1 || fail "latency run exited non-zero"
grep "p50_us" "$LOG" || fail "no latency lines in output"
D4=$(grep "^D4" "$LOG" | sed 's/.*p50_us=\([0-9.]*\).*/\1/')
# Kill bar 2: keystroke pair p50 > 1000 µs. awk because p50 is fractional.
echo "$D4" | awk '{exit !($1 <= 1000.0)}' || fail "kill bar 2: D4 p50=${D4}µs > 1000µs"
ok "D4 keystroke pair p50=${D4}µs (bar 1000µs)"

echo "# syncd stripped binary size (Linux, release)"
$RUN sh -c '
    set -eu
    cargo build -q -p syncd --release --bin syncd
    strip -o /tmp/syncd-stripped target-linux/release/syncd
    wc -c </tmp/syncd-stripped
' >"$LOG" 2>&1 || fail "release size build"
ok "syncd stripped (linux/arm64): $(tail -1 "$LOG" | tr -d ' ') bytes"

echo "# all M1 rows green"
