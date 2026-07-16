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

# Start from a clean session: a previous (possibly failed) run may have left the agent
# registered or the extension enabled — this script owns both states for its duration.
[ -x "$APP/Contents/MacOS/BoltedSyncApp" ] \
  && "$APP/Contents/MacOS/BoltedSyncApp" --daemon unregister >/dev/null 2>&1 || true
launchctl bootout "gui/$(id -u)/dev.bolted.sync.daemon" 2>/dev/null || true
pluginkit -e ignore -i "$APPEX_ID" 2>/dev/null || true
pkill -f "FinderBadges.appex" 2>/dev/null || true

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
  # And the SMAppService agent (S rows), if it got as far as registering.
  [ -x "$APP/Contents/MacOS/BoltedSyncApp" ] \
    && "$APP/Contents/MacOS/BoltedSyncApp" --daemon unregister >/dev/null 2>&1 || true
  launchctl bootout "gui/$(id -u)/dev.bolted.sync.daemon" 2>/dev/null || true
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

echo "== U rows: headless VM tests over the wire (each test spawns its own daemon) =="
BOLTED_SYNCD="$SYNCD" swift test --package-path "$PKG" 2>&1 | tee "$OUT/swift-test.log" | tail -5
# The exit-code trap: read the actual XCTest tally, not the wrapper's status.
grep -Eq "Executed [0-9]+ tests?, with 0 failures" "$OUT/swift-test.log" || {
  echo "U rows FAILED: XCTest tally missing or nonzero failures (see $OUT/swift-test.log)" >&2
  exit 1
}
grep -q "keystroke-to-state" "$OUT/swift-test.log" && grep "keystroke-to-state" "$OUT/swift-test.log"

echo "== the greppable rule: no constraint literals in Sources/ (planted control first) =="
# The vehicle's constraint numbers must never appear in shell source (they arrive as wire
# params). Bounded so timing constants (300ms) and decimals (0.15 opacity) don't false-positive.
LITERALS='(^|[^0-9_.])(30|120|1440|15)([^0-9_.]|$)'
PLANT="$PKG/Sources/BoltedSyncCore/PlantedControl.swift"
echo 'let plantedMaxLabel = 30' > "$PLANT"
if ! grep -rE "$LITERALS" "$PKG/Sources" >/dev/null; then
  rm -f "$PLANT"
  echo "grep control FAILED: the matcher missed a planted constraint literal" >&2
  exit 1
fi
rm -f "$PLANT"
if grep -rE "$LITERALS" "$PKG/Sources"; then
  echo "FAILED: a constraint literal appears in shell source (see matches above)" >&2
  exit 1
fi
echo "no-literals ok (matcher proven by the planted control)"

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
echo "== S rows: SMAppService owns the bundled daemon =="
# The manual group daemon yields the socket path to launchd.
kill "$GROUP_PID" 2>/dev/null || true
GROUP_PID=""
rm -f "$GDIR/syncd.sock"

APPBIN="$APP/Contents/MacOS/BoltedSyncApp"
LABEL="dev.bolted.sync.daemon"
DOMAIN="gui/$(id -u)"
daemon_pid() {
  launchctl print "$DOMAIN/$LABEL" 2>/dev/null | sed -n 's/^[[:space:]]*pid = \([0-9]*\)$/\1/p'
}

echo "== S1: the app registers its own agent (ceremony recorded verbatim) =="
"$APPBIN" --daemon register | tee "$OUT/s1.log"
if grep -q "daemon-status=requires_approval" "$OUT/s1.log"; then
  echo "S1 CEREMONY: the OS demands a Login Items approval before the agent may run." >&2
  echo "Approve 'Bolted Sync' under System Settings > General > Login Items, rerun." >&2
  exit 2
fi
grep -q "daemon-register=ok daemon-status=enabled" "$OUT/s1.log" \
  || { echo "S1 FAILED: register did not reach enabled (see above, verbatim)" >&2; exit 1; }
launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1 \
  || { echo "S1 FAILED: launchd does not know the label after register" >&2; exit 1; }
echo "S1 ok: SMAppService registered the bundled agent (status=enabled)"

echo "== S2: socket activation through the bundled plist =="
"$SYNCCTL" "$GDIR/syncd.sock" ping | tee "$OUT/s2.log"
grep -q "Pong" "$OUT/s2.log" || { echo "S2 FAILED: no pong via the launchd socket" >&2; exit 1; }
SPID="$(daemon_pid)"
[ -n "$SPID" ] || { echo "S2 FAILED: no daemon pid under the label" >&2; exit 1; }
# argv[0] is ProgramArguments[0] ("syncd"), so ps args can't prove WHICH binary launchd ran;
# the executable's txt descriptor can.
lsof -a -p "$SPID" -d txt 2>/dev/null | tee "$OUT/s2-ps.log"
grep -q "BoltedSync.app/Contents/MacOS/syncd" "$OUT/s2-ps.log" \
  || { echo "S2 FAILED: the spawned daemon is not the BUNDLED syncd" >&2; exit 1; }
echo "S2 ok: first connect spawned the bundled syncd (pid $SPID) via launchd"

