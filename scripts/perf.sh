#!/usr/bin/env bash
# Time every read endpoint the Mac app calls, at base and stress scale.
#
# A fresh `acp_perf` database per scale, seeded from tests/fixtures/scale-seed.sql,
# a release server on its own port, 50 sequential requests per endpoint.
# Prints p50/p95 (ms) and the response size. Leaves `acp_perf` at the last
# scale run in place and the server stopped.
#
#   scripts/perf.sh          scale 1 then 5
#   scripts/perf.sh 1        base only
#   N=200 scripts/perf.sh    more requests per endpoint
#   FRAMES=1 scripts/perf.sh also run tests/perf_frames.rs at each scale
set -euo pipefail
cd "$(dirname "$0")/.."

DB=${PERF_DB:-acp_perf}
URL=postgres://localhost:5433/$DB
PORT=${PERF_PORT:-8141}
N=${N:-50}
SCALES=${*:-1 5}

cargo build --quiet --release --bin acp-server --bin acp-admin
[ -n "${FRAMES:-}" ] && cargo test --quiet --release --features app --test perf_frames --no-run

P1=33333333-0000-0000-0000-000000000001
T1=44444444-0000-0000-0000-000000000001
ENDPOINTS=(
  /api/user/me
  /api/user/home
  /api/user/tasks
  /api/user/tasks/mine
  /api/user/projects
  /api/user/people
  /api/user/labels
  /api/user/tracks
  /api/user/projects/$P1
  /api/user/projects/$P1/flow
  "/api/user/tasks?projectId=$P1"
  "/api/user/artifacts?parentType=project&parentId=$P1"
  /api/user/tasks/$T1
  /api/user/tasks/$T1/notes
  "/api/user/artifacts?parentType=task&parentId=$T1"
)

SERVER=
trap '[ -n "$SERVER" ] && kill $SERVER 2>/dev/null || true' EXIT

for SCALE in $SCALES; do
  dropdb -p 5433 --if-exists "$DB"
  createdb -p 5433 "$DB"
  DATABASE_URL=$URL PORT=$PORT ./target/release/acp-server >/tmp/acp-perf.log 2>&1 &
  SERVER=$!
  for _ in $(seq 1 50); do
    curl -sf -o /dev/null "localhost:$PORT/health" && break
    sleep 0.2
  done
  psql "$URL" -q -v ON_ERROR_STOP=1 -v scale="$SCALE" -f tests/fixtures/scale-seed.sql >/dev/null
  TOKEN=$(DATABASE_URL=$URL ./target/release/acp-admin session anmol.srivastava@airtribe.live | tail -1)

  echo
  echo "scale=$SCALE  ($(psql "$URL" -tAc "select count(*) from task") tasks)  n=$N"
  printf '%-62s %8s %8s %10s\n' endpoint p50_ms p95_ms bytes
  for path in "${ENDPOINTS[@]}"; do
    for _ in 1 2 3; do curl -s -o /dev/null -H "Authorization: Bearer $TOKEN" "localhost:$PORT$path"; done
    samples=$(for _ in $(seq 1 "$N"); do
      curl -s -o /dev/null -w '%{time_total} %{size_download}\n' \
        -H "Authorization: Bearer $TOKEN" "localhost:$PORT$path"
    done)
    bytes=$(echo "$samples" | tail -1 | cut -d' ' -f2)
    echo "$samples" | cut -d' ' -f1 | sort -n | awk -v p="$path" -v b="$bytes" '
      { t[NR] = $1 * 1000 }
      END { printf "%-62s %8.1f %8.1f %10d\n", p, t[int(NR * 0.5 + 0.5)], t[int(NR * 0.95 + 0.5)], b }'
  done

  if [ -n "${FRAMES:-}" ]; then
    ACP_PERF_URL="http://localhost:$PORT" ACP_PERF_TOKEN="$TOKEN" \
      cargo test --quiet --release --features app --test perf_frames -- --ignored --nocapture --test-threads=1
  fi

  kill $SERVER; wait $SERVER 2>/dev/null || true; SERVER=
done
