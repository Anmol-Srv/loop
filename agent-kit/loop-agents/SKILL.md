---
name: loop-agents
description: Use Loop (Airtribe Control Plane) and build agents for it — read and create projects and tasks as a person, mint an agent, wire it to Loop the way Airtribe's own agents are (worker that takes hand-offs, intake that files tasks from a source), and keep it awake with a launchd watcher. Load when asked to use Loop, add work to the dashboard, or build, connect or debug a Loop agent.
version: 1.0.0
author: Airtribe Control Plane
license: MIT
---

# Loop: using it, and building agents for it

Loop is Airtribe's control plane: a server (`/api/...`) plus a desktop app.
People own **projects**, which hold **tasks**. People own **agents**, programs
on their own machine that take the tasks handed to them and report back on the
task. Everything an agent does shows up on the dashboard. The dashboard is the
source of truth.

There are two kinds of credential. Never mix them up:

| Credential | Who it is | Calls | Where it lives |
| --- | --- | --- | --- |
| **Person token** | a signed-in human | `/api/user/...` | `~/Library/Application Support/airtribe-control-plane/credentials` (the app's sign-in) |
| **Agent token** | one agent | `/api/agent/...`, MCP `/api/services/mcp` | the agent's own `.env`, as `AIRTRIBE_TOKEN` |

The server address is never written into code or skills. A person reads it
from `.../airtribe-control-plane/server` (or `$ACP_URL`); an agent reads
`$AIRTRIBE_URL`. Note that the address may include a path prefix (for example
`.../loop`), so always append `/api/...` to the whole value.

Every response is `{"success": true, "data": ...}`. When a call fails, the
error message tells you what to fix. Read it, change the call and send it
again. Never resend an identical request.

## 1. Using Loop as a person

Work as the signed-in person. Read the token, never print it, and never
commit it:

```bash
D="$HOME/Library/Application Support/airtribe-control-plane"
U=$(tr -d '[:space:]' < "$D/server"); T=$(tr -d '[:space:]' < "$D/credentials")
curl -fsS --oauth2-bearer "$T" "$U/api/user/me"
```

Use `curl`, not Python's `urllib`. On macOS, `urllib` often fails with
CERTIFICATE_VERIFY_FAILED.

| Need | Call |
| --- | --- |
| Who's on the team (assignee ids) | `GET /api/user/people` |
| Projects with done/total | `GET /api/user/projects` |
| A project and its phases | `GET /api/user/projects/{id}`, `GET …/{id}/flow` |
| Make a project **with its tasks** in one call | `POST /api/user/projects {name, description, priority?, startDate?, targetDate?, tasks:[{title, body?, assigneeId?, priority?, category?}]}` |
| Add a task to a project | `POST /api/user/projects/{id}/tasks {title, body?, assigneeId?, priority?}` |
| Standalone task (or into `projectId`) | `POST /api/user/tasks {title, body?, assigneeId?, priority?, projectId?}` |
| Search tasks | `GET /api/user/tasks?projectId=&status=&assigneeEmail=` |
| Move a task | `PATCH /api/user/tasks/{id} {status, expectedStatus?}` |
| Edit title/body/priority/assignee | `PATCH /api/user/tasks/{id}/details {title?, body?, priority?, assigneeId?}` |
| Assign | `POST /api/user/tasks/{id}/assign {personEmail}` |
| Note on a task | `POST /api/user/tasks/{id}/notes {body}` |
| Hand a task to an agent | `POST /api/user/tasks/{id}/handoff {agentId, brief?}` |
| Approve / bounce an agent's plan | `POST …/{id}/plan/approve`, `POST …/{id}/plan/changes {body}` |
| Review a submission | `POST /api/user/tasks/{id}/review {decision: approve\|changes, body?}` |
| Take it back from the agent | `POST /api/user/tasks/{id}/takeback` |
| Answer / instruct the agent | `POST …/{id}/answer {body}`, `POST …/{id}/instruct {body}` |

Rules when changing the dashboard for someone:

- **Propose first.** Show the projects, tasks and assignees you plan to create
  as a table, and create them only after the person says go. Writes are
  visible to the whole team.
- Check every assignee against `GET /api/user/people` before writing anything.
  A task has exactly one assignee. If work is shared, assign it to one person
  and name the other in the body.
- Priority runs from 0 to 4, and 2 is the default. Each task needs a title. A
  project needs a name.
- After you create things, list them back (`GET /api/user/projects`) and
  report the counts.

The `acp` CLI (`acp login`, `acp project`, `acp task`, `acp agent connect`)
does the same jobs if it is installed.

## 2. What an agent is to Loop

An agent row has a `handle`, a `name`, a `runtime`
(`hermes | claude-code | codex | other`) and two roles:

- **canWork**: it takes tasks its owner hands off. Its loop is inbox, then
  plan, then approval, then work, then submit. Its contract is the
  `airtribe-agent` skill, served by `GET /api/agent/skill`.
- **canIntake**: it files tasks for its owner from a source such as Slack, and
  they land in Triage. Its contract is the `airtribe-intake` skill.

Both roles share a few rules. An agent acts **only** on tasks handed to it. It
never approves its own work. It plans before it builds. It submits with real
evidence: a PR, a commit or a Figma link. It stops the moment a task is taken
back or dropped.

## 3. Building an agent: the recipe

Every agent Airtribe runs is built from the same five parts. Build them in
this order and check each one before starting the next.

### 3.1 Mint it

In the app, go to Agents › Connect an agent, choose a runtime, name it and
copy the prompt. To mint it over the API with a person token instead:

```bash
curl -fsS -X POST --oauth2-bearer "$T" -H 'content-type: application/json' \
  -d '{"handle":"slack-agent","name":"Slack Agent","runtime":"hermes","canWork":false,"canIntake":true}' \
  "$U/api/user/agents"
```

The response is `{agent, token, prompt}`. The **token is shown once**. The
`prompt` is the full onboarding text for that runtime. You can follow it
yourself or paste it into the agent's first session. If a token leaks, rotate
it with `POST /api/user/agents/{id}/rotate`.

### 3.2 Give it a home

Each agent gets its own directory, for example `~/.<agent-name>/` or a Hermes
profile. Put this in it:

```
.env          AIRTRIBE_URL=…  AIRTRIBE_TOKEN=…   (chmod 600, nothing else reads it)
AGENT.md      who it is, whose agent, how it works on code (the persona)
SKILL.md      GET /api/agent/skill, re-fetched when the inbox names a new version
mcp.json      the airtribe MCP server (below)
logs/  state/
```

```json
{ "mcpServers": { "airtribe": {
    "type": "http",
    "url": "${AIRTRIBE_URL}/api/services/mcp",
    "headers": { "Authorization": "Bearer ${AIRTRIBE_TOKEN}" } } } }
```

The server is called `airtribe` everywhere (not `acp`). Keep the token in
`.env` and refer to it as `${AIRTRIBE_TOKEN}`. It must never go in a file that
could be committed.

Check the setup: `curl -fsS --oauth2-bearer "$AIRTRIBE_TOKEN" "$AIRTRIBE_URL/api/agent/me"`
must print the agent.

### 3.3 Say hello

The agent shows as **Connected** only after it calls hello. Report what is
actually true:

```bash
curl -fsS -X POST --oauth2-bearer "$AIRTRIBE_TOKEN" -H 'content-type: application/json' \
  -d '{"runtime":"claude-code","setup":{"skill":"<installed version: line>","mcp":true,"watcher":true}}' \
  "$AIRTRIBE_URL/api/agent/hello"
```

### 3.4 Keep it awake: the watcher

Model calls are expensive and polling is cheap. A **launchd job every 30s**
makes one cheap check. It starts the model only when that check says something
changed. This is how Airtribe's own agents run.

`~/Library/LaunchAgents/live.airtribe.<name>.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>live.airtribe.NAME</string>
  <key>ProgramArguments</key><array><string>/bin/bash</string><string>/Users/USER/.NAME/watch.sh</string></array>
  <key>StartInterval</key><integer>30</integer>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>/Users/USER/.NAME/logs/launchd.out.log</string>
  <key>StandardErrorPath</key><string>/Users/USER/.NAME/logs/launchd.err.log</string>
</dict></plist>
```

Load it with `launchctl bootstrap gui/$(id -u) <plist>`. Unload it with
`launchctl bootout gui/$(id -u)/live.airtribe.<name>`. **Ask the owner before
you install any background service.**

Every watcher script has the same skeleton:

```bash
#!/usr/bin/env bash
set -uo pipefail
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$HOME/.local/bin"  # launchd has no PATH
D="$HOME/.NAME"; set -a; . "$D/.env"; set +a
LOG="$D/logs/watch.log"; mkdir -p "$D/state" "$D/logs"
mkdir "$D/state/run.lock.d" 2>/dev/null || exit 0       # a tick is still running
trap 'rmdir "$D/state/run.lock.d"' EXIT
api() { curl -fsS --max-time 20 --oauth2-bearer "$AIRTRIBE_TOKEN" "$@" 2>>"$LOG"; }

# cheap check: no model call when nothing changed
…
# only now: run the model
```

What the cheap check is depends on the role.

**Worker (canWork) with Claude Code.** Our Airtribe Agent works this way.

- Check `GET /api/agent/tasks/pending`. It returns
  `[{id, title, state, pendingEvents, latestEventId}]`, and `[]` means quiet.
  Do not keep a local "seen" cache. A run that fails leaves its events unacked,
  so the next tick retries it.
- Skip tasks whose `state` is `stopped` or `done`.
- **Run one continuous Claude session per task.** On the first run, generate a
  UUID and save `{sessionId, cwd}` to `state/tasks/<id>.json`. Report it with
  `POST /api/agent/tasks/{id}/session {sessionId, cwd}`, which lets the owner
  attach from the app. Use `claude -p … --session-id <uuid>` on that first run
  and `--resume <uuid>` on later runs. `--resume` only finds the session from
  the same cwd, so resolve the cwd once and reuse it.
- **cwd**: in `GET /api/agent/tasks/{id}`, use the first
  `project.repos[].localPath`. If there is none, use `folder.path`. If there is
  neither, use a per-task folder. When the cwd is a git repo, make a worktree
  beside it on its own branch (`agent/<slug>`), and never touch the primary
  checkout. If a PR is already attached to the task, build on that PR's
  branch.
- **Owner attached**: if `ps` shows an interactive `claude --resume <sid>`
  without `-p`, skip that task this tick.
- Take a lock per task (`state/tasks/<id>.lock.d`) so one task never runs
  twice at once.
- Invocation:

  ```bash
  claude -p "$prompt" --session-id|--resume "$sid" --model claude-opus-5-5 \
    --mcp-config "$D/mcp.json" --strict-mcp-config \
    --append-system-prompt-file "$D/state/system.md" \
    --permission-mode acceptEdits --add-dir <work roots> \
    --allowedTools "mcp__airtribe" Read Edit Write Glob Grep "Bash(git:*)" "Bash(gh:*)" …
  ```

  Build `system.md` from `AGENT.md` and `SKILL.md`. The prompt names **one**
  task id and says: "handle only this task's events, ack them, and follow
  AGENT.md and the airtribe-agent skill".

**Intake (canIntake) with Hermes.** Our Slack Agent works this way.

- The cheap check is a small script that prints a fingerprint of the source,
  for example the latest message timestamp across watched channels. If it
  matches `state/seen`, exit.
- If it changed, run the pass:
  `hermes -p <profile> --skills airtribe-intake -z "<do one intake pass…>"`.
- Write the new fingerprint to `seen` **only after a pass that really
  worked**. Otherwise a failed pass never retries.
- Each pass ends with `run_report` (`POST /api/agent/runs
  {status: ok|partial|failed, summary, counts:{filed, appended, alreadyFiled, skipped}, error?}`).
  The owner reads these as the agent's log.
- File with `POST /api/agent/intake
  {source:{kind, key, url, channel, channelName?, author, text, receivedAt}, title, body, category, reason, confidence, priority?}`.
  `category` is one of bug, feature, feedback, question or chore. `source.key`
  is the stable message id, and the same key is never filed twice: a `409`
  means it is already filed, so skip it. When a message continues a thread
  that was already filed, use `POST /api/agent/intake/{id}/append`. Check
  `GET /api/agent/intake/recent` first, for dedupe.
- Read the source only. Never post, react or mark anything as read.

**Hermes worker, no launchd.** Hermes can watch on its own:
`hermes cron create "every 1m" … --monitor-script airtribe-inbox.sh --skill airtribe-agent`.
The monitor script fetches `GET /api/agent/inbox`, which returns the same
bytes until something changes. Cron jobs run only while the Hermes gateway is
up (`hermes cron status`).

**Claude Code with nobody running a watcher.** Run
`/loop 5m check my Airtribe inbox and act on it using the airtribe-agent skill`
in an open session.

### 3.5 Prove it end to end

1. The dashboard shows the agent as Connected, and the setup checks are
   green.
2. For a worker: hand it a throwaway task. A watcher run should start within
   about 30s, then `task_ack`, then a `task_plan` should appear. Approve it,
   and the next tick should build it. Take it back, and the agent should stop.
3. For intake: post a test message in the source. A Triage task should appear
   with a `run_report` beside it. Post the same message again and nothing new
   should be filed.
4. Tail `logs/watch.log` and confirm quiet ticks make no model call.

## 4. When it doesn't work

| Symptom | Look at |
| --- | --- |
| Stuck at "Connecting" | It never called `/api/agent/hello`. |
| Never wakes | Check `launchctl print gui/$(id -u)/live.airtribe.<name>`, the launchd err log, and PATH inside the script. |
| Wakes but does nothing | `tasks/pending` is empty because events were already acked. Check that the agent acks only what it handled. |
| Runs the same task over and over | The agent never acks its events (`events_ack` with the highest id handled). |
| `--resume` starts fresh | The cwd changed between runs. Use the saved cwd from `state/tasks/<id>.json`. |
| 401 | The token was rotated or revoked. Mint a new one and update `.env`. |
| Submit refused | No approved plan for this hand-off, or no evidence attached. |

## Never

- Hard-code the server URL or a token in a script, skill, plist or commit.
- Give an agent a person token, or a person flow an agent token.
- Let an agent act on tasks not handed to it, approve its own work, or keep
  going after `taken_back` or `dropped`.
- Install a launchd job or other background service without the owner saying
  yes.
- Create dashboard work in bulk without showing the plan first.
