# Agent sessions — the contract

Context: `PRODUCT.md` (register, personality, visibility principles) and
`docs/superpowers/specs/2026-09-23-agent-control-plane-design.md` (the agent
model). This pass makes agents visible teammates: identity, live working
states, reports, logs, and a refined Connect flow — Linear-grade.

Never kill a process you did not start; the dev server on :8080 is shared.
Tests: `DATABASE_URL=postgres://localhost:5433/acp_dev cargo test --features app`.
New migrations only (a build.rs makes cargo notice them); never edit applied ones.

## Owners — disjoint files
| Owner | Files |
| --- | --- |
| Server | `migrations/**`, `src/controllers/**`, `src/routes/**`, `src/models/**`, `src/middleware/**`, `src/cli/**` (additive), `src/bin/acp.rs`, `tests/*.rs` except `*_ui.rs` |
| Client | `src/desktop/**`, `tests/*_ui.rs`, `docs/design-mocks/render/**` |
| Parent | `agent-kit/**`, `PRODUCT.md`, `docs/**` (other), verification |

## Visibility (server-enforced)
- **Private to the task's owner** (the assignee who delegated), **admins**, and **the agent itself**: notes of kind `question`, `answer`, `instruction`; the step log. Everyone else never receives them (filtered in the query, not the client). Home `needsAttention` already is the owner's.
- **Team-visible**: delegate identity, `agentState`, the `now` line, progress/submission/review notes, artifacts, agents at work.
- Agent management (`/api/user/agents*`) stays owner-only as today.

## Server
1. **Now line**: `task.agent_now text`, `task.agent_now_at timestamptz`. Agent: `POST /api/agent/tasks/{id}/now {text}` (1–120 chars, trimmed) and `update` accepts optional `now`. Cleared automatically whenever `agent_state` leaves `working`/`acknowledged`. MCP tool `task_now`.
2. **Step log**: agent `POST /api/agent/tasks/{id}/log {lines: string[]}` (≤200 lines/request, each ≤2000 chars, appended to `run_log_line` with the next seq; no lease). MCP tool `task_log`. Reading `GET /api/user/tasks/{id}/logs?afterSeq=` is owner/admin only (403 otherwise, readable sentence).
3. **Private instructions**: `POST /api/user/tasks/{id}/instruct {body}` — owner only, only while the agent holds the task; stores a note kind `instruction` (extend the note kind CHECK); the note trigger emits agent event kind `instruction` with `{body, author}`. Instructions appear in the agent's `task_context` notes.
4. **Hello reports setup**: `POST /api/agent/hello {runtime, version?, setup?: {skill?: string, mcp?: bool, watcher?: bool}}` stored in `agent.setup jsonb` (default `{}`).
5. **Delegation timing**: `task.delegated_at timestamptz` set on hand-off (backfill from the latest `handed_off` agent_event).
6. **Shapes**:
   - Task JSON `delegate` gains `now`, `nowAt`, `delegatedAt`, `runtime`, `ownerName`; task JSON gains `canSeeAgentPrivate: bool` (viewer is owner or admin).
   - Owner's Agent JSON gains `setup` (object), `currentTask: {id, title, state, now} | null` (most recently delegated held task), `activity: number[14]` (agent-authored notes per day, oldest → today).
   - `GET /api/user/agents/active` (any signed-in person): `[{agent: {id, name, handle, runtime}, owner: {id, name}, task: {id, title, projectName}, state, now, nowAt, lastSeenAt, delegatedAt}]` for held tasks with state in (acknowledged, working, needs_input, in_review), ordered working first then by `nowAt`/`lastSeenAt` desc.
   - `GET /api/user/tasks/{id}/notes` filters private kinds by viewer.
7. Tests in `tests/agent_sessions.rs`: now set/cleared, log post + owner-only read (403 for another member, 200 admin), instruction owner-only + emits event + agent sees it, private notes hidden from another member but visible to owner/admin, hello setup stored, `/agents/active` visible to others and excludes finished/archived, delegatedAt set.

## Client (design: calm, precise, alive; no chatbot bubbles, no neon/AI glow)
- **Agent avatar** component: rounded-square mark (painted robot glyph) tinted with the owner's avatar colour; sizes 16/20/28/40. States: idle (static), working (a slow rotating arc/ring, ~1.6 s, ease-in-out, repaint only while visible and working), needs input (amber dot badge), offline (desaturated when lastSeen > 10 min). Reduced motion → static ring.
- **Task page — "Agent session" section** (replaces the current scattered delegate bits):
  - Header: avatar + "Hermes · Anmol's agent" + stage stepper Handed off → Working → Needs you / In review → Done (current stage emphasised, past stages ticked with painted ticks, times on hover) + "started 2h ago".
  - Now line under it while working: small spinner + `now` text + relative `nowAt`.
  - Session timeline: agent entries (progress, submission, review, and for the owner also question/answer/instruction with a quiet "Only you can see this" lock label). Icons per kind, painted. Not chat bubbles: a left-rail timeline with hairline connector.
  - Submission renders as a **report card**: summary, evidence chips (PR/commit/Figma with kind glyphs, clickable), target status, Approve / Request changes (owner).
  - Owner-only **Logs** disclosure: monospace, line numbers faint, auto-follows the tail while working (poll ~2 s only while expanded and working), "Copy log".
  - Owner-only **"Tell Hermes…"** composer (private instruction), Cmd+Enter to send.
  - Rail keeps Delegate/Agent state/Last seen but uses the new avatar + state chip.
- **Home — "Agents at work"** strip above the task table (team-visible): one compact row per active agent — avatar with live ring, agent · owner, task title (link), now line or state, elapsed. Hidden when empty. No cards grid.
- **Agents page**: cards per agent (not a table) — avatar (large), name/handle/runtime, status chip, current task + now line, last seen, 14-day activity bars (tiny), setup checklist (skill version, MCP, watcher) with painted ticks/crosses, Rotate/Revoke in a ⋯ menu (reuse `viz::more` + menus). Revoked collapsed into a quiet "Revoked (2)" disclosure.
- **Connect an agent** — 3 steps in one dialog with a step indicator: (1) name, handle, runtime (cards with the runtime's name + one-line description); (2) the prompt in a code block, Copy, "shown once, contains a secret" warning, "I've pasted it" → (3) live wait: pulsing avatar "Waiting for Hermes to say hello…" polling `/api/user/agents` every 2 s → on `connected`, a checklist from `setup` animates in (skill · MCP · watcher) and "Done". Escape/Cancel safe at every step; step 3 can be left and revisited from the Agents card ("Waiting for first contact").
- Everywhere: design tokens only; painted glyphs; no blue selected borders; numbers white; no visible scrollbars; focus-visible states; accessible names on every control (accesskit labels); motion 150–250 ms ease-out except the working ring; every motion has the `AIRTRIBE_REDUCE_MOTION` path.
- Tests (`tests/agent_sessions_ui.rs`, harness pattern from `tests/agents_ui.rs`): stepper states, private sections absent for a non-owner viewer, connect flow steps 1→2→3→connected checklist, Home strip hidden when empty. Renders at 1440 and 820 into `docs/design-mocks/render/agent-sessions/`, read back, fixed.
