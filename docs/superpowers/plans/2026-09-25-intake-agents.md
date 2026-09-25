# Intake agents (Slack Agent first) — the contract

A person's agent can **create tasks for them** from what it reads (Slack first;
email/GitHub later use the same path). Linear's Triage is the model: intake
lands in a Triage state; the owner accepts or dismisses. The dashboard stays
universal — any person can connect an intake agent; nothing is Slack- or
Anmol-specific in the product.

Never kill a process you did not start; the dev server on :8080 is shared.
Tests: `DATABASE_URL=postgres://localhost:5433/acp_dev cargo test --features app`.
New migrations only. Read `PRODUCT.md` (visibility principles, personality).

## Owners
| Owner | Files |
| --- | --- |
| Server | `migrations/**`, `src/controllers/**`, `src/routes/**`, `src/models/**`, `src/cli/**` (additive), `tests/*.rs` except `*_ui.rs` |
| Client | `src/desktop/**`, `tests/*_ui.rs`, `docs/design-mocks/render/**` |
| Parent | `agent-kit/**`, the Slack agent runtime, docs, verification |

## Server
1. **Capability**: `agent.can_intake boolean NOT NULL DEFAULT false`, set on create (`POST /api/user/agents {…, canIntake}`) and toggleable by the owner (`PATCH /api/user/agents/{id} {canIntake}`). Agent JSON gains `canIntake`, `intakeProjectId`, `intakeStats: {triage, accepted, dismissed}`. Runtime list gains nothing new (a Slack agent runs on hermes/other).
2. **Triage state**: add `triage` to the task status CHECK and to both tracks: `triage → open` (accept) and `triage → dropped` (dismiss); nothing else leaves triage; no other state enters it. `done_at` untouched. Triage tasks count as not started in metrics; Home/My Tasks show them in their own "Triage" group (client).
3. **Intake project**: first intake by an agent creates (once) a project `"<Source> — <Owner name>"` (source from the request, e.g. "Slack — Anmol"), key `intake-<owner-handle>-<source>`, created_by = owner, description "Tasks <agent> filed from <source> for <owner>.", stored as `agent.intake_project_id`. Team-visible like any project. Owner may archive/rename it like any project; if archived/deleted, the next intake creates a fresh one.
4. **Task source + category**:
   - `task.category text NULL CHECK (category IN ('bug','feature','feedback','question','chore'))` — settable at intake and by any writer later (details PATCH accepts `category`).
   - `task_source (task_id PK → task CASCADE, kind text ('slack', …), source_key text, url text, channel text, author text, text text, received_at timestamptz, reason text, confidence real)`; `UNIQUE (agent_id, source_key)` — add `agent_id` column → agent; this is the dedupe guarantee.
   - Task JSON gains `category` and `source: {kind, url, channel, author, text, receivedAt, reason, confidence} | null` (team-visible — the message was posted where the team can see it… **except DMs**: if `channel` starts with `D`/is a DM (`source.private = true` in the request), `text` and `author` are returned only to the owner/admins (use the existing `sees_agent_private` rule); others see "From a direct message").
5. **Agent intake API** (agent token, `can_intake` required, readable 403 otherwise):
   - `POST /api/agent/intake {source: {kind, key, url, channel, channelName?, author, text, receivedAt, private?}, title, body, category, reason, confidence, priority?}` → creates the task in the intake project, status `triage`, assigned to the owner, `created_by` = owner, delegate none; returns the task. If `(agent_id, source.key)` exists → `409` with `{taskId}` in the message and nothing created.
   - `POST /api/agent/intake/{taskId}/append {source: {…}, text}` → adds a note (kind `note`, agent-authored) with the permalink, only on tasks this agent filed; dedupes the appended source key too (`409` if seen).
   - `GET /api/agent/intake/recent?days=14` → this agent's filed tasks `{id, title, category, status, source: {key, url, channel}}` for its own semantic dedupe.
   - MCP tools for agent tokens with `can_intake`: `intake_create`, `intake_append`, `intake_recent` (docs like the others).
6. **Owner triage** (session, task owner = assignee or admin): `POST /api/user/tasks/{id}/accept {projectId?}` (→ `open`; if `projectId` given, move the task into that project's first phase — only projects the viewer can see; readable errors) and `POST /api/user/tasks/{id}/dismiss {reason?}` (→ `dropped`, reason as a note). Both only from `triage`. Both write `change` audit rows.
7. **Counts/Home**: `/api/user/counts` gains `triage` (mine). Home payload `needsAttention` gains `{kind: "triage", taskId, title, agentName, body: category}` items for my triage tasks (or a separate `triage: [...]` array — pick one, document it).
8. Tests `tests/intake.rs`: capability required; first intake creates the project once; dedupe 409; append dedupe; triage transitions (accept/dismiss, others refused); DM source privacy; recent listing; MCP tools; archived intake project → new one.

## Client (calm, precise, alive; no chatbot bubbles)
- **Connect flow**: step 1 gains a capability toggle "Can create tasks for me (intake)" with one line of explanation; runtime tiles unchanged. Agent cards show an "Intake" badge, intake stats (triage / accepted / dismissed) and a link to its intake project.
- **Triage**: a "Triage" group at the top of My Tasks and a Triage count in the sidebar (from `/counts`); Home needs-attention shows triage items. Each triage row: category badge, title, source line ("Slack · #issues-and-feedback · Priya · 12m"), and inline **Accept** (with a project picker, default = keep in intake project) and **Dismiss** (optional reason) — also in the row right-click menu.
- **Task page**: a **Source** card under the description for tasks with `source`: source glyph (painted Slack-ish hash mark — not the Slack logo), channel, author, time, the original text as a quote block (not a bubble), "Open in Slack" link, and the agent's reason + confidence ("Filed by Slack Agent — looks like a bug: checkout fails for saved cards · 0.86"). Category badge next to the title, editable via a small picker.
- Category badges: bug (red-ish), feature (purple), feedback (blue), question (slate), chore (slate) — token colours, readable contrast.
- Tests `tests/intake_ui.rs` + renders at 1440/820 into `docs/design-mocks/render/intake/`: connect with intake toggle, My Tasks triage group with accept/dismiss, task source card (public and DM-private as teammate), agent card with intake stats. Read them back, fix.
