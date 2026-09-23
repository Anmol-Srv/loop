# Agent control plane v1 — design

Personal agents (Hermes, Claude Code, Codex, anything that speaks HTTP or MCP)
take tasks their owner hands off from the dashboard, report back, and react to
what people change on those tasks. Local-first: the agent runs on its owner's
machine and calls the server; the server never reaches in.

Scope of v1 is **post-handoff** only: connecting an agent, handing a task to
it, everything that happens on that task afterwards, and taking it back.
Agents creating or triaging work is out of scope.

## 1. Agents are rows, not tokens

```sql
CREATE TABLE agent (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id     uuid NOT NULL REFERENCES person(id),
  handle       text NOT NULL,                 -- 'hermes', unique per owner
  name         text NOT NULL,                 -- 'Hermes (Anmol's Mac)'
  runtime      text NOT NULL DEFAULT 'other'
                 CHECK (runtime IN ('hermes','claude-code','codex','other')),
  connected_at timestamptz,                   -- first hello
  last_seen_at timestamptz,                   -- any authenticated call, 1-min throttle
  event_cursor bigint NOT NULL DEFAULT 0,     -- last agent_event id it acknowledged
  revoked_at   timestamptz,
  created_at   timestamptz NOT NULL DEFAULT now(),
  UNIQUE (owner_id, handle)
);
ALTER TABLE credential ADD COLUMN agent_id uuid REFERENCES agent(id);
```

- A `kind='agent'` credential carries `agent_id` (backfill: one agent per
  existing agent credential, handle = label, suffixed on collision). Tokens
  rotate under a stable agent; revoking the agent revokes its credentials.
- The old label-based assignment (`assignee_kind='agent'`, `assignee_token_id`,
  `claimed_by`, leases, `/work/*`, `reactor/`) is superseded. Leave the columns;
  remove `reactor/`, the `/work/*` routes, the lease reaper and the MCP
  `work_*` tools. `run_log_line` stays readable but agents report through
  activity (below).

## 2. Hand-off keeps the human assignee

A handed-off task is still **owned by its human assignee** — their department
still picks the track, their name still shows as owner. The agent is a delegate:

```sql
ALTER TABLE task
  ADD COLUMN delegate_agent_id uuid REFERENCES agent(id),
  ADD COLUMN agent_state text CHECK (agent_state IN
    ('handed_off','acknowledged','working','needs_input','in_review','done','stopped')),
  ADD COLUMN review_target text;              -- status the agent asked to move to
CREATE INDEX task_delegate_idx ON task (delegate_agent_id) WHERE delegate_agent_id IS NOT NULL;
```

Rules:
- Only the task's assignee can hand it off, only to an agent they own, only
  when the task is not finished (done_at null, not dropped). Hand-off sets
  `delegate_agent_id`, `agent_state='handed_off'`.
- **Take back** (assignee) clears the delegate and sets `agent_state='stopped'`
  for the event; the agent loses all access to the task.
- Dropping a delegated task is also a stop for the agent.

## 3. What the agent can do (only on tasks delegated to it)

| Action | Effect |
| --- | --- |
| `ack` | `agent_state` handed_off → acknowledged |
| `update {body, status?}` | progress entry; optional status move along the track for **non-finishing** targets (open→in_progress, →blocked, blocked→in_progress). Acts as the assignee, so track rules and conflict checks apply. `agent_state='working'` |
| `ask {body}` | question entry; `agent_state='needs_input'` until the owner answers |
| `attach {kind,url,title}` | artifact on the task (pr, commit, figma, doc, link) |
| `note {body}` | plain note |
| `submit {target, summary, manualReason?}` | target must be a finishing status legal from the current one (eng: completed/shipped, design: handoff/completed). Evidence gates checked **now** (eng→completed needs pr/commit or manualReason; design→handoff needs figma). Sets `review_target`, `agent_state='in_review'` |

Owner side (dashboard, session auth, assignee only):
- **Answer** a question → answer entry, `agent_state='working'`.
- **Approve** a submission → applies `review_target` as the assignee (normal
  transition path, stamps done_at), `agent_state='done'`.
- **Request changes {body}** → entry, `agent_state='working'`, review_target null.

Agent entries live in `note` so they appear where people already read:

```sql
ALTER TABLE note
  ADD COLUMN agent_id uuid REFERENCES agent(id),
  ADD COLUMN kind text NOT NULL DEFAULT 'note'
    CHECK (kind IN ('note','progress','question','answer','submission','review'));
```
An agent's note has `agent_id` set and `author_id` null.

## 4. Events: dashboard changes reach the agent

```sql
CREATE TABLE agent_event (
  id         bigserial PRIMARY KEY,
  agent_id   uuid NOT NULL REFERENCES agent(id) ON DELETE CASCADE,
  task_id    uuid NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  kind       text NOT NULL,   -- handed_off, taken_back, dropped, changed, note, artifact, answer, approved, changes_requested
  payload    jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX agent_event_feed_idx ON agent_event (agent_id, id);
```

- Produced by **Postgres triggers** on `task` UPDATE, `note` INSERT and
  `artifact` INSERT — one place, so no route can forget. `changed` carries the
  changed field names and new values (title, body, priority, status,
  target/start where present, blocked_by). A trigger emits only when the task
  has a delegate (for taken_back: had one) and the change was not made by that
  agent: agent routes run `SELECT set_config('acp.agent_id', $id, true)` in
  their transaction and triggers skip rows made under it.
