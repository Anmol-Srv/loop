#!/usr/bin/env bash
# Loads the control plane with a realistic slice: two projects, three people
# holding different disciplines, blocked work, an agent mid-flight, and
# proposals waiting. Safe to re-run — it truncates first.
set -euo pipefail
cd "$(dirname "$0")/.."
BASE=${ACP_URL:-http://localhost:8080}
PSQL="psql -p 5433 -d acp_dev"

$PSQL -qc "TRUNCATE artifact, change, run_log_line, task, phase, project, job, setup_code, credential, person CASCADE;"

adm() { ./target/debug/acp-admin "$@"; }
adm bootstrap-admin anmol@airtribe.live Anmol >/dev/null 2>&1 || true
adm add-person dhaval@airtribe.live Dhaval >/dev/null
adm add-person navneet@airtribe.live Navneet >/dev/null

$PSQL -qc "UPDATE person SET disciplines='{frontend,backend}' WHERE email='anmol@airtribe.live';
           UPDATE person SET disciplines='{design}'            WHERE email='dhaval@airtribe.live';
           UPDATE person SET disciplines='{backend}'           WHERE email='navneet@airtribe.live';"

H=$(adm session anmol@airtribe.live | sed -n 2p)
A=$(adm mint hermes --owner anmol@airtribe.live --scopes read,claim,propose | sed -n 2p)

api() { curl -sS -X "$1" "$BASE$2" -H "authorization: Bearer $3" -H 'content-type: application/json' ${4:+-d "$4"}; }
ent() { python3 -c 'import sys,json;print(json.load(sys.stdin)["data"]["entity"]["id"])'; }
pid() { $PSQL -tAc "SELECT id FROM person WHERE email='$1'"; }

# ---- project one: this tool, mid-flight
P1=$(api POST /api/user/projects "$H" '{"key":"loop","name":"Airtribe Control Plane"}' | ent)
i=0; declare -a PH
for spec in "Backbone|done" "Core and CLI|done" "MCP and approval|done" "Delegation|done" "Pages and design|active"; do
  n=${spec%|*}; st=${spec#*|}
  id=$(api POST "/api/user/projects/$P1/phases" "$H" "{\"name\":\"$n\",\"position\":$i}" | ent)
  api PATCH "/api/user/phases/$id" "$H" "{\"status\":\"$st\"}" >/dev/null
  PH[$i]=$id; i=$((i+1))
done

task() { # phase-idx  title  discipline  status
  local id
  id=$(api POST "/api/user/phases/${PH[$1]}/tasks" "$H" \
        "{\"title\":\"$2\"$([ -n "$3" ] && echo ",\"discipline\":\"$3\"")}" | ent)
  [ "$4" = open ] || api PATCH "/api/user/tasks/$id" "$H" "{\"status\":\"$4\"}" >/dev/null
  echo "$id"
}

task 0 "Schema and migrations"      backend  done >/dev/null
task 0 "The change write path"      backend  done >/dev/null
task 1 "Scoped bearer tokens"       backend  done >/dev/null
task 1 "The acp CLI"                backend  done >/dev/null
task 2 "Proposals must not mutate"  backend  done >/dev/null
T_MCP=$(task 2 "Scope-filtered MCP tools" backend done)
task 3 "Claim-lease distribution"   backend  done >/dev/null
task 3 "Run logs"                   backend  done >/dev/null

T_TOKENS=$(task 4 "Design tokens and theme"   design   done)
T_HOME=$(task  4 "Home page"                  frontend in_progress)
T_TRACK=$(task 4 "Project tracker"            frontend open)
T_EMPTY=$(task 4 "Empty state illustrations"  design   open)
T_IDS=$(task   4 "Task by id endpoint"        backend  open)
T_RATE=$(task  4 "Rate limit the MCP surface" backend  open)
T_LIVE=$(task  4 "Task detail live log"       frontend open)

# who owns what
api POST "/api/user/tasks/$T_HOME/assign"  "$H" '{"personEmail":"anmol@airtribe.live"}'  >/dev/null
api POST "/api/user/tasks/$T_TOKENS/assign" "$H" '{"personEmail":"dhaval@airtribe.live"}' >/dev/null
api POST "/api/user/tasks/$T_LIVE/assign"  "$H" '{"personEmail":"anmol@airtribe.live"}'  >/dev/null
# Dhaval owns the design task that the tracker waits on, so team capacity has a
# real bottleneck to show rather than three zeros.
api POST "/api/user/tasks/$T_EMPTY/assign" "$H" '{"personEmail":"dhaval@airtribe.live"}' >/dev/null
api POST "/api/user/tasks/$T_IDS/assign"   "$H" '{"personEmail":"navneet@airtribe.live"}' >/dev/null

# the tracker waits on the design tokens — the flow the board must show
api PATCH "/api/user/tasks/$T_TRACK/blockers" "$H" "{\"blockedBy\":[\"$T_EMPTY\"]}" >/dev/null

# an agent mid-flight on a task its owner holds
api POST "/api/user/tasks/$T_LIVE/logs" "$H" \
  '{"lines":["reading templates","wiring the cursor","cargo build: clean"]}' >/dev/null 2>&1 || true
api PATCH "/api/user/tasks/$T_LIVE" "$H" '{"status":"in_progress"}' >/dev/null

api POST "/api/user/artifacts" "$H" \
  "{\"parentType\":\"task\",\"parentId\":\"$T_MCP\",\"kind\":\"pr\",\"url\":\"https://github.com/Anmol-Srv/loop/commit/e3bbfa3\",\"title\":\"MCP surface\"}" >/dev/null

# ---- project two: earlier, design-led
P2=$(api POST /api/user/projects "$H" '{"key":"chk","name":"Checkout redesign"}' | ent)
D1=$(api POST "/api/user/projects/$P2/phases" "$H" '{"name":"Design","position":0}' | ent)
D2=$(api POST "/api/user/projects/$P2/phases" "$H" '{"name":"Build","position":1}' | ent)
api PATCH "/api/user/phases/$D1" "$H" '{"status":"done"}'   >/dev/null
api PATCH "/api/user/phases/$D2" "$H" '{"status":"active"}' >/dev/null

t2() { local id; id=$(api POST "/api/user/phases/$1/tasks" "$H" "{\"title\":\"$2\",\"discipline\":\"$3\"}" | ent)
       [ "$4" = open ] || api PATCH "/api/user/tasks/$id" "$H" "{\"status\":\"$4\"}" >/dev/null; echo "$id"; }
t2 "$D1" "Cart and checkout flows" design done >/dev/null
t2 "$D1" "Error and empty states"  design done >/dev/null
C_UI=$(t2 "$D2" "Cart UI"          frontend in_progress)
C_API=$(t2 "$D2" "Cart totals API" backend  open)
C_PAY=$(t2 "$D2" "Payment picker"  frontend open)
api POST "/api/user/tasks/$C_UI/assign" "$H" '{"personEmail":"anmol@airtribe.live"}' >/dev/null
api PATCH "/api/user/tasks/$C_PAY/blockers" "$H" "{\"blockedBy\":[\"$C_API\"]}" >/dev/null

# an agent proposal waiting on a human
export ACP_TOKEN=$A
api PATCH "/api/user/tasks/$T_HOME" "$A" '{"status":"in_review"}' >/dev/null
api POST "/api/user/artifacts" "$A" \
  "{\"parentType\":\"task\",\"parentId\":\"$T_IDS\",\"kind\":\"link\",\"url\":\"https://docs.rs/sqlx\",\"title\":\"sqlx docs\"}" >/dev/null

echo
echo "seeded."
echo "  sign in as   anmol@airtribe.live   (admin, frontend+backend)"
echo "  session      $H"
echo "  agent        $A"
