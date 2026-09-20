#!/usr/bin/env bash
# Drives the full delegation loop the way Hermes will, against a running server.
# Proves over real HTTP what the unit tests only prove in process.
set -euo pipefail
cd "$(dirname "$0")/.."

BASE=${ACP_URL:-http://localhost:8080}
PSQL="psql -p 5433 -d acp_dev"
step() { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '   ok  %s\n' "$*"; }
die()  { printf '   FAIL %s\n' "$*" >&2; exit 1; }

api() { # method path token [body]
  curl -sS -X "$1" "$BASE$2" -H "authorization: Bearer $3" \
       -H 'content-type: application/json' ${4:+-d "$4"}
}
code() {
  curl -sS -o /dev/null -w '%{http_code}' -X "$1" "$BASE$2" -H "authorization: Bearer $3" \
       -H 'content-type: application/json' ${4:+-d "$4"}
}
jq_() { python3 -c "import sys,json;d=json.load(sys.stdin);print($1)"; }

step "0. bootstrap identities"
./target/debug/acp-admin add-person anmol@airtribe.live Anmol >/dev/null
HUMAN=$(./target/debug/acp-admin mint human  --owner anmol@airtribe.live --scopes read,write          | sed -n 2p)
AGENT=$(./target/debug/acp-admin mint hermes --owner anmol@airtribe.live --scopes read,claim,propose  | sed -n 2p)
ok "human token (read,write) and agent token 'hermes' (read,claim,propose)"

step "1. human sets up the work"
PID=$(api POST /api/user/projects "$HUMAN" '{"key":"rehearse","name":"Delegation Rehearsal"}' | jq_ 'd["data"]["entity"]["id"]')
PH=$(api POST "/api/user/projects/$PID/phases" "$HUMAN" '{"name":"Build","position":1}'        | jq_ 'd["data"]["entity"]["id"]')
T=$(api POST "/api/user/phases/$PH/tasks" "$HUMAN" '{"title":"migrate the report"}'            | jq_ 'd["data"]["entity"]["id"]')
api POST "/api/user/tasks/$T/assign" "$HUMAN" '{"agentLabel":"hermes"}' >/dev/null
ok "project, phase, task; task assigned to hermes"

step "2. agent claims the work"
CLAIMED=$(api POST /api/user/work/claim "$AGENT" '{}' | jq_ 'd["data"]["id"] if d["data"] else "none"')
[ "$CLAIMED" = "$T" ] || die "expected to claim $T, got $CLAIMED"
ok "claimed $CLAIMED"
STATUS=$(api GET "/api/user/tasks?phaseId=$PH" "$HUMAN" | jq_ 'd["data"][0]["status"]')
[ "$STATUS" = "in_progress" ] || die "status should be in_progress, is $STATUS"
ok "status moved to in_progress under lease"

step "3. a second worker cannot take it"
AGENT2=$(./target/debug/acp-admin mint hermes-2 --owner anmol@airtribe.live --scopes read,claim,propose | sed -n 2p)
SECOND=$(api POST /api/user/work/claim "$AGENT2" '{}' | jq_ 'd["data"]["id"] if d["data"] else "none"')
[ "$SECOND" = "none" ] || die "lease leaked: second worker got $SECOND"
ok "second worker got nothing"

step "4. agent heartbeats and streams its log"
api POST "/api/user/work/$T/heartbeat" "$AGENT" >/dev/null
api POST "/api/user/tasks/$T/logs" "$AGENT" '{"lines":["reading the source","porting the query","done"]}' >/dev/null
LINES=$(api GET "/api/user/tasks/$T/logs" "$HUMAN" | jq_ 'len(d["data"])')
[ "$LINES" = "3" ] || die "expected 3 log lines, got $LINES"
FIRST=$(api GET "/api/user/tasks/$T/logs" "$HUMAN" | jq_ 'd["data"][0]["text"]')
[ "$FIRST" = "reading the source" ] || die "log out of order: $FIRST"
ok "3 lines, in order, readable by the human"

step "5. a worker that does not hold the lease cannot write to the log"
C=$(code POST "/api/user/tasks/$T/logs" "$AGENT2" '{"lines":["i was never here"]}')
[ "$C" = "403" ] || die "expected 403 for non-holder, got $C"
ok "non-holder rejected with 403"

step "6. agent proposes the result -- and nothing changes yet"
api PATCH "/api/user/tasks/$T" "$AGENT" '{"status":"in_review"}' >/dev/null
STATUS=$(api GET "/api/user/tasks?phaseId=$PH" "$HUMAN" | jq_ 'd["data"][0]["status"]')
[ "$STATUS" = "in_progress" ] || die "GUARDRAIL BREACHED: agent moved the task to $STATUS"
ok "task still in_progress; the proposal is only a proposal"

step "7. agent cannot approve itself"
CID=$(api GET /api/user/changes/pending "$HUMAN" | jq_ 'd["data"][0]["id"]')
C=$(code POST "/api/user/changes/$CID/approve" "$AGENT")
[ "$C" = "403" ] || die "agent self-approved (HTTP $C)"
ok "self-approval rejected with 403"

step "8. human approves"
api POST "/api/user/changes/$CID/approve" "$HUMAN" >/dev/null
STATUS=$(api GET "/api/user/tasks?phaseId=$PH" "$HUMAN" | jq_ 'd["data"][0]["status"]')
[ "$STATUS" = "in_review" ] || die "approval did not apply; status is $STATUS"
ok "task is in_review"

step "9. a lapsed lease frees the task"
T2=$(api POST "/api/user/phases/$PH/tasks" "$HUMAN" '{"title":"second job"}' | jq_ 'd["data"]["entity"]["id"]')
api POST "/api/user/tasks/$T2/assign" "$HUMAN" '{"agentLabel":"hermes"}' >/dev/null
api POST /api/user/work/claim "$AGENT" '{}' >/dev/null
$PSQL -qc "UPDATE task SET claim_expires_at = now() - interval '1 minute' WHERE id = '$T2'"
RECLAIMED=$(api POST /api/user/work/claim "$AGENT2" '{}' | jq_ 'd["data"]["id"] if d["data"] else "none"')
[ "$RECLAIMED" = "$T2" ] || die "lapsed lease did not free the task (got $RECLAIMED)"
ok "another worker reclaimed it after the lease lapsed"

printf '\n\033[1mDELEGATION REHEARSAL PASSED\033[0m\n'
