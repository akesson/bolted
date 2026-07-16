#!/bin/sh
# Step 20 M2 — the systemd lifecycle rows (L1-L5), inside a systemd-PID-1 container.
#
# Machine-bound and docker-gated, never inside `check`. R1's empirical test is the container
# boot itself: if systemd will not run as PID 1 under this Docker, the refusal is recorded and
# the L rows are honestly not-executed (M1 stands alone).
#
# Exit-code trap: every verdict below reads command output, not wrapper exit statuses.
set -eu

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
IMAGE=bolted-step20
NAME=bolted-step20-systemd
LOG=$(mktemp -t step20-systemd)
X="docker exec $NAME"

ok() { echo "ok  - $1"; }
fail() {
    echo "FAIL - $1"
    echo "--- captured output tail ---"
    tail -30 "$LOG"
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    exit 1
}
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

command -v docker >/dev/null 2>&1 || {
    echo "SKIP: docker not installed"
    exit 1
}
docker info >/dev/null 2>&1 || {
    echo "SKIP: docker daemon not running"
    exit 1
}

echo "# R1 — boot systemd as PID 1 (the container ceremony IS the probe)"
docker build -q -t "$IMAGE" "$ROOT/spikes/os-integration/linux" >"$LOG" 2>&1 \
    || fail "image build"
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --rm --privileged --name "$NAME" \
    -v "$ROOT":/work -w /work \
    -v bolted-step20-cargo:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/work/target-linux \
    "$IMAGE" /lib/systemd/systemd >"$LOG" 2>&1 || fail "R1: container refused to start"
# Poll until the boot settles — systemctl cannot even reach the bus in the first moments, so
# `--wait` alone races. "degraded" is fine (units we don't care about may fail in a
# container); "running" is fine; a hung boot is not.
STATE=""
i=0
while [ "$i" -lt 60 ]; do
    STATE=$($X systemctl is-system-running 2>>"$LOG") || true
    case "$STATE" in running | degraded) break ;; esac
    sleep 0.5
    i=$((i + 1))
done
case "$STATE" in
running | degraded) ok "R1: systemd PID 1 is up (state: $STATE, after $((i / 2))s)" ;;
*) fail "R1: systemd state '$STATE' after 30s" ;;
esac
$X systemctl --version | head -1

echo "# install: build syncd in-container, place binary + units"
$X cargo build -q -p syncd --bins >"$LOG" 2>&1 || fail "in-container build"
$X cp target-linux/debug/syncd /usr/local/bin/syncd
$X cp target-linux/debug/syncctl /usr/local/bin/syncctl
$X cp spikes/os-integration/linux/syncd.socket spikes/os-integration/linux/syncd.service \
    /etc/systemd/system/
$X systemctl daemon-reload
$X systemctl start syncd.socket
$X test -S /run/syncd.sock || fail "socket unit did not bind /run/syncd.sock"
ok "socket unit listening on /run/syncd.sock"

echo "# L1 — socket activation: first connect spawns the service"
$X systemctl is-active syncd.service >"$LOG" 2>&1 && fail "service active before any connect"
PONG=$($X syncctl /run/syncd.sock ping 2>>"$LOG") || fail "L1: ping failed"
echo "$PONG" | grep -qi "pong" || fail "L1: expected pong, got: $PONG"
$X systemctl is-active --quiet syncd.service || fail "L1: service not active after connect"
PID1=$($X systemctl show -p MainPID --value syncd.service)
ok "L1: connect spawned syncd.service (MainPID=$PID1)"

echo "# L2 — single instance: unit identity, and a stray manual daemon is refused"
$X systemctl start syncd.service # second start: same unit, must be a no-op
PID2=$($X systemctl show -p MainPID --value syncd.service)
[ "$PID1" = "$PID2" ] || fail "L2: second start changed MainPID $PID1 -> $PID2"
STRAY=$($X /usr/local/bin/syncd --systemd 2>&1) && fail "L2: stray syncd --systemd started"
echo "$STRAY" | grep -q "not socket-activated" || fail "L2: unexpected stray refusal: $STRAY"
ok "L2: second start is a no-op (MainPID stable); stray daemon refused ('not socket-activated')"

echo "# L3 — crash-respawn + the backlog window (the open-then-verify re-check)"
$X syncctl /run/syncd.sock toggle >/dev/null # bump state so the reset is observable
V_BEFORE=$($X syncctl /run/syncd.sock version)
$X kill -9 "$PID2"
# Immediately connect: the socket unit still holds the listener. Time the full
# connect->request->respawn->accept->pong path.
T0=$($X date +%s%N)
PONG=$($X syncctl /run/syncd.sock ping 2>>"$LOG") || fail "L3: post-kill ping failed"
T1=$($X date +%s%N)
echo "$PONG" | grep -qi "pong" || fail "L3: expected pong, got: $PONG"
PID3=$($X systemctl show -p MainPID --value syncd.service)
[ "$PID3" != "$PID2" ] && [ "$PID3" != "0" ] || fail "L3: no respawn (MainPID=$PID3)"
V_AFTER=$($X syncctl /run/syncd.sock version)
[ "$V_BEFORE" != "$V_AFTER" ] || fail "L3: version survived kill -9 — state not fresh?"
ok "L3: kill -9 -> queued connect accepted by respawned daemon (pid $PID2 -> $PID3) in $(((T1 - T0) / 1000000)) ms; state reset ($V_BEFORE -> $V_AFTER)"

echo "# L4 — idle-exit, then reactivation through the socket unit"
$X mkdir -p /etc/systemd/system/syncd.service.d
$X sh -c 'printf "[Service]\nEnvironment=\"SYNCD_ARGS=--idle-exit-secs 2\"\n" > /etc/systemd/system/syncd.service.d/idle.conf'
$X systemctl daemon-reload
$X systemctl stop syncd.service
$X syncctl /run/syncd.sock ping >/dev/null || fail "L4: activation with idle-exit failed"
sleep 4
$X systemctl is-active --quiet syncd.service && fail "L4: service still active after idle window"
$X test -S /run/syncd.sock || fail "L4: socket unit gone after idle-exit"
PONG=$($X syncctl /run/syncd.sock ping 2>>"$LOG") || fail "L4: reactivation ping failed"
echo "$PONG" | grep -qi "pong" || fail "L4: expected pong, got: $PONG"
ok "L4: idle-exit fired, next connect respawned through the socket unit"

echo "# L5 — H6: the stash survives a daemon kill -9 (C20/C21 shape asserted by syncctl)"
BLOB=$($X syncctl /run/syncd.sock f1-stash) || fail "L5: f1-stash failed"
PID=$($X systemctl show -p MainPID --value syncd.service)
$X kill -9 "$PID"
# The blob is one JSON line; docker exec without -i has no stdin, so it travels as an argv.
$X syncctl /run/syncd.sock f1-restore "$BLOB" >"$LOG" 2>&1 \
    || fail "L5: f1-restore failed"
ok "L5: stash restored into the respawned daemon; C20/C21 assertions held"

echo "# R2 observation — who unlinks the socket file"
$X systemctl stop syncd.socket
if $X test -S /run/syncd.sock; then
    echo "note- socket FILE SURVIVES 'systemctl stop syncd.socket' (record in report)"
else
    ok "R2: systemd unlinked /run/syncd.sock on socket-unit stop"
fi

echo "# all M2 rows green"
