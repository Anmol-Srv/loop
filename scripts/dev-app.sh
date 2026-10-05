#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

[ -z "${PORT:-}" ] && [ -f .env ] && PORT=$(sed -n 's/^PORT=//p' .env)
PORT=${PORT:-8080}
APP="$PWD/target/debug/acp-app"

cargo build --features app --bin acp-app
pkill -f "^$APP" || true
while pgrep -f "^$APP" >/dev/null; do sleep 0.2; done
mkdir -p .dev-home
HOME="$PWD/.dev-home" ACP_URL="http://localhost:$PORT" nohup env -u ACP_TOKEN "$APP" > .dev-home/acp-app.log 2>&1 &
sleep 1
kill -0 $! 2>/dev/null || { echo "acp-app exited on launch; see .dev-home/acp-app.log" >&2; exit 1; }
echo "acp-app running against http://localhost:$PORT (log: .dev-home/acp-app.log)"
