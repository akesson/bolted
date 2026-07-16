#!/bin/sh
# Step 19 — hand-assemble BoltedSync.app (+ embedded FinderBadges.appex) from SPM builds:
# no Xcode project (recon R1). This script IS a probe artifact: its length and its gotcha
# comments are the priced inventory of what `bolted new` scaffolding would have to emit for
# VISION's "a tray icon is a scaffold option" promise.
#
# Output: spikes/os-integration/apple/finder-citizen/dist/BoltedSync.app
set -eu

cd "$(dirname "$0")/../../.."
ROOT="$(pwd)"
PKG="$ROOT/spikes/os-integration/apple/finder-citizen"
RES="$PKG/Resources"
DIST="$PKG/dist"
APP="$DIST/BoltedSync.app"

if ! command -v swift >/dev/null 2>&1; then
  echo "error: swift toolchain not found — assembly needs Xcode (VISION risk 5)." >&2
  exit 1
fi

# Identity + team discovery — the step-18 convention (test-sandbox.sh): the app group must be
# prefixed by the team id the signing identity actually carries.
IDENTITY="${BOLTED_SIGN_IDENTITY:-$(security find-identity -v -p codesigning \
  | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
if [ -z "$IDENTITY" ]; then
  echo "error: no 'Developer ID Application' signing identity in the keychain." >&2
  echo "  Set BOLTED_SIGN_IDENTITY to override (step-19 pins the step-18 posture)." >&2
  exit 1
fi
TEAM_ID="$(printf '%s' "$IDENTITY" | sed -n 's/.*(\([A-Z0-9]*\))$/\1/p')"
GROUP="$TEAM_ID.dev.bolted.os-spike"
echo "identity: $IDENTITY"
echo "app group: $GROUP"

echo "== build (release) =="
swift build -c release --package-path "$PKG" >/dev/null
cargo build -q --release -p syncd
BIN="$PKG/.build/release"

echo "== layout =="
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" \
         "$APP/Contents/Library/LaunchAgents" \
         "$APP/Contents/PlugIns/FinderBadges.appex/Contents/MacOS"

# The committed plists/entitlements pin the owner's team id; re-stamp to the discovered one.
stamp() { sed "s/TKBX3BV5K6\.dev\.bolted\.os-spike/$GROUP/" "$1" > "$2"; }

cp "$BIN/BoltedSyncApp" "$APP/Contents/MacOS/BoltedSyncApp"
stamp "$RES/App-Info.plist" "$APP/Contents/Info.plist"

APPEX="$APP/Contents/PlugIns/FinderBadges.appex"
cp "$BIN/FinderBadges" "$APPEX/Contents/MacOS/FinderBadges"
stamp "$RES/Appex-Info.plist" "$APPEX/Contents/Info.plist"

# The bundled daemon + its SMAppService agent plist. launchd does not expand $HOME in
# SockPathName, so the per-user socket path is baked at assembly time (recorded friction).
cp "$ROOT/target/release/syncd" "$APP/Contents/MacOS/syncd"
sed "s|@SOCKET@|$HOME/Library/Group Containers/$GROUP/syncd.sock|" \
  "$RES/dev.bolted.sync.daemon.plist" \
  > "$APP/Contents/Library/LaunchAgents/dev.bolted.sync.daemon.plist"

echo "== sign (inside-out: appex, then app — never --deep) =="
APP_ENT="$DIST/app.entitlements"
APPEX_ENT="$DIST/appex.entitlements"
stamp "$RES/app.entitlements" "$APP_ENT"
stamp "$RES/appex.entitlements" "$APPEX_ENT"

codesign --force --options runtime \
  --entitlements "$APPEX_ENT" --sign "$IDENTITY" "$APPEX"
codesign --force --options runtime --identifier dev.bolted.sync.syncd \
  --sign "$IDENTITY" "$APP/Contents/MacOS/syncd"
codesign --force --options runtime \
  --entitlements "$APP_ENT" --sign "$IDENTITY" "$APP"

echo "== verify =="
codesign --verify --strict --verbose=2 "$APP"
echo "assembled: $APP"