echo "== S4a: crash-respawn under SMAppService ownership (A3 spot-check) =="
kill -9 "$SPID"
sleep 1
"$SYNCCTL" "$GDIR/syncd.sock" version | tee "$OUT/s4.log"
grep -q "Version { version: 0 }" "$OUT/s4.log" \
  || { echo "S4a FAILED: no fresh daemon after kill -9 (or state survived)" >&2; exit 1; }
RPID="$(daemon_pid)"
[ -n "$RPID" ] && [ "$RPID" != "$SPID" ] \
  || { echo "S4a FAILED: no distinct respawned pid" >&2; exit 1; }
echo "S4a ok: kill -9 -> next connect respawned (pid $SPID -> $RPID), state reset to v0"

echo "== S4b: single instance — a second manual bootstrap of the label, refusal verbatim =="
S4B_EXIT=0
launchctl bootstrap "$DOMAIN" "$APP/Contents/Library/LaunchAgents/$LABEL.plist" \
  > "$OUT/s4b.log" 2>&1 || S4B_EXIT=$?
cat "$OUT/s4b.log"
[ "$S4B_EXIT" -ne 0 ] \
  || { echo "S4b FAILED: a second bootstrap of the label was ACCEPTED" >&2; exit 1; }
echo "S4b ok: the label refused a second bootstrap (exit $S4B_EXIT)"

echo "== S3: unregister boots the agent out =="
"$APPBIN" --daemon unregister | tee "$OUT/s3.log"
grep -q "daemon-unregister=ok" "$OUT/s3.log" \
  || { echo "S3 FAILED: unregister refused (see above)" >&2; exit 1; }
if launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; then
  echo "S3 FAILED: the label survives unregister" >&2
  exit 1
fi
echo "S3 ok: unregister removed the agent from the domain"

echo
echo "== M4 rows: the integrated citizen (launchd-owned daemon + live appex) =="
# Re-register: from here the daemon exists only on demand, and the appex's 2 s reconnect loop
# is what summons it (connect -> socket activation -> spawn).
"$APPBIN" --daemon register >/dev/null
wait_for_line() { # $1=pattern $2=timeout-secs — matches only lines newer than $MARK
  i=0
  while [ "$i" -lt "$2" ]; do
    if tail -c +"$MARK" "$LOG" 2>/dev/null | grep -q "$1"; then return 0; fi
    i=$((i+1)); sleep 1
  done
  echo "M4 FAILED waiting for: $1" >&2
  tail -c +"$MARK" "$LOG" >&2 || true
  return 1
}
MARK="$(($(wc -c < "$LOG") + 1))"
wait_for_line "live-wire connected" 15
wait_for_line "watching folder=" 5
echo "M4 ok: the appex reconnected THROUGH socket activation (its connect spawned the daemon)"

echo "== M4/G4a: paused flips the badge state over the wire =="
MARK="$(($(wc -c < "$LOG") + 1))"
"$APPBIN" --drive toggle | tee "$OUT/m4-toggle.log"
grep -q "drive-toggle=toggled" "$OUT/m4-toggle.log" || { echo "G4a FAILED: toggle refused" >&2; exit 1; }
wait_for_line "paused=true" 10
echo "G4a ok: the appex observed the toggle (tick-then-fetch) and re-badges as paused"

echo "== M4/G4b: a canonical folder change re-points the watched directory =="
NEWDIR="$HOME/Library/Group Containers/$GROUP/watched-m4"
mkdir -p "$NEWDIR"
MARK="$(($(wc -c < "$LOG") + 1))"
"$APPBIN" --drive set-folder "$NEWDIR" | tee "$OUT/m4-folder.log"
grep -q "drive-set-folder=submitted" "$OUT/m4-folder.log" \
  || { echo "G4b FAILED: the driver's submit was refused" >&2; exit 1; }
wait_for_line "watching folder=$NEWDIR" 10
echo "G4b ok: observe-over-wire re-pointed FIFinderSyncController.directoryURLs"

echo "== M4c: kill -9 under attached surfaces — the topology self-heals =="
DPID="$(daemon_pid)"
[ -n "$DPID" ] || { echo "M4c FAILED: no daemon pid before the kill" >&2; exit 1; }
MARK="$(($(wc -c < "$LOG") + 1))"
kill -9 "$DPID"
wait_for_line "live-wire disconnected" 10
wait_for_line "live-wire connected" 15
NPID="$(daemon_pid)"
[ -n "$NPID" ] && [ "$NPID" != "$DPID" ] \
  || { echo "M4c FAILED: no respawned daemon behind the healed wire" >&2; exit 1; }
echo "M4c ok: kill -9 (pid $DPID) -> appex reconnect respawned the daemon (pid $NPID), badges live"

echo
echo "FINDER-CITIZEN VERDICT:"
echo "  G1 registered / G2 spawned / G3 reached + control refused."
echo "  U rows green (see the XCTest tally above)."
echo "  S1 register(enabled) / S2 socket-activated bundled daemon / S4 respawn+single / S3 unregister."
echo "  M4: reconnect-through-activation / G4 badge+folder follow canonical / kill -9 self-heal."
echo "(manual rows remaining: badge visuals + the G5 context-menu command — see the step-19 report protocol)"
