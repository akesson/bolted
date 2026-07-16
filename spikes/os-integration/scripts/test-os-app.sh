#!/bin/sh
# Step 19 — the Finder-citizen probe rows, scripted. Machine-bound AND session-bound: it
# registers a Finder extension into this user's session (and boots it out on exit), and may
# relaunch Finder as a spawn nudge. Run via `mise run test:os:app`.
#
# M1 rows: G1 (pluginkit accepts the hand-assembled appex), G2 (Finder/pluginkit spawns it),
# G3 (the OS-spawned, OS-sandboxed process reaches the group-container socket) + the mandatory
# control (a live socket OUTSIDE the container must be refused — else G3 is vacuous).
#
# Every assertion greps actual output (the step-05/13 exit-code lesson).
set -eu

cd "$(dirname "$0")/../../.."
ROOT="$(pwd)"
PKG="$ROOT/spikes/os-integration/apple/finder-citizen"
APP="$PKG/dist/BoltedSync.app"
APPEX="$APP/Contents/PlugIns/FinderBadges.appex"
APPEX_ID="dev.bolted.sync.finderbadges"
OUT="${TMPDIR:-/tmp}/bolted-finder-citizen"
mkdir -p "$OUT"

echo "== assemble =="
sh "$ROOT/spikes/os-integration/scripts/assemble-app.sh"

# Group derivation must match assemble-app.sh (same discovery, same suffix).
IDENTITY="${BOLTED_SIGN_IDENTITY:-$(security find-identity -v -p codesigning \
  | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
TEAM_ID="$(printf '%s' "$IDENTITY" | sed -n 's/.*(\([A-Z0-9]*\))$/\1/p')"
GROUP="$TEAM_ID.dev.bolted.os-spike"
GDIR="$HOME/Library/Group Containers/$GROUP"
LOG="$GDIR/finder-badges.log"

echo "== daemons: group socket + the control socket outside the container =="
cargo build -q -p syncd
SYNCD="$ROOT/target/debug/syncd"
SYNCCTL="$ROOT/target/debug/syncctl"
GROUP_PID=""
CONTROL_PID=""
cleanup() {
  # Boot the extension back out of the session (registration state is the user's, not ours).
  pluginkit -e ignore -i "$APPEX_ID" 2>/dev/null || true
  pluginkit -r "$APPEX" 2>/dev/null || true
  pkill -f "FinderBadges.appex" 2>/dev/null || true
  [ -n "$GROUP_PID" ] && kill "$GROUP_PID" 2>/dev/null || true
  [ -n "$CONTROL_PID" ] && kill "$CONTROL_PID" 2>/dev/null || true
}
trap cleanup EXIT

start_daemon() { # $1=socket path; echoes pid
  rm -f "$1"
  # >/dev/null matters: called under command substitution, an inherited stdout pipe would
  # keep $(start_daemon …) blocked until the DAEMON exits, not the function.
  "$SYNCD" --socket "$1" >/dev/null 2>&1 &
  _pid=$!
  i=0
  while [ ! -S "$1" ] && [ "$i" -lt 50 ]; do i=$((i+1)); sleep 0.1; done
  [ -S "$1" ] || { echo "error: daemon never bound $1" >&2; exit 1; }
  echo "$_pid"
}

mkdir -p "$GDIR"
GROUP_PID="$(start_daemon "$GDIR/syncd.sock")"
# The control daemon: alive and answering, so a refusal can only be the extension sandbox.
CONTROL_PID="$(start_daemon "/tmp/bolted-g3-control.sock")"

echo "== fresh log =="
rm -f "$LOG"
pkill -f "FinderBadges.appex" 2>/dev/null || true

echo "== G1: register + enable the hand-assembled appex =="
pluginkit -a "$APPEX"
pluginkit -e use -i "$APPEX_ID"
pluginkit -m -v -i "$APPEX_ID" | tee "$OUT/g1.log"
grep -q "$APPEX_ID" "$OUT/g1.log" || { echo "G1 FAILED: pluginkit does not list the appex" >&2; exit 1; }
echo "G1 ok: pluginkit accepted the hand-assembled appex"

echo "== G2/G3: wait for the OS to spawn it (nudging Finder if needed) =="
wait_for_log() { # $1=timeout seconds
  i=0
  while [ "$i" -lt "$1" ]; do
    if [ -f "$LOG" ] && grep -q "G3-CONTROL" "$LOG"; then return 0; fi
    i=$((i+1)); sleep 1
  done
  return 1
}
if ! wait_for_log 15; then
  echo "(no spawn after 15s — relaunching Finder as the nudge; recorded as ceremony)"
  killall Finder 2>/dev/null || true
  wait_for_log 30 || { echo "G2 FAILED: extension never spawned (no log at $LOG)" >&2; exit 1; }
fi
cat "$LOG"

echo "== G2: the spawned process is the appex, not anything of ours =="
SPAWNED_PID="$(sed -n 's/^\([0-9]*\) spawned.*/\1/p' "$LOG" | tail -1)"
[ -n "$SPAWNED_PID" ] || { echo "G2 FAILED: no spawned line in the log" >&2; exit 1; }
ps -p "$SPAWNED_PID" -o args= | tee "$OUT/g2.log"
grep -q "FinderBadges.appex" "$OUT/g2.log" \
  || { echo "G2 FAILED: pid $SPAWNED_PID is not the appex binary" >&2; exit 1; }
echo "G2 ok: pid $SPAWNED_PID runs from inside the .appex bundle"

echo "== G3: the verdict =="
grep -q "G3 connect-ok" "$LOG" || {
  echo "G3: the OS-spawned extension did NOT reach the group socket — kill-1 territory." >&2
  echo "Refusal recorded verbatim in the log above; stop and report." >&2
  exit 1
}
grep -q "ping=pong" "$LOG" || { echo "G3 FAILED: connected but no pong round-trip" >&2; exit 1; }
# The control: refused outside the container, against a LIVE daemon.
grep -q "G3-CONTROL connect-refused errno=1" "$LOG" || {
  echo "G3 CONTROL FAILED: expected EPERM outside the container (sandbox not proven)." >&2
  exit 1
}
# Peer identity from outside: the appex pid holds a connection to the group socket.
lsof -a -p "$SPAWNED_PID" -U 2>/dev/null | tee "$OUT/g3-lsof.log" || true
grep -q "syncd.sock" "$OUT/g3-lsof.log" \
  || echo "(note: lsof shows no held socket — connection may have closed; log evidence stands)"
echo "G3 ok: OS-spawned + OS-sandboxed, and the wire is reachable"

echo
echo "FINDER-CITIZEN M1 VERDICT: G1 registered / G2 spawned / G3 reached + control refused."
