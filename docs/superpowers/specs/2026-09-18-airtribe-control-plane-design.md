# Airtribe Control Plane — Design

**Date:** 2026-09-18
**Status:** Approved design, pre-implementation

## 1. Purpose

A single system for the Airtribe engineering team to track projects, their
phases, and their tasks; to hold the links, PRs, and documents that belong to
each; and to let AI agents (Hermes, Claude) read that state and do work against
it under human approval.

It is the system of record. Tasks live here and nowhere else. GitHub PRs and
issues are linked as artifacts, not mirrored as tasks.

### Success criteria

- Every engineer can list, filter, assign, and update tasks from their own Mac.
- Every link, PR, and document for a piece of work is findable from the task.
- An agent can find claimable work, do it, and report back without a human
  copying context by hand.
- No agent can change shared state without a human approving that change.
- Every mutation is attributable to a person, including those an agent made on
  a person's behalf.

### Non-goals

- Sprints, story points, burndown charts, or time tracking.
- Replacing GitHub, Slack, or Google Docs.
- Two-way sync with any external tracker.

## 2. Architecture

One Rust binary serving three surfaces over one core.

```
  web UI   ─┐
  acp CLI  ─┼─►  acp-server (Axum)  ─►  Postgres
  MCP shim ─┘          │
   (hermes/claude)     ├─ authz: Google SSO, scoped agent tokens
                       ├─ audit log: every mutation, actor + on_behalf_of
                       └─ proposal queue: agent writes awaiting approval
```

Database credentials exist only on the server. Laptops and agents hold
per-identity bearer tokens and speak HTTPS.

### Stack

| Concern | Choice |
|---|---|
| HTTP | Axum |
| Database | Postgres via `sqlx` (runtime-checked queries — see the note below) |
| Migrations | `sqlx::migrate`, timestamped plain `.sql` files |
| Background jobs | `job` table + Postgres `LISTEN/NOTIFY` |
| Auth | Google Workspace OIDC (`hd=airtribe.live`); bearer tokens |
| CLI | same workspace, static binary, token in macOS Keychain |

Two deliberate departures from a literal mycohort translation, both to reduce
maintained machinery:

- **No ORM.** Relations are explicit join queries in `models/`. SeaORM would
  give Objection-style relations at the cost of a codegen layer and worse
  diagnostics, and the join-by-hand style has been fine at this size.

  **Correction (2026-09-20):** an earlier version of this section justified the
  choice by saying sqlx checks SQL against the schema at compile time so a typo
  fails `cargo build`. That is a real sqlx feature, but it requires the
  `query!`/`query_as!` macros, and the codebase uses the runtime-checked
  `query`/`query_as` functions in all 72 query sites. A typo surfaces as a
  runtime error, not a build failure. The decision to skip an ORM stands on the
  other grounds; the stated reason was wrong.

  Adopting the macros is available and worth considering: it needs a
  `.sqlx` offline cache committed to the repo (`cargo sqlx prepare`) and
  regenerated whenever the schema changes, plus a reachable database or that
  cache in CI. Deferred, not rejected.
- **No Redis/BullMQ.** A `job` table plus `LISTEN/NOTIFY` removes a dependency,
  makes jobs transactional with the data that enqueued them, and leaves the
  queue inspectable with plain SQL.

Both are reversible and neither constrains the schema.

## 3. Codebase conventions

The directory layout is a near 1:1 translation of `mycohort-api`, so anyone who
can navigate that repo can navigate this one.

```
src/
  main.rs              ← server.js       entry point
  app.rs               ← app.js          router assembly
  constants.rs         ← constant.js
  config/              ← config/
  models/              ← models/         one file per table
  routes/
    admin/             ← routes/admin/
    user/              ← routes/user/
    services/mcp/      ← routes/services/mcp/
  controllers/         ← controllers/    business logic
  services/            ← services/       github.rs, google.rs, slack.rs
  jobs/                ← bull/workers/   background workers
  middleware/          ← plugins/request_decorators/
  lib/errors/          ← lib/errors/     AppError
  utils/
migrations/            ← api/db/migrations/
tests/
```

Carried over from mycohort:

- Database columns are snake_case.
- Rust structs serde-rename to camelCase on the wire.
- Responses use the `{ "success": true, "data": ... }` envelope.
- One `AppError` type with a code enum, one central error handler.
- Each route module exposes `fn routes() -> Router`, registered in `app.rs` the
  way Fastify plugins are registered in `app.js`.

## 4. Data model

Nine tables. Every view and report is a query over these.

| Table | Purpose |
|---|---|
| `person` | Google identity, email, role (`member` \| `admin`) |
| `agent_token` | token hash, label, `owner → person`, `scopes[]`, `expires_at`, `revoked_at` |
| `project` | key, name, status, `lead → person` |
| `phase` | `project →`, position, name, status, `gate` |
| `task` | `phase →`, title, body, status, priority, `assignee_kind`, assignee, `claimed_by`, `claim_expires_at`, `blocked_by[]` |
| `artifact` | polymorphic parent (project \| phase \| task), `kind` (pr \| doc \| link), url, title, `metadata jsonb`, `added_by` |
| `change` | audit log and proposal queue: actor, `on_behalf_of`, target, op, `patch jsonb`, state, `applied_at` |
| `job` | queue: kind, payload, `run_after`, attempts, `locked_by` |
| `run_log_line` | agent session output: `task →`, seq, text |

### `change` is the only write path

Every mutation, human or agent, writes a `change` row.

- An actor holding `write` mutates directly: the effect and an `applied` change
  row commit in the same transaction.
- An actor holding only `propose` **does not mutate at all**. The controller
  records the full intent in `change.patch` and returns
  `{"status":"proposed","changeId":...}`. Approval replays that intent through
  the same controller with an applying actor.

