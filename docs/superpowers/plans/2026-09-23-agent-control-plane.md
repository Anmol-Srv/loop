# Agent control plane v1 — the contract

Spec: `docs/superpowers/specs/2026-09-23-agent-control-plane-design.md`. Read it
first; this file only fixes owners, wire shapes and done.

Never kill a process you did not start (track yours by `$!`/port). The dev
server on :8080 and Postgres on :5433 are shared — use your own port for
anything you run. If a build fails in a file you do not own, wait and retry.
Tests: `DATABASE_URL=postgres://localhost:5433/acp_dev cargo test --features app`.

## Owners — disjoint files

| Owner | Files |
| --- | --- |
| **Server** | `migrations/**`, `src/controllers/**`, `src/routes/**`, `src/models/**`, `src/middleware/**`, `src/jobs/**`, `src/app.rs`, `src/cli/**` (except `client.rs` — shared, additive only), `src/bin/acp.rs`, `src/bin/acp-admin.rs`, `tests/*.rs` (new files), `reactor/` (delete) |
| **Client** | `src/desktop/**` |
| **Parent** | `agent-kit/**` (skill + onboarding templates), `skills/**`, `docs/**`, `scripts/**`, Hermes |

## Wire shapes (both sides build against these)

JSON is camelCase, wrapped `{success, data}` as today.

Agent (owner view): `{id, handle, name, runtime, status: "waiting"|"connected"|"revoked", connectedAt, lastSeenAt, createdAt, activeTasks: number}`

Session routes (assignee/owner only):
- `GET /api/user/agents` → `Agent[]` (mine)
- `POST /api/user/agents {handle, name, runtime}` → `{agent, token, prompt}` — `prompt` is the full paste-once onboarding text (spec §6), token shown only here.
- `POST /api/user/agents/{id}/rotate` → `{agent, token, prompt}`
- `DELETE /api/user/agents/{id}` → revoke (also un-delegates its open tasks with a `taken_back` event)
- `POST /api/user/tasks/{id}/handoff {agentId}` · `POST /api/user/tasks/{id}/takeback`
- `POST /api/user/tasks/{id}/answer {body}`
- `POST /api/user/tasks/{id}/review {decision: "approve"|"changes", body?}`

Task JSON (everywhere a task is returned) gains:
`delegate: null | {id, handle, name, state, lastSeenAt}` and `reviewTarget: string|null`.
Note JSON gains `kind` and `agent: null | {id, name}` (author null for agent notes).
`/api/user/home` needs-attention gains items `{kind: "question"|"review", taskId, title, agentName, body}` for tasks where I am the assignee.

Agent routes: exactly spec §5. Errors are readable sentences (they are read by models).

## Server — done

Migration(s) for spec §1–4 with backfill; triggers; `/api/agent/*`; session routes above; MCP tools + Streamable-HTTP compliance + `acp://skill`; `include_str!("../../agent-kit/airtribe-agent/SKILL.md")` and `agent-kit/onboarding/<runtime>.md` templates rendered with `{{server}}`, `{{handle}}`, `{{name}}`, `{{owner}}` (server URL from `PUBLIC_URL` env, else the request's Host/X-Forwarded-Proto); CLI verbs; delete `reactor/`, `/work/*`, lease reaper and `work_*` MCP tools and their tests. Tests per spec §9 in `tests/agents.rs`, whole suite green. Prove the MCP handshake with a real client: `hermes mcp test` is the Parent's job, but a scripted initialize → notifications/initialized → tools/list over curl must pass.

## Client — done

Spec §7: Agents page (sidebar item under Projects) with Connect modal showing the prompt once with a Copy button and a clear "shown once" line, status/last seen/active tasks, Rotate, Revoke (confirm); task page hand-off/take-back, delegate chip, kind-styled agent notes, question callout + answer box, review panel with Approve / Request changes; Home needs-attention items; agent marker in task tables. Design system tokens only, painted glyphs (Nunito has no arrows/ticks). Kittest coverage for the modal and review panel; renders via `scripts/render-pages.sh` at 1440/1100/820 read back and checked. Until the server lands, build against the wire shapes with the existing mock/seed pattern in tests.
