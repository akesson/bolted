#!/bin/sh
# Step 18 M3 — the sandbox verdict, scripted (probe rows C1/C2/C3 + the unsandboxed Codable
# proof). Machine-bound: needs Xcode's swift, a Developer ID Application identity, and a GUI
# session is NOT required. Run via `mise run test:os:sandbox`.
#
# Every assertion greps the probe's actual output (the step-05/step-13 exit-code lesson): the
# wrapper's exit status is never the only evidence.
set -eu

cd "$(dirname "$0")/../../.."
ROOT="$(pwd)"
PROBE_DIR="$ROOT/spikes/os-integration/apple/sync-probe"
OUT="${TMPDIR:-/tmp}/bolted-sandbox-probe"
mkdir -p "$OUT"

if ! command -v swift >/dev/null 2>&1; then
  echo "error: swift toolchain not found — the sandbox probe needs Xcode (VISION risk 5)." >&2
  exit 1
fi

# The signing identity: a Developer ID Application cert (the pinned distribution posture). The
# team id embedded in it must prefix the app group, so both are derived from what the keychain
# actually holds rather than hard-coded.
IDENTITY="${BOLTED_SIGN_IDENTITY:-$(security find-identity -v -p codesigning \
  | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
if [ -z "$IDENTITY" ]; then
  echo "error: no 'Developer ID Application' signing identity in the keychain." >&2
  echo "  The sandbox verdict needs a real identity (step-18 recon R4); ad-hoc is not the" >&2
  echo "  pinned posture. Set BOLTED_SIGN_IDENTITY to override." >&2
  exit 1
fi
TEAM_ID="$(printf '%s' "$IDENTITY" | sed -n 's/.*(\([A-Z0-9]*\))$/\1/p')"
GROUP="$TEAM_ID.dev.bolted.os-spike"
GDIR="$HOME/Library/Group Containers/$GROUP"
echo "identity: $IDENTITY"
echo "app group: $GROUP"

# The committed entitlements pin the owner's team id; regenerate to the discovered one so the
# script is honest on another maintainer's machine.
ENTITLEMENTS="$OUT/sandboxed.entitlements"
sed "s/TKBX3BV5K6\.dev\.bolted\.os-spike/$GROUP/" \
  "$PROBE_DIR/entitlements/sandboxed.entitlements" > "$ENTITLEMENTS"

echo "== build =="
swift build --package-path "$PROBE_DIR" >/dev/null
cargo build -q -p syncd
PROBE="$PROBE_DIR/.build/debug/SyncProbe"
SANDBOXED="$PROBE_DIR/.build/debug/SyncProbeSandboxed"
cp "$PROBE" "$SANDBOXED"
codesign --force --options runtime --identifier dev.bolted.sync-probe \
  --entitlements "$ENTITLEMENTS" --sign "$IDENTITY" "$SANDBOXED"

SYNCD="$ROOT/target/debug/syncd"
SYNCCTL="$ROOT/target/debug/syncctl"
DAEMON_PID=""
cleanup() { [ -n "$DAEMON_PID" ] && kill "$DAEMON_PID" 2>/dev/null || true; }
trap cleanup EXIT

start_daemon() {
  # A stale socket file would satisfy the bind-wait below before the daemon actually binds.
  rm -f "$1"
  "$SYNCD" --socket "$1" 2>/dev/null &
  DAEMON_PID=$!
  # Wait for the socket to exist rather than sleeping blind.
  i=0
  while [ ! -S "$1" ] && [ "$i" -lt 50 ]; do i=$((i+1)); sleep 0.1; done
  [ -S "$1" ] || { echo "error: daemon never bound $1" >&2; exit 1; }
}
stop_daemon() { kill "$DAEMON_PID" 2>/dev/null || true; wait "$DAEMON_PID" 2>/dev/null || true; DAEMON_PID=""; }

echo "== M3a: unsandboxed Codable proof (full cycle) =="
start_daemon "$OUT/cycle.sock"
"$PROBE" cycle "$OUT/cycle.sock" | tee "$OUT/cycle.log"
grep -q "CYCLE-OK" "$OUT/cycle.log"
stop_daemon

echo "== C2: control — the sandboxed client must be REFUSED outside the group container =="
start_daemon "$OUT/c2.sock"
C2_EXIT=0
"$SANDBOXED" connect "$OUT/c2.sock" > "$OUT/c2.log" 2>&1 || C2_EXIT=$?
cat "$OUT/c2.log"
stop_daemon
if [ "$C2_EXIT" -ne 2 ] || ! grep -q "connect-refused errno=1" "$OUT/c2.log"; then
  echo "C2 FAILED: expected an EPERM refusal proving the sandbox is on; without it C1 is" >&2
  echo "vacuous (the step-10 lesson). Got exit=$C2_EXIT." >&2
  exit 1
fi
echo "C2 ok: sandbox is provably on (EPERM outside the container)"

echo "== C1: the group-container socket, sandboxed =="
mkdir -p "$GDIR"
start_daemon "$GDIR/syncd.sock"
"$SANDBOXED" connect "$GDIR/syncd.sock" | tee "$OUT/c1.log"
grep -q "connect-ok pong" "$OUT/c1.log"
echo "C1 ok: sandboxed client reached the daemon through the app-group container"

echo "== C3: tick-then-fetch, sandboxed, change driven by the Rust client =="
( "$SANDBOXED" listen-toggle "$GDIR/syncd.sock" 15 > "$OUT/c3.log" 2>&1 ) &
LISTENER=$!
sleep 1
"$SYNCCTL" "$GDIR/syncd.sock" toggle >/dev/null
wait "$LISTENER"
cat "$OUT/c3.log"
grep -q "C3-OK" "$OUT/c3.log"
stop_daemon

echo
echo "SANDBOX VERDICT: C1 reached / C2 refused (EPERM) / C3 tick-then-fetch — all green."
