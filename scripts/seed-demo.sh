#!/usr/bin/env bash
# Loads the control plane with its own history, so opening the UI shows real
# work rather than an empty state. Safe to re-run: it truncates first.
set -euo pipefail
cd "$(dirname "$0")/.."

BASE=${ACP_URL:-http://localhost:8080}
psql -p 5433 -d acp_dev -qc "TRUNCATE artifact, change, run_log_line, task, phase, project, job CASCADE;"

psql -p 5433 -d acp_dev -qc "DELETE FROM setup_code; DELETE FROM credential; DELETE FROM person;"

# A human session comes from a person, not from a minted agent: since the auth
# work, `mint` means an agent credential and agents cannot hold write.
./target/debug/acp-admin bootstrap-admin anmol@airtribe.live Anmol >/dev/null 2>&1 || true
H=$(./target/debug/acp-admin session anmol@airtribe.live | sed -n 2p)
A=$(./target/debug/acp-admin mint hermes --owner anmol@airtribe.live --scopes read,claim,propose | sed -n 2p)

api() { curl -sS -X "$1" "$BASE$2" -H "authorization: Bearer $3" -H 'content-type: application/json' ${4:+-d "$4"}; }
jq_() { python3 -c "import sys,json;d=json.load(sys.stdin);print($1)"; }
ent() { jq_ 'd["data"]["entity"]["id"]'; }

PID=$(api POST /api/user/projects "$H" '{"key":"loop","name":"Airtribe Control Plane"}' | ent)

i=0
declare -a PH
for spec in "P0 Backbone|done" "P1 Core and CLI|done" "P2 MCP and Approval|done" "P3 Delegation|done" "P4 Web UI|active"; do
  name=${spec%|*}; st=${spec#*|}
  id=$(api POST "/api/user/projects/$PID/phases" "$H" "{\"name\":\"$name\",\"position\":$i}" | ent)
  api PATCH "/api/user/phases/$id" "$H" "{\"status\":\"$st\"}" >/dev/null
  PH[$i]=$id; i=$((i+1))
done

task() { # phase-index title status
  local id
  id=$(api POST "/api/user/phases/${PH[$1]}/tasks" "$H" "{\"title\":\"$2\"}" | ent)
  [ "$3" = "open" ] || api PATCH "/api/user/tasks/$id" "$H" "{\"status\":\"$3\"}" >/dev/null
  echo "$id"
}

task 0 "schema and migrations" done >/dev/null
task 0 "the change write path"  done >/dev/null
task 1 "scoped bearer tokens"   done >/dev/null
task 1 "the acp CLI"            done >/dev/null
task 2 "proposals must not mutate" done >/dev/null
T_MCP=$(task 2 "scope-filtered MCP tools" done)
task 3 "claim-lease distribution" done >/dev/null
task 3 "run logs"                 done >/dev/null
T_UI=$(task 4 "the approval inbox" in_review)
T_LIVE=$(task 4 "task detail and live log" open)
task 4 "deploy notes"            open >/dev/null

api POST "/api/user/artifacts" "$H" \
  "{\"parentType\":\"task\",\"parentId\":\"$T_MCP\",\"kind\":\"pr\",\"url\":\"https://github.com/Anmol-Srv/loop/commit/e3bbfa3\",\"title\":\"MCP surface\"}" >/dev/null
api POST "/api/user/artifacts" "$H" \
  "{\"parentType\":\"project\",\"parentId\":\"$PID\",\"kind\":\"doc\",\"url\":\"https://github.com/Anmol-Srv/loop/blob/master/docs/superpowers/specs/2026-09-18-airtribe-control-plane-design.md\",\"title\":\"Design spec\"}" >/dev/null

# A task mid-flight under an agent, with a live log and a pending proposal.
api POST "/api/user/tasks/$T_LIVE/assign" "$H" '{"agentLabel":"hermes"}' >/dev/null
api POST /api/user/work/claim "$A" '{}' >/dev/null
api POST "/api/user/tasks/$T_LIVE/logs" "$A" \
  '{"lines":["reading templates/layout.html","wiring hx-trigger for in_progress only","appending via afterSeq cursor","cargo build: clean"]}' >/dev/null
api PATCH "/api/user/tasks/$T_LIVE" "$A" '{"status":"in_review"}' >/dev/null

# A second pending proposal, so the inbox has more than one card.
api POST "/api/user/artifacts" "$A" \
  "{\"parentType\":\"task\",\"parentId\":\"$T_UI\",\"kind\":\"link\",\"url\":\"https://htmx.org/docs/#polling\",\"title\":\"HTMX polling docs\"}" >/dev/null

echo
echo "seeded."
echo "  project  $PID"
echo "  human session (read,write,admin):  $H"
echo "  agent token 'hermes':              $A"
echo
echo "open http://localhost:8080/login and paste the human token"