- Feed: `GET /api/agent/events?after=<id>&wait=<0..25>` → events with
  id > after (default: the agent's `event_cursor`), long-polling up to `wait`
  seconds (LISTEN/NOTIFY channel `agent_event`). `POST /api/agent/events/ack
  {through}` advances `event_cursor` (monotonic).
- `GET /api/agent/inbox` → a **stable** plain-text summary: tasks delegated to
  me that need action (handed_off, answered, changes requested) plus
  unacknowledged events, no timestamps that tick. Identical bytes until
  something changes — built for Hermes' `--monitor-script` (model wakes only on
  change) and equally usable by `watch`, cron or a human.

## 5. One contract, three surfaces

REST under `/api/agent/*`, agent credentials only (a session gets 403 with a
readable message):

```
POST /api/agent/hello          {runtime, version?}   → me; sets connected_at
GET  /api/agent/me                                   → agent, owner, server
GET  /api/agent/tasks                                → my delegated tasks
GET  /api/agent/tasks/{id}                           → task context (below)
POST /api/agent/tasks/{id}/ack|update|ask|attach|note|submit
GET  /api/agent/events · POST /api/agent/events/ack · GET /api/agent/inbox
GET  /api/agent/skill                                → SKILL.md (text/markdown)
GET  /api/agent/onboarding?runtime=hermes|claude-code|codex|other → setup doc
```

**Task context** is what a worker needs to start without searching:
task (title, body, status, track, allowed next statuses, priority, dates),
project (name, description, labels, resources), owner, notes (all kinds,
oldest first), artifacts grouped by kind, and **related work**: tasks it is
blocked by and tasks it blocks, each with assignee department, status and
their pr/commit/figma artifacts — so a frontend task sees the backend PR it
has to wire, and a UI task sees the design task's Figma.

**MCP** (existing `/api/services/mcp`) serves the same actions as tools —
`agent_inbox`, `agent_tasks`, `task_context`, `task_ack`, `task_update`,
`task_ask`, `task_attach`, `task_note`, `task_submit`, `events_ack` — to agent
credentials, plus resource `acp://skill`. Make it a compliant Streamable-HTTP
server for JSON responses: negotiate protocolVersion (echo a supported client
version, default `2025-06-18`), accept notifications with `202`, `GET` → `405`.
It must connect from Hermes (`hermes mcp add --url … --auth header`) and Claude
Code (`claude mcp add --transport http … --header`).

**CLI**: `acp agent inbox|tasks|show|ack|update|ask|attach|submit` for tools
that only have a shell.

The skill is authored at `agent-kit/airtribe-agent/SKILL.md` and compiled into
the server (`include_str!`). It is deliberately **not** under `skills/`, which
the local Hermes profile already loads.

## 6. Onboarding

App → **Agents** (sidebar) → **Connect an agent**: handle, name, runtime →
server creates the agent and a 90-day token, returns a one-time **onboarding
prompt**:

> You are being connected to Airtribe Control Plane as agent "hermes" for
> Anmol. Server: https://… Token: acp_… (secret — store it, never print it
> again). Fetch your setup steps with
> `curl -fsS -H "Authorization: Bearer <token>" "<server>/api/agent/onboarding?runtime=hermes"`
> and follow them, then confirm.

The onboarding doc (templated by runtime, with the server URL filled in; the
token is referenced, never echoed back) tells the agent to: save the skill from
`/api/agent/skill` into its skills directory; register the MCP server (Hermes:
`hermes mcp add acp --url <server>/api/services/mcp --auth header`; Claude
Code: `claude mcp add --transport http …`); install the watcher (Hermes: a
script under `~/.hermes/scripts/` printing `/api/agent/inbox`, scheduled with
`hermes cron create "every 1m" --monitor-script … --skill airtribe-agent`);
call `hello`. The Agents page shows **Waiting for first contact** →
**Connected** (on hello) with last seen, current tasks, Rotate token, Revoke.

## 7. Dashboard

- **Task page**: `Hand off to <agent>` in the actions when the viewer is the
  assignee and owns an agent (picker if several); a delegate chip in the rail
  (agent name, state, last seen); agent entries in the notes timeline styled by
  kind; an open question as a callout with an inline answer box; a submission
  as a review panel (summary, evidence, target) with Approve / Request changes;
  `Take back`.
- **Home → Needs attention**: questions and reviews waiting on me.
- **My Tasks / tables**: a small agent marker on delegated tasks.
- **Agents page** as in §6.

## 8. The skill

Teaches, in order: when to look (inbox on every wake; act on events oldest
first; ack after handling), how to start (ack, read context, status to
in_progress), how to report (short progress updates at real milestones, not
chatter), when to ask vs decide, how to finish (attach evidence, submit with a
summary a reviewer can check), and how to stop (taken_back/dropped → stop
immediately, leave a final note only if work is mid-flight). Per kind of work:
design/UI → read the Figma artifacts, pull frames/screenshots with Figma tools
before building; frontend wiring → read the backend PR it depends on (`gh pr
view`, `gh pr diff`) for the API contract; backend → PR + commit as evidence,
note the endpoints for dependants. Never: approve its own work, act on tasks
not delegated to it, print the token.

## 9. Testing and done

- Server integration tests for every action, permission edge (other owner's
  agent, non-assignee hand-off, finished task, revoked agent, session calling
  agent routes), evidence gates on submit, trigger events and self-suppression,
  long-poll, stable inbox bytes, MCP handshake.
- Live test against the real Hermes `airtribe` profile, operated as its owner
  would — paste the onboarding prompt into `hermes chat`, no hints — covering
  onboarding, hand-off, ack, updates, question/answer, priority/description/
  note changes reaching it, submit with evidence, request changes, approve,
  take back, drop.
- Hermes baseline: back up the profile, strip the old wiring (acp MCP entry,
  the acp/reactor paragraph in SOUL.md, the `airtribe-react` cron and script,
  acp/reactor references in `skills/airtribe-task-lifecycle`), test, then
  restore to that stripped baseline and verify nothing from onboarding remains.
