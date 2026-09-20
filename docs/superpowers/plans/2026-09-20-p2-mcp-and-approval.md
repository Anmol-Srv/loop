# Airtribe Control Plane — P2 MCP & Approval Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the proposal guardrail real, give humans a way to approve or reject proposals, and expose the whole system to Hermes and Claude over MCP.

**Architecture:** A `propose`-scoped actor never mutates: the controller records the full intent in `change.patch` and stops. Approval replays that intent through the same controller with an applying actor. The MCP surface is a thin JSON-RPC layer over the existing controllers, with the tool list filtered by the caller's token scopes.

**Tech Stack:** Rust 1.96, Axum 0.8, sqlx 0.8, clap 4, reqwest 0.13.

**Spec:** `docs/superpowers/specs/2026-09-18-airtribe-control-plane-design.md`

**Builds on:** P0 backbone and P1 core & CLI, both merged to `master`.

**A note on this plan's density:** unlike the P0 and P1 plans, this one gives
complete code only for genuinely new logic (the proposal split, the apply
dispatcher, the JSON-RPC layer). For work that repeats an established shape —
a route module, a controller, a test harness — it names the interface and the
file to copy the pattern from. Every executor of this plan has the repository
in front of them, and `src/routes/user/phase.rs` is a better spec for "what a
route module looks like here" than a transcription of it would be.

## Global Constraints

- All P0 and P1 constraints hold: `snake_case` columns, camelCase on the wire,
  the `{ success, data }` envelope, no ORM, no Redis, Postgres on port **5433**.
- **A `propose`-scoped actor must never change shared state.** This is the
  single invariant P2 exists to enforce. Any code path that lets one through is
  a defect, not a tradeoff.
- Every mutation still writes exactly one `change` row.
- `change.patch` for a proposal must hold every argument needed to replay the
  operation later. A proposal that cannot be replayed is a bug.
- MCP tools are filtered by token scope. A tool the caller cannot use must not
  appear in `tools/list` — absent, not merely rejected on call.
- Scope names remain exactly `read`, `claim`, `propose`, `write`.

---

### Task 1: Proposals must not mutate

**Files:**
- Modify: `src/models/change.rs`
- Modify: `src/controllers/project.rs`, `src/controllers/phase.rs`, `src/controllers/task.rs`, `src/controllers/artifact.rs`
- Modify: `src/routes/user/project.rs`, `src/routes/user/phase.rs`, `src/routes/user/task.rs`, `src/routes/user/artifact.rs`
- Test: `tests/auth.rs` (already contains the failing regression test), `tests/proposal.rs`

**Interfaces:**
- Produces:
  - `acp_server::models::change::Outcome<T>` — `enum Outcome<T> { Applied(T), Proposed { change_id: Uuid } }`, `Serialize`, tagged so the wire form is `{"status":"applied","entity":{...}}` or `{"status":"proposed","changeId":"..."}`.
  - `acp_server::models::change::propose(db: &PgPool, actor: &Actor, target_type: TargetType, target_id: Uuid, op: Op, patch: Value) -> AppResult<Uuid>` — records a `pending` change in its own transaction and performs no other write.
  - Every controller's mutating function returns `AppResult<Outcome<T>>` instead of `AppResult<T>`.

Each mutating controller gains this shape at the top, before any write:

```rust
if !actor.can_apply {
    let id = Uuid::new_v4();  // the id the entity will get if approved
    let change_id = propose(&state.db, actor, TargetType::Task, id, Op::Create,
        json!({ "phase_id": phase_id, "title": title, "body": body, "priority": priority })).await?;
    return Ok(Outcome::Proposed { change_id });
}
```

For `Op::Create` the pre-generated `target_id` is also the id the row will be
given on approval, so a proposal can be referenced before it exists. Change the
`INSERT` statements to supply `id` explicitly rather than relying on the column
default.

- [ ] **Step 1: Confirm the regression test fails**

Run: `cargo test --test auth propose_token_must_not_mutate_shared_state`
Expected: FAIL — `left: 1, right: 0`.

- [ ] **Step 2: Add `Outcome` and `propose` to `src/models/change.rs`**

