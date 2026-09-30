#!/usr/bin/env bash
# Wraps the release binary in a real .app so it double-clicks, gets a dock
# icon, and behaves like a Mac application instead of a terminal program.
set -euo pipefail
cd "$(dirname "$0")/.."

NAME="Loop"
APP="target/$NAME.app"

# Where a fresh install signs in before anyone has chosen a server: baked
# into the binary. Override for another deployment, or set it to a local
# server for a build that should never leave this Mac.
export LOOP_DEFAULT_SERVER="${LOOP_DEFAULT_SERVER:-https://api-1.mycohort.live/loop}"

# The version shown in the account menu and Info.plist. release-mac.sh sets it
# to the release's date stamp; a local build says "dev".
export LOOP_VERSION="${LOOP_VERSION:-dev}"
BUNDLE_VERSION=$(printf '%s' "$LOOP_VERSION" | tr -c '0-9' '.' | tr -s '.' | sed 's/^\.//; s/\.$//')
BUNDLE_VERSION=${BUNDLE_VERSION:-0}

# UNIVERSAL=1 builds for Intel as well and joins the two with lipo — only
# needed if someone on the team has an Intel Mac, and it doubles the build.
if [ "${UNIVERSAL:-0}" = "1" ]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  cargo build --release --features app --bin acp-app --target aarch64-apple-darwin
  cargo build --release --features app --bin acp-app --target x86_64-apple-darwin
  BIN=target/acp-app-universal
  lipo -create -output "$BIN" \
    target/aarch64-apple-darwin/release/acp-app \
    target/x86_64-apple-darwin/release/acp-app
else
  cargo build --release --features app --bin acp-app
  BIN=target/release/acp-app
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/acp-app"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>live.airtribe.controlplane</string>
  <key>CFBundleVersion</key><string>$BUNDLE_VERSION</string>
  <!-- No window restoration. The app has one window and rebuilds its state
       from the server on launch, so there is nothing to restore — and with it
       on, any hard kill (a rebuild, a crash) makes macOS greet the next
       launch with a "reopen its windows?" dialog that blocks the app. -->
  <key>NSQuitAlwaysKeepsWindows</key><false/>
  <key>CFBundleShortVersionString</key><string>$LOOP_VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>acp-app</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <!-- A window app, not a background agent. -->
  <key>LSUIElement</key><false/>
</dict>
</plist>
PLIST

# The real mark, on the app's own canvas colour, rounded like every other Mac
# icon. Generated from the same alpha mask the sidebar draws, so the two can
# never drift.
ICONSET=$(mktemp -d)/icon.iconset
mkdir -p "$ICONSET"
python3 scripts/make-icon.py "$ICONSET"
if command -v iconutil >/dev/null && ls "$ICONSET"/*.png >/dev/null 2>&1; then
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns" 2>/dev/null || true
fi

# Ad-hoc signature: without it macOS quarantines the unsigned bundle and the
# keychain entry is scoped to an unstable identity.
codesign --force --deep --sign - "$APP" 2>/dev/null || \
  echo "  (codesign unavailable; the app still runs)"

echo
echo "built  $APP"
echo "run    open '$APP'"

# Quit any running copy politely before the next launch. A `kill -9` leaves
# macOS believing the app crashed, which is what put a "reopen its windows?"
# dialog in front of every rebuild.
osascript -e "quit app \"$NAME\"" 2>/dev/null || true

# DIST=1 zips the bundle for handing to teammates. `ditto` rather than `zip`
# keeps the signature and the bundle's extended attributes intact.
if [ "${DIST:-0}" = "1" ]; then
  ZIP="target/Loop.zip"
  rm -f "$ZIP"
  ditto -c -k --keepParent "$APP" "$ZIP"
  echo "dist   $ZIP ($(du -h "$ZIP" | cut -f1)) — $(lipo -archs "$APP/Contents/MacOS/acp-app")"
fi
