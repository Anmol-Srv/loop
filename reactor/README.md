# reactor — the deterministic half of acp delegation

`react.py` runs once a minute. No LLM. Each acp task assigned to `AIRTRIBE_AGENT` (default
`hermes`) maps to exactly one command per tick. Planning, reviewing and delivering belong to
Hermes or a human: `bridge deliver` is never called, and a task with no brief is only reported.

| Situation | Action |
|---|---|
| claimable, no `tasks/<slug>/TASK.md` naming the task | run-log `needs brief: planning required`, said once |
| claimable, brief present, worktree missing | `work_claim` → `bridge prepare` → `bridge launch` |
| claimable, brief present, worktree exists | `work_claim` → `bridge launch` |
| held, tmux alive, no new commit | `work_heartbeat` + `bridge status` only |
| held, new commit vs state | `verify.sh <worktree> <sha>`, run-log the verdict, row in `EVENTS.md` |
| held, verify clean | run-log `verified <sha>` + `task_update` → `in_review` (lands pending) |
| held, tmux dead, no new commit | `bridge launch` again after 1 / 5 / 15 min, 3 tries |
| held, 4th failure | `work_release` + run-log `stalled` |
| in flight ≥ `AIRTRIBE_MAX_INFLIGHT` (default 2) | nothing new is claimed |

State: `.react-state.json`. Trail: `react.log`. Stdout carries only actions and stuck
workers, so an idle tick prints nothing.
## Running it

```bash
export ACP_TOKEN=...          # agent credential: read,claim,propose. Missing -> one line, exit 2
export ACP_URL=http://localhost:8080          # default
python3 reactor/react.py --dry-run            # prints the planned commands, changes nothing
```

Schedule as a **no-agent** (no LLM) job, `every 1m`, one instance only.

**Planning contract.** Hermes writes `.airtribe-worktrees/tasks/<slug>/TASK.md` containing the
acp task id plus `**Branch:**` and, optionally, `**Worktree:**`. The reactor matches folders to
tasks on that id alone.

## Re-baselining

`verify.sh` fails a candidate only if the full `pnpm test` failure count exceeds
`baseline.json` (seeded 23 failed tests / 52 failed suites — Node 26 vs `jsonwebtoken`). After
Node is pinned, run `pnpm test` on a clean `master` worktree and write the new
`failed_tests` / `failed_suites` / `sha` / `note` into `baseline.json`.
