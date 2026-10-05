#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

PORT=$(sed -n 's/^PORT=//p' .env 2>/dev/null)
PORT=${PORT:-8080}

cargo build --features app --bin acp-app
pkill -f 'target/debug/acp-app' || true
mkdir -p .dev-home
HOME="$PWD/.dev-home" ACP_URL="http://localhost:$PORT" nohup ./target/debug/acp-app > /tmp/acp-app.log 2>&1 &
echo "acp-app running against http://localhost:$PORT (log: /tmp/acp-app.log)"
