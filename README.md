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
| P4 Web UI — board, filters, approval inbox, live run log | done |
| P5 Native Mac app — egui, Keychain, .app bundle | done |

## The Mac app

A native desktop client in Rust — egui, no webview, no HTML. It talks to the
same API as everything else, so it is a front end, not a second system.

```bash
./scripts/bundle-mac.sh        # builds "target/Airtribe Control Plane.app"
open "target/Airtribe Control Plane.app"
```

Or run it straight from cargo while developing:

```bash
cargo run --features app --bin acp-app
```

Sign in once by pasting a token; it is stored in the **macOS Keychain**, not a
dotfile, because it grants write access to shared team state. `ACP_TOKEN` in the
environment overrides the stored credential, which is useful for pointing the
app at a scratch server. `ACP_URL` sets the server (default
`http://localhost:8080`).

| Screen | What it shows |
|---|---|
| Board | projects, then phases in order with their tasks; filter by status and assignee kind |
| Inbox | pending agent proposals, each written as a sentence, with approve and reject |
| Task | detail, artifacts, change history, and the run log, live while an agent works |

Delegated work is marked twice over — a purple pill and a spine down the left
edge of the card — so you can see what an agent is holding without reading.

The GUI stack sits behind the `app` cargo feature, so `cargo build` for the
server does not compile egui at all.

## The web UI

```bash
cargo run --bin acp-server
./scripts/seed-demo.sh          # optional: loads the tool with its own history
open http://localhost:8080/login
```

Paste a token to sign in; it is stored in an httpOnly, SameSite=Strict cookie.
The cookie is a pointer to the token, not a copy of its authority — revoking the
token ends the session on the next request.

| Route | What it is |
|---|---|
| `/` | every project with its phase progress |
| `/projects/{id}` | phases in order with their tasks, filtered without a page reload |
| `/inbox` | pending agent proposals, each rendered as a sentence |
| `/tasks/{id}` | detail, artifacts, change history, and a live run log |

A session without the `write` scope sees the inbox but no approve buttons, and
the POST handlers reject it regardless — the UI shows the same scope truth the
API enforces.

The run log polls every two seconds *only* while a task is `in_progress`, and
appends incrementally from a cursor, so watching an agent work costs one small
request per tick and stops by itself when the work finishes.

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

## Verifying it works

```bash
cargo test                             # 59 tests
./scripts/rehearse-delegation.sh       # the full agent loop over real HTTP
./scripts/verify-p2.sh                 # the proposal guardrail over real MCP
```

Unit tests run against real throwaway Postgres databases via `sqlx::test`; the
schema constraints carry half the design, so the database is not mocked.

The two scripts matter more than the test count. They drive a running server the
way Hermes does and assert the *failure* modes, not the happy path: a second
worker cannot steal a live lease, a non-holder cannot write to a run log, an
agent cannot approve its own proposal, and a lapsed lease returns its task to
the pool.

## Deploying

```bash
docker build -t acp .
docker run -p 8080:8080 -e DATABASE_URL=postgres://... acp
```

Templates, MCP docs, HTMX, and CSS are compiled into the binary, so the runtime
image is the binary on Debian slim under a non-root user, with no source tree
and no asset directory. Migrations run at startup.

`/health` is liveness and touches nothing but the process. `/health/ready`
checks the database. They are separate on purpose: a sick database should not
get the container killed and restarted to no purpose.

## Known debt

- **Google SSO is not wired.** Login takes a pasted token. SSO mints into the
  same `agent_token` table, so it replaces one handler and nothing else.
- **Approval is not fully transactional.** The replay calls controllers that own
  their own transactions, so a replay can succeed while the state flip fails.
  The result is a proposal stuck in `pending` whose effect already applied —
  visible in the inbox, recoverable by rejecting it, never silent. Closing the
  window means threading a transaction through all seven mutating controllers.
- **A few reads are inline SQL in route handlers** (get-project-by-id,
  get-task-by-id, the inbox label lookup), marked with `ponytail:` comments,
  because the controllers did not expose them and the work was parallelised.
  Fold them into controllers when a second caller appears.
