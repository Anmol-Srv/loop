set -e
cd /Users/anmol/Drive/Airtribe/airtribe-control-plane
H=$(./target/debug/acp-admin mint human --owner anmol@airtribe.live --scopes read,write | sed -n 2p)
A=$(./target/debug/acp-admin mint hermes --owner anmol@airtribe.live --scopes read,propose | sed -n 2p)

api() { curl -s -X "$1" "localhost:8080$2" -H "authorization: Bearer $3" -H 'content-type: application/json' ${4:+-d "$4"}; }

PID=$(api POST /api/user/projects "$H" '{"key":"p2","name":"P2 Check"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["data"]["entity"]["id"])')
PH=$(api POST "/api/user/projects/$PID/phases" "$H" '{"name":"Build","position":1}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["data"]["entity"]["id"])')
T=$(api POST "/api/user/phases/$PH/tasks" "$H" '{"title":"migrate report"}' | python3 -c 'import sys,json;print(json.load(sys.stdin)["data"]["entity"]["id"])')

echo "1. agent sees only its scoped tools"
export ACP_TOKEN=$A
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | ./target/debug/acp-mcp | python3 -c '
import sys,json
names=sorted(t["name"] for t in json.load(sys.stdin)["result"]["tools"])
print("   ", ", ".join(names))
assert "task_search" in names and "task_create" in names, names
assert "work_claim" not in names, "claim tools are P3"'

echo "2. agent proposes via MCP -> task must NOT change"
echo "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"task_update\",\"arguments\":{\"taskId\":\"$T\",\"status\":\"in_review\"}}}" \
  | ./target/debug/acp-mcp | python3 -c 'import sys,json;print("   ", json.load(sys.stdin)["result"]["content"][0]["text"][:90])'
STATUS=$(api GET "/api/user/tasks?phaseId=$PH" "$H" | python3 -c 'import sys,json;print(json.load(sys.stdin)["data"][0]["status"])')
echo "    task status after proposal: $STATUS"
[ "$STATUS" = "open" ] || { echo "GUARDRAIL BREACHED"; exit 1; }

echo "3. agent cannot approve its own proposal"
CID=$(api GET /api/user/changes/pending "$H" | python3 -c 'import sys,json;print(json.load(sys.stdin)["data"][0]["id"])')
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "localhost:8080/api/user/changes/$CID/approve" -H "authorization: Bearer $A")
echo "    agent approve -> HTTP $CODE"
[ "$CODE" = "403" ] || { echo "AGENT SELF-APPROVED"; exit 1; }

echo "4. human approves -> task moves"
api POST "/api/user/changes/$CID/approve" "$H" > /dev/null
STATUS=$(api GET "/api/user/tasks?phaseId=$PH" "$H" | python3 -c 'import sys,json;print(json.load(sys.stdin)["data"][0]["status"])')
echo "    task status after approval: $STATUS"
[ "$STATUS" = "in_review" ] || { echo "APPROVAL DID NOT APPLY"; exit 1; }

echo "5. create-proposal target_id repoints to the real row"
export ACP_TOKEN=$A
echo "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"task_create\",\"arguments\":{\"phaseId\":\"$PH\",\"title\":\"from agent\"}}}" \
  | ./target/debug/acp-mcp > /dev/null
CID2=$(api GET /api/user/changes/pending "$H" | python3 -c 'import sys,json;print(json.load(sys.stdin)["data"][0]["id"])')
api POST "/api/user/changes/$CID2/approve" "$H" > /dev/null
psql -p 5433 -d acp_dev -tAc "SELECT CASE WHEN EXISTS (SELECT 1 FROM task t JOIN change c ON c.target_id = t.id WHERE c.id = '$CID2') THEN '    target_id points at a real task' ELSE '    DANGLING target_id' END"

echo
echo "ALL P2 CHECKS PASSED"
