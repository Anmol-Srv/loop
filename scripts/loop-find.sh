#!/usr/bin/env bash
# Find Loop tasks that match some words — for any Claude session working in
# the Airtribe repos, to find the task its work belongs to before starting.
#
#   scripts/loop-find.sh cohort email session report
#
# Prints the best matches first: score, id, status, assignee, the agent
# holding it (if any), title, project. Uses the team workspace's saved
# sign-in (the Loop app's), never the private one; the token is not printed.
set -euo pipefail
[ $# -gt 0 ] || { echo "usage: loop-find.sh <words…>" >&2; exit 2; }
read -r U T < <(python3 -c "import json,os;w=[x for x in json.load(open(os.path.expanduser('~/Library/Application Support/airtribe-control-plane/workspaces.json'))) if not x['private']][0];print(w['server'],w['token'])")
curl -fsS --oauth2-bearer "$T" "$U/api/user/tasks" | python3 -c '
import json,sys
words=[w.lower() for w in sys.argv[1:]]
for t in json.load(sys.stdin)["data"]:
    hay=" ".join([t.get("title") or "",t.get("body") or "",t.get("projectName") or ""]).lower()
    hits=sum(w in hay for w in words)
    if hits: print(hits, t["id"], t["status"], (t.get("assigneeName") or "-"), ("agent:"+t["delegate"]["name"] if t.get("delegate") else ""), "|", t["title"], "|", t.get("projectName") or "")
' "$@" | sort -rn | head -10
