# Airtribe Control Plane

Task tracker and control plane for the Airtribe engineering team. Projects have
phases, phases have tasks, and anything can carry artifacts (PRs, docs, links).
Humans drive it from a CLI; agents get a scoped, audited surface of their own.

- **Design:** `docs/superpowers/specs/2026-09-18-airtribe-control-plane-design.md`
- **Plans:** `docs/superpowers/plans/`

## Status

| Phase | State |
|---|---|
| P0 Backbone — schema, errors, `change` write path | done |
| P1 Core & CLI — tokens, auth, phases, tasks, artifacts, `acp` | done |
| P2 MCP — tool surface, approval queue | done |
| P3 Delegation — claim-lease, run logs, job worker | done |
| P4 Web UI | next |

## Setup

Requires Rust 1.96+ and Postgres 17.

```bash
brew services start postgresql@17
createdb -p 5433 acp_dev          # 5432 is taken by Docker on some machines
cp .env.example .env              # edit DATABASE_URL if your port differs
cargo build
```

Migrations run automatically when the server starts.

## Running

```bash
cargo run --bin acp-server        # listens on :8080
```

Bootstrap yourself a token. `acp-admin` talks to the database directly, which is
the only way to mint the first one — every other path requires a token already.

```bash
cargo run --bin acp-admin -- add-person you@airtribe.live "Your Name"
cargo run --bin acp-admin -- mint laptop --owner you@airtribe.live --scopes read,write
export ACP_TOKEN=<the token it prints once>
```

## Using it

```bash
acp project new loop "Airtribe Control Plane"
acp project ls

acp phase new <project-id> "P2 MCP" --position 2
acp phase ls <project-id>

acp task new <phase-id> "wire MCP auth"
acp task ls --phase <phase-id> --status open
acp task move <task-id> in_review
acp task assign <task-id> --to you@airtribe.live
acp task assign <task-id> --agent hermes

acp link task <task-id> --kind pr --url https://github.com/... --title "P1"
```

## Delegating to an agent

Assign a task to an agent and it becomes claimable. The server never reaches
into a laptop; the laptop pulls.

```bash
acp task assign <task-id> --agent hermes
```

The agent then claims it under a five-minute lease, heartbeats while it works,
streams run-log lines, and finishes by *proposing* a change. A laptop that
sleeps or dies has its lease lapse and the task returns to the pool — there is
no unlock command because a claim is a lease, not a lock.

`scripts/rehearse-delegation.sh` drives that whole loop against a running
server, including the cases that matter: a second worker cannot steal a live
lease, a non-holder cannot write to the log, and an agent cannot approve its
own proposal.

## Connecting an agent over MCP

```bash
export ACP_TOKEN=<a read,claim,propose token>
./target/debug/acp-mcp          # newline-delimited JSON-RPC on stdin/stdout
```

Point Hermes or Claude Code at that command. The shim holds the token, so the
agent never sees it. Every tool ships a companion document at
`acp://docs/<tool>`; `acp://docs/work_claim` describes the full working loop.

## How writes work

Every mutation — human or agent — writes exactly one row to the `change` table
inside the same transaction as its effect. That single mechanism gives us the
audit log, the history, and the agent guardrail.

A token's scopes decide what happens:

| Scope | Effect |
|---|---|
| `read` | read-only |
| `claim` | claim agent work, heartbeat, append run logs (P3) |
| `propose` | mutations land as `pending`, awaiting human approval |
| `write` | mutations apply immediately |

So an agent holding `read,propose` can never change shared state on its own.

## Layout

Mirrors `mycohort-api`, so the two repos navigate the same way.

```
src/
  app.rs          router assembly        ← app.js
  models/         one file per table     ← models/
  controllers/    business logic         ← controllers/
  routes/user/    HTTP surface           ← routes/user/
  middleware/     auth extractor         ← plugins/request_decorators/
  lib/errors/     AppError               ← lib/errors/
  cli/            CLI client
  bin/            acp, acp-admin, acp-server
migrations/       plain .sql             ← api/db/migrations/
```

## Tests

```bash
cargo test
```

Tests run against real throwaway Postgres databases via `sqlx::test`; the schema
constraints carry half the design, so the database is not mocked.
