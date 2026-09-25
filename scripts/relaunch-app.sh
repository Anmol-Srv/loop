#!/usr/bin/env bash
# Rebuild the app bundle and relaunch exactly one copy. Quits every running
# copy of this repo's app binary, whatever bundle name it was launched from
# (the app was renamed, and quitting by name left the old one open).
set -euo pipefail
cd "$(dirname "$0")/.."
pkill -f "$PWD/target/.*\.app/Contents/MacOS/acp-app" 2>/dev/null || true
for _ in 1 2 3 4 5; do pgrep -f "$PWD/target/.*\.app/Contents/MacOS/acp-app" >/dev/null || break; sleep 1; done
./scripts/bundle-mac.sh >/dev/null
open -n target/Loop.app