History, undo, attribution, and the agent guardrail all derive from this one
table rather than four separate mechanisms.

**Implementation note (added 2026-09-20).** The first cut of this got it wrong:
the controller performed the mutation and merely marked the change row
`pending`, so a `propose`-scoped token really did change shared state. The
regression test `propose_token_must_not_mutate_shared_state` exists to keep that
from returning. Any future controller must short-circuit to `propose()` before
its first write, not after.

**Known debt.** Approval replays through controllers that own their own
transactions, so a replay can succeed while the subsequent `state` flip fails.
The result is a change stuck in `pending` whose effect already applied — visible
in the inbox and recoverable by rejecting it, never silent. Closing the window
means threading an optional transaction through all seven mutating controllers;
deferred until something forces it.

### Artifacts are polymorphic by design

A PR hangs off a task, a spec doc off a phase, a dashboard link off a project.
One table, one `/artifacts` endpoint, one MCP tool.

### Claims are leases, not locks

`claimed_by` with `claim_expires_at`. A laptop that sleeps or dies releases its
work when the lease lapses. There is no unlock command and no stuck task.

## 5. Delegation model

The server never reaches into a laptop.

Assigning a task to an agent sets `assignee_kind='agent'` and makes the task
claimable. A laptop running Hermes polls for claimable work, claims one under a
lease, heartbeats while working, streams run-log lines back, and submits its
result as a `pending` change.

```
  assign ──► task claimable
                 │
  laptop poll ──►│ work_claim (lease)
                 ├─ work_heartbeat  (extends lease)
                 ├─ run_log_append  (streams output)
                 └─ task_update     ──► change(pending)
                                             │
                          acp approve <id> ──┘──► applied
```

This survives laptops being asleep, needs no inbound network to any Mac, and
matches the proposed-then-approved flow already in use with Hermes.

Rejected alternatives: server-side dispatch (needs reachability and credentials
into every laptop, plus runner health monitoring) and assignment-only (the
tracker never learns what the agent did).

## 6. Auth and guardrails

**Humans.** Web UI authenticates with Google OAuth restricted to the
`airtribe.live` domain. The CLI uses a device-code flow and stores its token in
the macOS Keychain.

**Agents.** Agent tokens are separate non-human identities. Each is bound to one
human owner, carries an explicit scope set, expires, and can be revoked on its
own without affecting its owner. Every action an agent takes records both
`actor` and `on_behalf_of`.

**Scopes.** An agent token cannot hold a capability it was not minted with.

| Scope | Grants |
|---|---|
| `read` | read projects, phases, tasks, artifacts |
| `claim` | claim work, heartbeat, release, append run logs |
| `propose` | create and update tasks and artifacts as `pending` changes |
| `write` | apply those changes directly; minted rarely, humans only |

**Offboarding.** Revoking a person cascades to every agent token they own.

## 7. Surfaces

All three surfaces call the same controllers. No business logic lives in a
surface.

### HTTP API

`/api/admin/*`, `/api/user/*`, `/api/services/mcp/*` — the same three
namespaces as mycohort. Session cookie for the UI, bearer token for CLI and
agents.

### CLI (`acp`)

```
acp task ls --project acp --phase 2 --assignee me --status open
acp task new "wire MCP auth" --phase 2 --assign hermes
acp task move <id> in_review
acp link <id> --pr <url> --doc <url>
acp approve <change-id>
acp token mint --for hermes --scope read,propose
acp mcp                       # stdio MCP shim
```

### MCP

Tools are grouped so that a token's scope determines which exist:

| Scope | Tools |
|---|---|
| `read` | `project_list`, `task_search`, `task_get`, `artifact_list`, `phase_status` |
| `claim` | `work_claim`, `work_heartbeat`, `work_release`, `run_log_append` |
| `propose` | `task_update`, `task_create`, `artifact_add` — all write `pending` |
| `write` | the same three, applied directly |

Following the precedent in `mycohort-api/api/routes/services/mcp`, each tool
ships a companion markdown document served as an MCP resource, so an agent can
orient itself without bespoke prompt glue.

Transport is remote MCP over HTTP, plus a stdio shim (`acp mcp`) so Hermes and
Claude Code can attach locally without network configuration.

## 8. Error handling

`AppError` carries a code enum and maps to a response through one central
`IntoResponse` implementation, mirroring mycohort's error handler plugin.

Agent-facing errors return remediation text rather than stack traces, so a model
can correct itself without a human reading the log.

## 9. Testing

- Controller tests run against a real throwaway Postgres via `sqlx::test`. The
  schema constraints carry half the design, so the database is not mocked.
- Each route module has HTTP-level tests.

Invariants that get explicit tests:

1. A token without the `write` scope can never produce an `applied` change.
2. A lapsed lease frees its task for another claimant.
3. Every mutation leaves exactly one `change` row.
4. Revoking a person revokes every agent token they own.

## 10. Delivery phases

Each phase is independently useful. Work can stop after any one of them and
leave something the team uses daily.

| Phase | Ships | Usable when done |
|---|---|---|
| **P0 Backbone** | schema, migrations, `AppError`, config, `change` write path, health endpoint | nothing user-facing; everything rests on it |
| **P1 Core + CLI** | project/phase/task/artifact CRUD, Google SSO, tokens, `acp` binary | the team tracks real work from the terminal |
| **P2 MCP** | tool surface, scopes, proposal queue, `acp approve`, stdio shim | Hermes and Claude can read and propose |
| **P3 Delegation** | claim-lease, heartbeats, run logs, `job` queue | agents pick up work and report back |
| **P4 Web UI** | list, filter, and board views; approval inbox | access without a terminal |

P1 ends by loading this project's own phases into the tool. From P2 onward, the
work is tracked in the system itself.