```rust
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Outcome<T> {
    #[serde(rename = "applied")]
    Applied { entity: T },
    #[serde(rename = "proposed")]
    Proposed {
        #[serde(rename = "changeId")]
        change_id: Uuid,
    },
}

/// Record an intended mutation without performing it. Used when the actor
/// lacks the `write` scope, so the effect waits for human approval.
pub async fn propose(
    db: &sqlx::PgPool,
    actor: &Actor,
    target_type: TargetType,
    target_id: Uuid,
    op: Op,
    patch: serde_json::Value,
) -> AppResult<Uuid> {
    let mut tx = db.begin().await?;
    let id = record(&mut tx, actor, target_type, target_id, op, patch).await?;
    tx.commit().await?;
    Ok(id)
}
```

`record` is unchanged and keeps its debug assertion role: it still writes
`pending` whenever `actor.can_apply` is false.

- [ ] **Step 3: Split every mutating controller**

Apply the shape above to, in order:
`project::create`, `phase::create`, `phase::set_status`, `task::create`,
`task::set_status`, `task::assign`, `artifact::add`.

Patch contents, exactly:

| Function | `TargetType` | `Op` | `patch` keys |
|---|---|---|---|
| `project::create` | `Project` | `Create` | `key`, `name` |
| `phase::create` | `Phase` | `Create` | `project_id`, `name`, `position`, `gate` |
| `phase::set_status` | `Phase` | `Update` | `status` |
| `task::create` | `Task` | `Create` | `phase_id`, `title`, `body`, `priority` |
| `task::set_status` | `Task` | `Update` | `status` |
| `task::assign` | `Task` | `Update` | `person_email` or `agent_label`, whichever was given; `null` for unassign |
| `artifact::add` | `Artifact` | `Create` | `parent_type`, `parent_id`, `kind`, `url`, `title` |

For the `Update` cases the `target_id` is the existing row's id. Validate the
target exists *before* recording the proposal, so a proposal against a missing
task still returns 404 rather than queueing garbage.

- [ ] **Step 4: Update the route handlers**

Each handler now returns `ApiResponse<Outcome<T>>`. No other change; the
handlers already call `caller.can_mutate()`.

- [ ] **Step 5: Update existing tests for the new response shape**

Tests that read `json["data"]["key"]` become `json["data"]["entity"]["key"]`.
Affected: `tests/project_api.rs`, `tests/phase_api.rs`, `tests/task_api.rs`,
`tests/artifact_api.rs`.

- [ ] **Step 6: Write the proposal test**

Create `tests/proposal.rs` covering:

1. A `propose` token creating a project returns `{"status":"proposed","changeId":...}`, leaves `project` empty, and leaves exactly one `pending` change whose `patch` contains both `key` and `name`.
2. A `propose` token updating a task's status leaves the task's status unchanged.
3. A `propose` token updating a *nonexistent* task returns 404 and records no change.
4. A `write` token still returns `{"status":"applied","entity":{...}}` and mutates.

- [ ] **Step 7: Run the full suite**

Run: `cargo test`
Expected: PASS, including `propose_token_must_not_mutate_shared_state`.

- [ ] **Step 8: Commit**

```bash
git add src tests
git commit -m "fix: proposals no longer mutate shared state before approval"
```

---

### Task 2: Approve and reject

**Files:**
- Create: `src/controllers/approval.rs`
- Create: `src/routes/user/change.rs`
- Modify: `src/controllers/mod.rs`, `src/routes/user/mod.rs`, `src/app.rs`, `src/bin/acp.rs`
- Test: `tests/approval.rs`

**Interfaces:**
- Consumes: `Outcome`, `propose`, every controller from Task 1.
- Produces:
  - `acp_server::models::change::ChangeRow { id, actor, on_behalf_of, target_type, target_id, op, patch, state, applied_at, created_at }` — `Serialize`, `sqlx::FromRow`.
  - `acp_server::controllers::approval::list_pending(state: &AppState) -> AppResult<Vec<ChangeRow>>`
  - `acp_server::controllers::approval::approve(state: &AppState, approver: &Actor, change_id: Uuid) -> AppResult<ChangeRow>`
  - `acp_server::controllers::approval::reject(state: &AppState, approver: &Actor, change_id: Uuid) -> AppResult<ChangeRow>`
  - Routes `GET /api/user/changes/pending`, `POST /api/user/changes/{id}/approve`, `POST /api/user/changes/{id}/reject`
  - CLI `acp pending`, `acp approve <id>`, `acp reject <id>`

`approve` replays the proposal by dispatching on `(target_type, op)` and calling
the same controller with an applying actor derived from the approver:

```rust
let replay_actor = Actor {
    label: format!("{} (approved by {})", change.actor, approver.label),
    person_id: approver.person_id,
    can_apply: true,
};
```

