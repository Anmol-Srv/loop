#!/usr/bin/env bash
# Install Loop, or update it to the latest release. The same command does both:
#
#   curl -fsSL https://api-1.mycohort.live/loop/install.sh | bash
#
# It downloads the latest build from the Loop server itself (not GitHub, whose
# download hosts some networks block and which a private repo would lock),
# quits Loop if it is running, replaces /Applications/Loop.app and opens it. Your sign-in,
# server and theme live outside the app bundle, so an update keeps them.
#
# Downloaded with curl, the bundle carries no quarantine flag, so macOS opens
# it without the "unidentified developer" prompt a browser download triggers.
set -euo pipefail

# release-mac.sh rewrites this line to the server it uploads to.
SERVER="https://api-1.mycohort.live/loop"
# Each build has its own URL (Loop-<version>.zip), named by the small, uncached
# `latest` file: a CDN in front of the server caches .zip downloads, and a
# fixed Loop.zip would keep handing out the build it cached first.
VERSION_WANTED=$(curl -fsS -m 15 "$SERVER/download/latest" 2>/dev/null | tr -d '[:space:]' || true)
if [ -n "$VERSION_WANTED" ]; then
  URL="$SERVER/download/Loop-$VERSION_WANTED.zip"
else
  URL="$SERVER/download/Loop.zip"
fi
APP="/Applications/Loop.app"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Loop is a macOS app; this installer only runs on a Mac." >&2
  exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "Downloading Loop ${VERSION_WANTED:-(latest)}…"
curl -fL --progress-bar "$URL" -o "$TMP/Loop.zip"
ditto -x -k "$TMP/Loop.zip" "$TMP"
[ -d "$TMP/Loop.app" ] || { echo "The download did not contain Loop.app." >&2; exit 1; }

# Quit politely first: a killed app leaves macOS offering to reopen windows.
if pgrep -f "Loop.app/Contents/MacOS/acp-app" >/dev/null; then
  osascript -e 'quit app "Loop"' 2>/dev/null || true
  for _ in 1 2 3 4 5; do pgrep -f "Loop.app/Contents/MacOS/acp-app" >/dev/null || break; sleep 1; done
fi

# /Applications is writable for an admin user without sudo; fall back to sudo
# for anyone else rather than failing half way.
SUDO=""
[ -w /Applications ] || SUDO="sudo"
$SUDO rm -rf "$APP"
$SUDO ditto "$TMP/Loop.app" "$APP"
$SUDO xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP/Contents/Info.plist" 2>/dev/null || echo "?")
echo "Loop $VERSION installed. Opening it…"
open "$APP"
