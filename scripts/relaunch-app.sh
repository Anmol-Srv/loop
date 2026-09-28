#!/usr/bin/env bash
# Rebuild everything and relaunch exactly one copy of the app, on the latest
# code: the server is rebuilt and restarted when its binary changed, and the
# app is installed to /Applications/Loop.app — a stable path, so the Dock icon
# always opens the newest build.
set -euo pipefail
cd "$(dirname "$0")/.."
INSTALLED="/Applications/Loop.app"

# Server: rebuild, restart only if the binary actually changed.
before=$(stat -f %m target/release/acp-server 2>/dev/null || echo 0)
cargo build --release --bin acp-server
after=$(stat -f %m target/release/acp-server)
if [ "$before" != "$after" ] || ! lsof -ti :8080 >/dev/null 2>&1; then
  pid=$(lsof -ti :8080 || true)
  [ -n "$pid" ] && kill $pid && for _ in 1 2 3 4 5; do lsof -ti :8080 >/dev/null || break; sleep 1; done
  (nohup ./target/release/acp-server >> /tmp/acp-server.log 2>&1 &)
  for _ in $(seq 1 20); do lsof -ti :8080 >/dev/null && break; sleep 0.5; done
  echo "server restarted"
fi

# App: quit every running copy (any bundle name or location), rebuild, install.
pkill -f "\.app/Contents/MacOS/acp-app" 2>/dev/null || true
for _ in 1 2 3 4 5; do pgrep -f "\.app/Contents/MacOS/acp-app" >/dev/null || break; sleep 1; done
./scripts/bundle-mac.sh >/dev/null
rm -rf "$INSTALLED"
cp -R target/Loop.app "$INSTALLED"
open -n "$INSTALLED"
echo "launched $INSTALLED"