The dispatcher must be exhaustive over the seven operations in Task 1's table.
An unrecognised combination returns `AppError::Internal`, never a silent no-op.

Approval is transactional: the replay and the `change.state` transition to
`approved` commit together, so a failed replay leaves the proposal pending and
returns the underlying error.

Only a caller with the `write` scope may approve or reject. A `propose` token
approving its own proposal must be rejected with 403 — otherwise the guardrail
is decorative.

- [ ] **Step 1: Write the failing test**

Create `tests/approval.rs` covering:

1. Propose a project with a `propose` token, approve with a `write` token, then the project exists, the change reads `approved`, and `applied_at` is set.
2. Propose, then reject: no project exists, the change reads `rejected`.
3. A `propose`-scoped token calling approve gets 403 and the change stays `pending`.
4. Approving an already-approved change gets 409.
5. `GET /api/user/changes/pending` lists only pending changes.
6. A proposal whose replay fails (propose a project key, then create that key directly with a write token, then approve) leaves the change `pending` and returns 409.

- [ ] **Step 2: Run it and confirm failure**

Run: `cargo test --test approval`
Expected: FAIL — the routes 404.

- [ ] **Step 3: Write `ChangeRow` and the approval controller**

Follow the controller pattern in `src/controllers/phase.rs`. The dispatcher:

```rust
async fn replay(state: &AppState, actor: &Actor, change: &ChangeRow) -> AppResult<()> {
    let p = &change.patch;
    let uuid_at = |k: &str| -> AppResult<Uuid> {
        p.get(k).and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
            .ok_or_else(|| AppError::Internal(format!("proposal is missing '{k}'")))
    };
    let str_at = |k: &str| -> AppResult<String> {
        p.get(k).and_then(|v| v.as_str()).map(str::to_string)
            .ok_or_else(|| AppError::Internal(format!("proposal is missing '{k}'")))
    };

    match (change.target_type.as_str(), change.op.as_str()) {
        ("project", "create") => { crate::controllers::project::create(state, actor, str_at("key")?, str_at("name")?).await?; }
        ("phase", "create") => { /* project_id, name, position, gate */ }
        ("phase", "update") => { /* set_status */ }
        ("task", "create") => { /* phase_id, title, body, priority */ }
        ("task", "update") => { /* status, or person_email / agent_label */ }
        ("artifact", "create") => { /* parent_type, parent_id, kind, url, title */ }
        (t, o) => return Err(AppError::Internal(format!("cannot replay {t}/{o}"))),
    }
    Ok(())
}
```

Fill in every branch. For `task/update`, distinguish a status change from an
assignment by which key the patch carries.

- [ ] **Step 4: Write the routes**

Follow `src/routes/user/phase.rs`. All three require `caller.require("write")?`.

- [ ] **Step 5: Register and wire**

Append `pub mod approval;` to `src/controllers/mod.rs`, `pub mod change;` to
`src/routes/user/mod.rs`, and merge the router in `src/app.rs`.

- [ ] **Step 6: Add the CLI subcommands**

In `src/bin/acp.rs` add `Pending`, `Approve { id }`, `Reject { id }` to the
top-level `Command` enum, following the existing arms.

