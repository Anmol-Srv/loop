#!/usr/bin/env bash
# The auth flow, driven the way a person and an agent actually drive it.
set -euo pipefail
cd "$(dirname "$0")/.."
BASE=${ACP_URL:-http://localhost:8080}
PSQL="psql -p 5433 -d acp_dev"
step(){ printf '\n\033[1m%s\033[0m\n' "$*"; }
ok(){ printf '   ok  %s\n' "$*"; }
die(){ printf '   FAIL %s\n' "$*" >&2; exit 1; }
post(){ curl -sS -X POST "$BASE$1" -H 'content-type: application/json' ${3:+-H "authorization: Bearer $3"} -d "$2"; }
code(){ curl -sS -o /dev/null -w '%{http_code}' -X "$1" "$BASE$2" -H 'content-type: application/json' ${4:+-H "authorization: Bearer $4"} ${3:+-d "$3"}; }
jq_(){ python3 -c "import sys,json;d=json.load(sys.stdin);print($1)"; }

$PSQL -qc "DELETE FROM setup_code; DELETE FROM credential; DELETE FROM person;"

step "1. bootstrap an admin and seed a member"
CODE=$(./target/debug/acp-admin bootstrap-admin anmol@airtribe.live "Anmol" | grep -oE '[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}')
./target/debug/acp-admin add-person dhaval@airtribe.live Dhaval >/dev/null
MCODE=$(./target/debug/acp-admin invite dhaval@airtribe.live | grep -oE '[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}')
ok "admin code $CODE, member code $MCODE"

step "2. setup signs you straight in"
ADMIN=$(post /api/auth/setup "{\"email\":\"anmol@airtribe.live\",\"code\":\"$CODE\",\"password\":\"correct horse battery\"}" | jq_ 'd["data"]["token"]')
[ ${#ADMIN} -gt 20 ] || die "no token returned"
ROLE=$(curl -sS "$BASE/api/user/me" -H "authorization: Bearer $ADMIN" | jq_ 'd["data"]["scopes"]')
ok "signed in; scopes $ROLE"

step "3. the same code cannot be used twice"
C=$(code POST /api/auth/setup "{\"email\":\"anmol@airtribe.live\",\"code\":\"$CODE\",\"password\":\"another password!\"}")
[ "$C" = "400" ] || [ "$C" = "401" ] || die "reused code returned $C"
ok "reuse refused ($C)"

step "4. unknown address and wrong password are indistinguishable"
A=$(post /api/auth/login '{"email":"anmol@airtribe.live","password":"wrong wrong wrong"}')
B=$(post /api/auth/login '{"email":"ghost@airtribe.live","password":"wrong wrong wrong"}')
[ "$A" = "$B" ] || die "responses differ:\n  $A\n  $B"
ok "identical: $(echo "$A" | jq_ 'd["error"]["message"]')"

step "5. five failures lock the account, even for the right password"
for _ in 1 2 3 4; do post /api/auth/login '{"email":"anmol@airtribe.live","password":"nope nope nope"}' >/dev/null; done
LOCKED=$(post /api/auth/login '{"email":"anmol@airtribe.live","password":"correct horse battery"}' | jq_ 'd["error"]["message"]')
case "$LOCKED" in *lock*) ok "locked: $LOCKED";; *) die "expected a lockout, got: $LOCKED";; esac
$PSQL -qc "UPDATE person SET locked_until = now() - interval '1 minute' WHERE email='anmol@airtribe.live'"
ADMIN=$(post /api/auth/login '{"email":"anmol@airtribe.live","password":"correct horse battery"}' | jq_ 'd["data"]["token"]')
ok "works again once the lock lapses"

step "6. a member sets up and mints their own agent"
MEMBER=$(post /api/auth/setup "{\"email\":\"dhaval@airtribe.live\",\"code\":\"$MCODE\",\"password\":\"another good password\"}" | jq_ 'd["data"]["token"]')
AGENT=$(post /api/user/agents '{"label":"hermes","scopes":["read","claim","propose"]}' "$MEMBER" | jq_ 'd["data"]["token"]')
ok "agent minted by a member, no admin needed"

step "7. an agent cannot be given write"
MSG=$(post /api/user/agents '{"label":"bad","scopes":["read","write"]}' "$MEMBER" | jq_ 'd["error"]["message"]')
case "$MSG" in *write*) ok "refused: $MSG";; *) die "expected refusal, got: $MSG";; esac

step "8. a member cannot reach the admin routes"
C=$(code POST /api/admin/invite '{"email":"navneet@airtribe.live"}' "$MEMBER")
[ "$C" = "403" ] || die "member got $C on an admin route"
ok "member blocked (403)"

step "9. promoting does not retrofit an existing session"
post /api/admin/role '{"email":"dhaval@airtribe.live","role":"admin"}' "$ADMIN" >/dev/null
C=$(code POST /api/admin/invite '{"email":"navneet@airtribe.live"}' "$MEMBER")
[ "$C" = "401" ] || die "the promoted member's OLD session should be dead, got $C"
ok "old session ended by the role change; scopes are not retrofitted"
MEMBER=$(post /api/auth/login '{"email":"dhaval@airtribe.live","password":"another good password"}' | jq_ 'd["data"]["token"]')
ok "signed in again, now carrying admin"

step "10. demotion ends the demoted admin's sessions"
ENDED=$(post /api/admin/role '{"email":"anmol@airtribe.live","role":"member"}' "$MEMBER" | jq_ 'd["data"]["sessionsEnded"]')
C=$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/api/user/me" -H "authorization: Bearer $ADMIN")
[ "$C" = "401" ] || die "demoted admin's session still works ($C)"
ok "$ENDED session(s) ended; the old token is dead"

step "11. the last admin cannot demote themselves"
MSG=$(post /api/admin/role '{"email":"dhaval@airtribe.live","role":"member"}' "$MEMBER" | jq_ 'd["error"]["message"]')
case "$MSG" in *only\ admin*) ok "refused: $MSG";; *) die "expected last-admin refusal, got: $MSG";; esac

step "12. sessions listing never leaks a hash"
BODY=$(curl -sS "$BASE/api/admin/sessions" -H "authorization: Bearer $MEMBER")
case "$BODY" in *tokenHash*|*token_hash*) die "hash leaked";; *) ok "no hash in the response";; esac

printf '\n\033[1mAUTH FLOW PASSED\033[0m\n'
