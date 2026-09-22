#!/usr/bin/env bash
# verify.sh <worktree> <sha> — independent verification of one committed candidate.
#
# The fresh detached worktree at the exact SHA and the gate ladder are the bridge's job
# (scripts/airtribe_bridge.py verify, gates in config/gates.json). This script adds the two
# things the bridge deliberately does not do: refuse a protected branch, and compare the full
# `pnpm test` failure count against reactor/baseline.json, because that suite carries known
# baseline failures and would otherwise fail every candidate.
#
#   exit 0  clean: gates passed and the full suite is no worse than the baseline
#   exit 1  failed: a gate failed, or the full suite regressed past the baseline
#   exit 2  not attempted: bad arguments, protected branch, wrong SHA, or an environment refusal
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=${AIRTRIBE_REPO:-/Users/anmol/Drive/Airtribe/mycohort-api}
[ $# -eq 2 ] || { echo "usage: verify.sh <worktree> <sha>" >&2; exit 2; }
WORKTREE=$1 SHA=$2
TASK_ID=${AIRTRIBE_TASK_ID:-$(basename "$WORKTREE")}

BRANCH=$(git -C "$WORKTREE" branch --show-current 2>/dev/null) || { echo "not a worktree: $WORKTREE" >&2; exit 2; }
case "$BRANCH" in
  master|main|"") echo "refusing to verify protected or detached branch: '${BRANCH:-detached}'" >&2; exit 2 ;;
esac
HEAD=$(git -C "$WORKTREE" rev-parse HEAD)
[ "$HEAD" = "$SHA" ] || { echo "worktree HEAD $HEAD is not the candidate $SHA" >&2; exit 2; }

OUT=$(python3 "$HERE/../scripts/airtribe_bridge.py" verify \
        --repo "$REPO" --task-id "$TASK_ID" --worktree "$WORKTREE" ${AIRTRIBE_GATES:+--gates "$AIRTRIBE_GATES"})
RC=$?
echo "$OUT"
[ $RC -eq 0 ] || exit $RC

# ponytail: `pnpm test` is not a gate (see config/gates.json), so run it here against the
# candidate worktree — it is clean and at the exact SHA, the bridge refused it otherwise.
# Re-reading counts out of the evidence first means a future full-suite gate needs no change.
EVIDENCE=$(printf '%s' "$OUT" | python3 -c 'import json,sys;print(json.load(sys.stdin).get("evidence",""))')
python3 - "$EVIDENCE" "$WORKTREE" "$HERE/baseline.json" <<'PY'
import json, re, subprocess, sys

evidence, worktree, baseline_path = sys.argv[1:4]
COUNT = re.compile(r"^(Tests|Test Suites):\s+(?:.*?\b(\d+) failed)?", re.M)


def counts(text):
    # a matched line with no "N failed" means zero failures, not "unreadable"
    got = {k: int(n or 0) for k, n in COUNT.findall(text)}
    return got.get("Tests"), got.get("Test Suites")


text = ""
if evidence:
    ev = json.load(open(evidence))
    text = "\n".join(g.get("output", "") for g in ev.get("gates", []))
failed, suites = counts(text)
if failed is None:
    print("full suite not in the evidence — running `pnpm test` in the candidate worktree")
    text = subprocess.run(["pnpm", "test"], cwd=worktree, capture_output=True, text=True).stdout
    failed, suites = counts(text)
if failed is None:
    print("could not read a jest failure count from the suite output", file=sys.stderr)
    sys.exit(2)

base = json.load(open(baseline_path))
print(f"full suite: {failed} failed tests / {suites} failed suites "
      f"(baseline {base['failed_tests']}/{base['failed_suites']} — {base['note']})")
if failed > base["failed_tests"] or (suites or 0) > base["failed_suites"]:
    print("REGRESSION past the baseline", file=sys.stderr)
    sys.exit(1)
print("no regression past the baseline")
PY