- [ ] **Step 7: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src tests
git commit -m "feat: add approval queue with replay of proposed changes"
```

---

### Task 3: The MCP surface

**Files:**
- Create: `src/routes/services/mod.rs`, `src/routes/services/mcp/mod.rs`, `src/routes/services/mcp/tools.rs`, `src/routes/services/mcp/protocol.rs`
- Create: `src/routes/services/mcp/docs/*.md` (one per tool)
- Modify: `src/routes/mod.rs`, `src/app.rs`
- Test: `tests/mcp.rs`

**Interfaces:**
- Consumes: every controller, plus `Caller` for scope filtering.
- Produces:
  - `POST /api/services/mcp` speaking MCP JSON-RPC 2.0.
  - Methods: `initialize`, `tools/list`, `tools/call`, `resources/list`, `resources/read`.
  - `acp_server::routes::services::mcp::tools::for_scopes(scopes: &[String]) -> Vec<ToolDef>`

Tool definitions, grouped by the scope that unlocks them:

| Scope | Tools |
|---|---|
| `read` | `project_list`, `phase_status`, `task_search`, `task_get`, `artifact_list` |
| `propose` | `task_create`, `task_update`, `artifact_add` |
| `write` | the same three, but their results apply immediately |

`claim`-scoped tools (`work_claim`, `work_heartbeat`, `work_release`,
`run_log_append`) are **P3**. Do not implement them here.

JSON-RPC shape — note this is *not* the `{ success, data }` envelope, because
MCP clients require the standard form:

```json
{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"..."}]}}
{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"unknown tool 'x'"}}
```

Error codes: `-32700` parse error, `-32601` method not found, `-32602` invalid
params or unknown tool, `-32603` internal. An `AppError` maps to `-32602` for
the 4xx variants and `-32603` for the 5xx ones.

Authentication reuses the `Caller` extractor, so the same bearer token works.
`tools/list` returns only tools the caller's scopes permit.

- [ ] **Step 1: Write the failing test**

Create `tests/mcp.rs` covering:

1. `initialize` returns a `protocolVersion` and a `serverInfo.name` of `acp`.
2. `tools/list` with a `read`-only token contains `task_search` and does **not** contain `task_create`.
3. `tools/list` with a `read,propose` token contains `task_create`.
4. `tools/call` of `task_search` returns matching tasks as JSON text content.
5. `tools/call` of `task_create` with a `propose` token creates **no** task and returns a proposal id in its text content.
6. `tools/call` of `task_create` with a `write` token creates the task.
7. `tools/call` of an unknown tool returns JSON-RPC error `-32602`.
8. A request with no bearer token gets HTTP 401.

- [ ] **Step 2: Run it and confirm failure**

Run: `cargo test --test mcp`
Expected: FAIL — the route 404s.

- [ ] **Step 3: Write the protocol layer**

`protocol.rs` holds the request and response types:

```rust
#[derive(Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}
```

with constructors `JsonRpcResponse::ok(id, result)` and
`JsonRpcResponse::err(id, code, message)`.

- [ ] **Step 4: Write the tool registry**

`tools.rs` defines `ToolDef { name: &'static str, description: &'static str, scope: &'static str, input_schema: serde_json::Value }` and a `const` list. `for_scopes` filters it.

Each tool's `input_schema` is a JSON Schema object naming its parameters, so an
agent can call it without guessing.

- [ ] **Step 5: Write the dispatcher**

`mod.rs` routes `tools/call` by name to the matching controller, converts the
`Outcome` into text content, and maps errors to JSON-RPC codes.

- [ ] **Step 6: Write the tool docs**

One markdown file per tool under `docs/`, following the pattern of
`mycohort-api/api/routes/services/mcp/*.md.ejs`: what the tool does, its
parameters, and one worked example. Serve them through `resources/list` and
`resources/read` with URIs of the form `acp://docs/<tool_name>`.

Embed them at compile time with `include_str!` so the binary stays
self-contained.

- [ ] **Step 7: Run the full suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src tests
git commit -m "feat: add the MCP surface with scope-filtered tools"
```

---

### Task 4: The stdio shim

**Files:**
- Create: `src/bin/acp-mcp.rs`
- Test: `tests/mcp_shim.rs`

**Interfaces:**
- Produces: binary `acp-mcp` that reads newline-delimited JSON-RPC on stdin,
  forwards each request to `POST /api/services/mcp` with the `ACP_TOKEN` bearer,
  and writes the response to stdout.
- Produces: `acp_server::cli::shim::forward(client: &Client, line: &str) -> String`
  so the translation is testable without spawning a process.

This exists because Hermes and Claude Code attach to a local command far more
easily than to an authenticated remote endpoint. The shim holds the token so the
agent never sees it.

- [ ] **Step 1: Write the failing test**

Create `tests/mcp_shim.rs`: a malformed line yields a `-32700` parse error
response rather than a panic or a silent drop, and a well-formed line is passed
through unchanged in its `id`.

- [ ] **Step 2: Run it and confirm failure**

Run: `cargo test --test mcp_shim`

- [ ] **Step 3: Implement `forward` in `src/cli/shim.rs` and the binary**

The loop is: read line, `forward`, print, flush. Flushing every line matters — MCP
clients block waiting for the response.

- [ ] **Step 4: Verify against a running server**

```bash
cargo run --bin acp-server &
export ACP_TOKEN=<a read-scoped token>
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | ./target/debug/acp-mcp
```

Expected: a JSON-RPC response listing the read-scope tools.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "feat: add the acp-mcp stdio shim"
```

---

## What P2 leaves to P3

- `work_claim`, `work_heartbeat`, `work_release`, `run_log_append` and the
  lease mechanics behind them.
- The `job` table worker.
- Streaming run logs.
