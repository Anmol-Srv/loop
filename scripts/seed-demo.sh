#!/usr/bin/env bash
# Loads the control plane with a realistic slice: two projects, four people
# in different departments, blocked work, an agent mid-flight, and an agent
# waiting on a human. Safe to re-run — it truncates first.
set -euo pipefail
cd "$(dirname "$0")/.."
# The server and database it uses: PORT and DATABASE_URL from the environment or .env.
[ -z "${PORT:-}" ] && [ -f .env ] && PORT=$(sed -n 's/^PORT=//p' .env)
[ -z "${DATABASE_URL:-}" ] && [ -f .env ] && DATABASE_URL=$(sed -n 's/^DATABASE_URL=//p' .env)
BASE=${ACP_URL:-http://localhost:${PORT:-8080}}
export PGOPTIONS='-c client_min_messages=warning'
PSQL="psql -q ${DATABASE_URL:-postgres://localhost:5432/acp_dev}"

# Fail before wiping anything if the server or the admin binary is not ready.
cargo build -q --bin acp-admin
curl -fsS "$BASE/health/ready" >/dev/null || { echo "no server at $BASE; start it with: cargo run --bin acp-server" >&2; exit 1; }

$PSQL -qc "TRUNCATE artifact, change, run_log_line, task, phase, project, job, setup_code, credential, person, label CASCADE;"

adm() { ./target/debug/acp-admin "$@"; }
adm bootstrap-admin anmol@airtribe.live Anmol >/dev/null 2>&1 || true
adm add-person pratik@airtribe.live Pratik >/dev/null
adm add-person evana@airtribe.live Evana >/dev/null
adm add-person chinmay@airtribe.live Chinmay >/dev/null

adm set-department anmol@airtribe.live backend    >/dev/null
adm set-department pratik@airtribe.live design    >/dev/null
adm set-department evana@airtribe.live design     >/dev/null
adm set-department chinmay@airtribe.live frontend >/dev/null

# A local-only password, so the app can sign in after every re-seed.
PASSWORD=12345678
for who in anmol pratik evana chinmay; do adm set-password "$who@airtribe.live" "$PASSWORD" >/dev/null; done

H=$(adm session anmol@airtribe.live | sed -n 2p)
A=$(adm mint hermes --owner anmol@airtribe.live --runtime hermes | sed -n 's/^Token: \([0-9a-f]*\).*/\1/p')
AGENT=$($PSQL -tAc "SELECT id FROM agent WHERE handle='hermes'")

api() {
  local out
  out=$(curl -sS -X "$1" "$BASE$2" -H "authorization: Bearer $3" -H 'content-type: application/json' ${4:+-d "$4"})
  case $out in *'"success":true'*) printf '%s' "$out" ;; *) echo "$1 $2 failed: $out" >&2; return 1 ;; esac
}
ent() { python3 -c 'import sys,json;print(json.load(sys.stdin)["data"]["entity"]["id"])'; }
# A project comes with an empty first phase; seed that one rather than leave it.
phase() { # project-id  name  position
  if [ "$3" = 0 ]; then
    local id
    id=$($PSQL -tAc "UPDATE phase SET name='$2' WHERE project_id='$1' AND position=0 RETURNING id" | head -1)
    [ -n "$id" ] || { echo "project $1 has no phase at position 0" >&2; return 1; }
    echo "$id"
  else
    api POST "/api/user/projects/$1/phases" "$H" "{\"name\":\"$2\",\"position\":$3}" | ent
  fi
}

