#!/usr/bin/env bash
# Render every page of the app offscreen, against a seeded throwaway server.
#
# The app cannot be screenshotted by the tooling that builds it, so this is how
# a page gets looked at before it ships: a fresh `acp_render` database, a
# server on its own port, the fixture in tests/fixtures/render-seed.sql, and
# the real app drawn into PNGs under docs/design-mocks/render/pages/ at three
# widths. Nothing here touches the working database.
#
#   scripts/render-pages.sh                 every page
#   scripts/render-pages.sh task project    only shots whose name starts so
#
# RENDER_DB and RENDER_PORT let two renders run at once without sharing a
# database or a port.
set -euo pipefail
cd "$(dirname "$0")/.."

DB=${RENDER_DB:-acp_render}
URL=postgres://localhost:5433/$DB
PORT=${RENDER_PORT:-8091}

dropdb -p 5433 --if-exists "$DB"
createdb -p 5433 "$DB"

cargo build --quiet --release --bin acp-server --bin acp-admin
DATABASE_URL=$URL PORT=$PORT ./target/release/acp-server >/tmp/acp-render.log 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null || true' EXIT

# The server listens only after its migrations have run.
for _ in $(seq 1 50); do
  curl -sf -o /dev/null "localhost:$PORT/health" && break
  sleep 0.2
done

psql "$URL" -q -v ON_ERROR_STOP=1 -f tests/fixtures/render-seed.sql
TOKEN=$(DATABASE_URL=$URL ./target/release/acp-admin session anmol.srivastava@airtribe.live | tail -1)

mkdir -p docs/design-mocks/render/pages
ACP_RENDER_URL="http://localhost:$PORT" ACP_RENDER_TOKEN="$TOKEN" \
  cargo test --quiet --features app --test page_render -- --ignored --nocapture "$@"
