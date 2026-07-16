#!/bin/sh
# Step 18 M4 — the launchd lifecycle, scripted (probe rows A1–A4 + F1 across a real kill -9).
# Machine-bound AND session-bound: it bootstraps a LaunchAgent into THIS user's GUI domain and
# tears it down again. Run via `mise run test:os:launchd`.
#
# Every assertion reads actual output (the exit-code lesson); launchctl refusals are captured
# verbatim into the log because the refusal text IS probe evidence (A2).
set -eu

cd "$(dirname "$0")/../../.."
ROOT="$(pwd)"
LABEL="dev.bolted.syncd"
OUT="${TMPDIR:-/tmp}/bolted-launchd-probe"
mkdir -p "$OUT"
GDIR="$HOME/Library/Group Containers/bolted.os-spike.launchd"
SOCKET="$GDIR/syncd.sock"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
DOMAIN="gui/$(id -u)"

if ! command -v launchctl >/dev/null 2>&1; then
  echo "error: launchctl not found — this tier is macOS-only." >&2
  exit 1
fi

echo "== build =="
cargo build -q -p syncd
mkdir -p "$GDIR" "$HOME/Library/LaunchAgents"

cleanup() {
  launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
  rm -f "$PLIST"
}
trap cleanup EXIT
# A previous run may have left the agent loaded; start from a clean domain.
cleanup

# Stamp the plist (rung 3: generated data, no hand edits).
sed -e "s|@SYNCD@|$ROOT/target/debug/syncd|" \
    -e "s|@SOCKET@|$SOCKET|" \
    -e "s|@LOG@|$OUT/syncd.stderr.log|" \
  spikes/os-integration/scripts/dev.bolted.syncd.plist.tmpl > "$PLIST"

SYNCCTL="$ROOT/target/debug/syncctl"

daemon_pid() {
  launchctl print "$DOMAIN/$LABEL" 2>/dev/null | sed -n 's/^[[:space:]]*pid = \([0-9]*\)$/\1/p'
}

echo "== A1: bootstrap + socket activation =="
launchctl bootstrap "$DOMAIN" "$PLIST"
# No daemon should be running yet (no RunAtLoad): the first CONNECT is what spawns it.
PRE_PID="$(daemon_pid || true)"
[ -z "$PRE_PID" ] || echo "note: daemon already running before first connect (pid $PRE_PID)"
"$SYNCCTL" "$SOCKET" ping | tee "$OUT/a1.log"
grep -q "Pong" "$OUT/a1.log"
PID1="$(daemon_pid)"
[ -n "$PID1" ] || { echo "A1 FAILED: no pid after first connect" >&2; exit 1; }
echo "A1 ok: first connect spawned syncd (pid $PID1)"

echo "== A2: single instance — the second bootstrap's refusal, verbatim =="
A2_EXIT=0
launchctl bootstrap "$DOMAIN" "$PLIST" > "$OUT/a2.log" 2>&1 || A2_EXIT=$?
cat "$OUT/a2.log"
if [ "$A2_EXIT" -eq 0 ]; then
  echo "A2 FAILED: a second bootstrap of the same label was ACCEPTED (kill criterion 4)." >&2
  exit 1
fi
echo "A2 ok: second bootstrap refused (exit $A2_EXIT; text above is the evidence)"

echo "== A3 + F1 setup: state before the crash =="
"$SYNCCTL" "$SOCKET" toggle > "$OUT/a3-toggle.log"
grep -q "Toggled { paused: true }" "$OUT/a3-toggle.log"
"$SYNCCTL" "$SOCKET" version | grep -q "version: 1"
STASH="$("$SYNCCTL" "$SOCKET" f1-stash)"
echo "stash blob: $STASH"

echo "== A3: kill -9, then the next connect respawns a FRESH daemon =="
kill -9 "$PID1"
sleep 1
"$SYNCCTL" "$SOCKET" version | tee "$OUT/a3.log"
PID2="$(daemon_pid)"
[ -n "$PID2" ] && [ "$PID2" != "$PID1" ] || {
  echo "A3 FAILED: no respawn (pid1=$PID1 pid2=${PID2:-none})" >&2; exit 1; }
# All pre-crash state is gone: the version reset proves the store died with the process —
# which is exactly what makes H6/F1 matter.
grep -q "version: 0" "$OUT/a3.log" || {
  echo "A3 FAILED: version survived the kill -9?" >&2; exit 1; }
"$SYNCCTL" "$SOCKET" stats | grep -q "drafts: 0"
echo "A3 ok: respawned as pid $PID2, canonical version reset to 0, zero drafts"

echo "== F1: the stash outlives the daemon (H6) =="
"$SYNCCTL" "$SOCKET" f1-restore "$STASH" | tee "$OUT/f1.log"
grep -q "F1-OK" "$OUT/f1.log"

echo "== A4: idle-exit, and the NEXT connect still works =="
# The plist sets --idle-exit-secs 4; all our connections are one-shot, so just wait it out.
sleep 6
POST_IDLE_PID="$(daemon_pid || true)"
if [ -n "$POST_IDLE_PID" ]; then
  echo "A4 FAILED: daemon (pid $POST_IDLE_PID) still running after the idle window" >&2
  exit 1
fi
"$SYNCCTL" "$SOCKET" ping | grep -q "Pong"
PID3="$(daemon_pid)"
echo "A4 ok: idle-exited, and the next connect respawned (pid $PID3)"

echo
echo "LAUNCHD VERDICT: A1 socket-activation / A2 second-bootstrap refused / A3 crash-respawn"
echo "with full state loss / A4 idle-exit + respawn / F1 stash across daemon death — all green."