# ---- project one: this tool, mid-flight
P1=$(api POST /api/user/projects "$H" '{"key":"loop","name":"Airtribe Control Plane"}' | ent)
i=0; declare -a PH
for spec in "Backbone|done" "Core and CLI|done" "MCP and approval|done" "Delegation|done" "Pages and design|active"; do
  n=${spec%|*}; st=${spec#*|}
  id=$(phase "$P1" "$n" $i)
  api PATCH "/api/user/phases/$id" "$H" "{\"status\":\"$st\"}" >/dev/null
  PH[$i]=$id; i=$((i+1))
done

# A task's track follows its assignee's department, so assign before moving.
mk() { # phase-id  title  assignee-email  status
  local id
  id=$(api POST "/api/user/phases/$1/tasks" "$H" "{\"title\":\"$2\"}" | ent) || return 1
  [ -z "$3" ] || api POST "/api/user/tasks/$id/assign" "$H" "{\"personEmail\":\"$3\"}" >/dev/null || return 1
  [ "$4" = open ] || api PATCH "/api/user/tasks/$id" "$H" "{\"status\":\"$4\"}" >/dev/null || return 1
  echo "$id"
}
task() { mk "${PH[$1]}" "$2" "$3" "$4"; }

ANMOL=anmol@airtribe.live PRATIK=pratik@airtribe.live EVANA=evana@airtribe.live CHINMAY=chinmay@airtribe.live

task 0 "Schema and migrations"      "$ANMOL"   shipped >/dev/null
task 0 "The change write path"      "$ANMOL"   shipped >/dev/null
task 1 "Scoped bearer tokens"       "$ANMOL"   shipped >/dev/null
task 1 "The acp CLI"                "$ANMOL"   shipped >/dev/null
task 2 "Proposals must not mutate"  "$ANMOL"   shipped >/dev/null
T_MCP=$(task 2 "Scope-filtered MCP tools" "$ANMOL"   shipped)
task 3 "Claim-lease distribution"   "$ANMOL"   shipped >/dev/null
task 3 "Run logs"                   "$ANMOL"   completed >/dev/null

T_TOKENS=$(task 4 "Design tokens and theme"   "$PRATIK"  handoff)
task 4 "Home page"                  "$CHINMAY" in_progress >/dev/null
T_TRACK=$(task 4 "Project tracker"            ""         open)
T_EMPTY=$(task 4 "Empty state illustrations"  "$EVANA"   research)
T_IDS=$(task   4 "Task by id endpoint"        "$ANMOL"   open)
T_RATE=$(task  4 "Rate limit the MCP surface" "$ANMOL"   open)
task 4 "Task detail live log"       "$CHINMAY" open >/dev/null

# the tracker waits on the empty states — the flow the board must show
api PATCH "/api/user/tasks/$T_TRACK/blockers" "$H" "{\"blockedBy\":[\"$T_EMPTY\"]}" >/dev/null

api POST "/api/user/artifacts" "$H" \
  "{\"parentType\":\"task\",\"parentId\":\"$T_MCP\",\"kind\":\"pr\",\"url\":\"https://github.com/Anmol-Srv/loop/commit/e3bbfa3\",\"title\":\"MCP surface\"}" >/dev/null
api POST "/api/user/artifacts" "$H" \
  "{\"parentType\":\"task\",\"parentId\":\"$T_TOKENS\",\"kind\":\"figma\",\"url\":\"https://www.figma.com/file/loop-tokens\",\"title\":\"Tokens\"}" >/dev/null

# ---- project two: earlier, design-led
P2=$(api POST /api/user/projects "$H" '{"key":"chk","name":"Checkout redesign"}' | ent)
D1=$(phase "$P2" Design 0)
D2=$(phase "$P2" Build 1)
api PATCH "/api/user/phases/$D1" "$H" '{"status":"done"}'   >/dev/null
api PATCH "/api/user/phases/$D2" "$H" '{"status":"active"}' >/dev/null

mk "$D1" "Cart and checkout flows" "$EVANA"  shipped >/dev/null
mk "$D1" "Error and empty states"  "$PRATIK" handoff >/dev/null
mk "$D2" "Cart UI"                 "$CHINMAY" in_progress >/dev/null
C_API=$(mk "$D2" "Cart totals API" "$ANMOL"   in_progress)
C_PAY=$(mk "$D2" "Payment picker"  "$CHINMAY" open)
api PATCH "/api/user/tasks/$C_PAY/blockers" "$H" "{\"blockedBy\":[\"$C_API\"]}" >/dev/null

# ---- agents: one mid-flight, one waiting on a human
handoff() { api POST "/api/user/tasks/$1/handoff" "$H" "{\"agentId\":\"$AGENT\",\"brief\":\"$2\"}" >/dev/null; }
agent() { api POST "/api/agent/tasks/$1/$2" "$A" "$3" >/dev/null; }
api POST /api/agent/hello "$A" '{"runtime":"hermes"}' >/dev/null

handoff "$T_IDS" "Serve a single task by id so the task page stops fetching every task."
agent "$T_IDS" ack '{}'
agent "$T_IDS" update '{"body":"Starting on the endpoint.","status":"in_progress"}'
agent "$T_IDS" log '{"lines":["reading src/routes/user/task.rs","adding the handler","cargo test: clean"]}'
agent "$T_IDS" now '{"text":"writing the route test"}'

handoff "$T_RATE" "Rate limit the MCP surface before agents outside the office connect."
agent "$T_RATE" ack '{}'
agent "$T_RATE" ask '{"body":"Should the limit be per agent token or per owner?"}'

echo
echo "seeded."
echo "  sign in as   anmol@airtribe.live / $PASSWORD   (chinmay, pratik and evana work too)"
echo "  agent        $A"
