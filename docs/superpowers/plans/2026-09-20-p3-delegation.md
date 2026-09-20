# Airtribe Control Plane — P3 Delegation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a laptop running Hermes pull work from the control plane, report progress while it works, and hand its result back for approval — without the server ever reaching into that laptop.

**Architecture:** Assigning a task to an agent makes it *claimable*. A worker claims it under a time-bounded lease, heartbeats to extend the lease, streams run-log lines, and finishes by proposing a change. A lease that stops being renewed lapses on its own, so a laptop that sleeps or dies never strands work.

**Tech Stack:** Rust 1.96, Axum 0.8, sqlx 0.8, tokio, Postgres `LISTEN`/`NOTIFY`.

**Spec:** `docs/superpowers/specs/2026-09-18-airtribe-control-plane-design.md`

**Builds on:** P0, P1, and P2 (the MCP surface and the approval queue).

**Plan density:** as with P2, complete code appears for new logic (the claim
query, the reaper, the notify listener). Work that repeats an established shape
names its interface and the file to copy from.

## Global Constraints

- All prior constraints hold. Postgres on port **5433**.
- **A claim is a lease, never a lock.** There is no unlock command and no
  administrative override. Anything that can strand a task indefinitely is a
  defect.
- Claiming must be atomic under concurrency. Two workers polling at the same
  instant must not both receive the same task — enforce this in SQL with
  `FOR UPDATE SKIP LOCKED`, not in application code.
- Default lease is 5 minutes; a heartbeat extends it by another 5.
- `claim`-scoped tools may claim, heartbeat, release, and append run logs. They
  may not apply changes. Finishing work still produces a *proposal*.
- Run-log lines are append-only and ordered per task by `seq`.

---

### Task 1: Claim, heartbeat, release

**Files:**
- Create: `migrations/20260920000002_claim_indexes.sql`
- Create: `src/controllers/work.rs`
- Create: `src/routes/user/work.rs`
- Modify: `src/controllers/mod.rs`, `src/routes/user/mod.rs`, `src/app.rs`
- Test: `tests/work_claim.rs`

**Interfaces:**
- Produces:
  - `acp_server::controllers::work::claimable(state, limit: i64) -> AppResult<Vec<Task>>`
  - `acp_server::controllers::work::claim(state, caller_label: &str, task_id: Option<Uuid>) -> AppResult<Option<Task>>` — claims the named task, or the next available one when `None`.
  - `acp_server::controllers::work::heartbeat(state, caller_label: &str, task_id: Uuid) -> AppResult<Task>`
  - `acp_server::controllers::work::release(state, caller_label: &str, task_id: Uuid) -> AppResult<Task>`
  - Routes `GET /api/user/work/claimable`, `POST /api/user/work/claim`, `POST /api/user/work/{id}/heartbeat`, `POST /api/user/work/{id}/release`

A task is claimable when `assignee_kind = 'agent'`, its status is `open` or
`in_progress`, and it is not currently leased:

```sql
SELECT ... FROM task
WHERE assignee_kind = 'agent'
  AND status IN ('open', 'in_progress')
  AND (claimed_by IS NULL OR claim_expires_at < now())
ORDER BY priority, created_at
FOR UPDATE SKIP LOCKED
LIMIT 1
```

The claim itself sets `claimed_by`, `claim_expires_at = now() + interval '5
minutes'`, and moves status to `in_progress`, in the same transaction as the
`SELECT ... FOR UPDATE`.

`heartbeat` and `release` must verify `claimed_by` matches the caller. A worker
heartbeating a task it does not hold gets 403 — otherwise a stale worker can
keep a task alive after another has taken it.

Claims are operational state, not editorial state, so they do **not** write
`change` rows. Note this in a comment; it is the one deliberate exception to
"every mutation writes a change", and a reader will otherwise assume it is a
bug.

- [ ] **Step 1: Write the failing test**

Create `tests/work_claim.rs` covering:

1. A task assigned to an agent appears in `claimable`; a task assigned to a human does not.
2. Claiming sets `claimed_by`, sets `claim_expires_at` in the future, and moves status to `in_progress`.
3. A second worker claiming immediately afterwards gets `None` — the task is leased.
4. After the lease is expired (set `claim_expires_at` to the past directly), a second worker can claim it.
5. `heartbeat` by the holder extends `claim_expires_at`; `heartbeat` by anyone else returns 403.
6. `release` by the holder clears `claimed_by` and returns status to `open`.
7. Ten concurrent claims against three claimable tasks hand out each task exactly once. Use `tokio::join!` over futures on the same pool and assert the set of returned ids has no duplicates.

Test 7 is the one that matters. Write it first.

- [ ] **Step 2: Run it and confirm failure**

Run: `cargo test --test work_claim`

- [ ] **Step 3: Write the migration**

```sql
CREATE INDEX task_lease_idx ON task(claim_expires_at) WHERE claimed_by IS NOT NULL;
```

The `task_claimable_idx` partial index from P0 already covers the claim lookup.

- [ ] **Step 4: Write the controller, routes, and wiring**

Follow `src/controllers/task.rs` and `src/routes/user/task.rs`. Every route
requires `caller.require("claim")?`.

`claim` derives `caller_label` from `caller.actor.label`, so a worker cannot
claim as someone else.

- [ ] **Step 5: Run the full suite and commit**

```bash
cargo test
git add migrations src tests
git commit -m "feat: add claim-lease work distribution"
```

---

### Task 2: Run logs

**Files:**
- Create: `src/controllers/run_log.rs`
- Create: `src/routes/user/run_log.rs`
- Modify: `src/controllers/mod.rs`, `src/routes/user/mod.rs`, `src/app.rs`
- Test: `tests/run_log.rs`

**Interfaces:**
- Produces:
  - `acp_server::controllers::run_log::append(state, caller_label: &str, task_id: Uuid, lines: Vec<String>) -> AppResult<i64>` — returns the last `seq` written.
  - `acp_server::controllers::run_log::read(state, task_id: Uuid, after_seq: i64) -> AppResult<Vec<RunLogLine>>`
  - Routes `POST /api/user/tasks/{id}/logs`, `GET /api/user/tasks/{id}/logs?afterSeq=N`

`seq` is allocated server-side, not supplied by the caller, so concurrent
appends cannot collide:

```sql
INSERT INTO run_log_line (task_id, seq, text)
SELECT $1, COALESCE(MAX(seq), 0) + row_number() OVER (), line
FROM run_log_line, unnest($2::text[]) AS line
WHERE task_id = $1
```

Verify this against the `UNIQUE (task_id, seq)` constraint from P0 under
concurrent writers; if the window form races, fall back to a per-task advisory
lock (`pg_advisory_xact_lock(hashtext(task_id::text))`), which is simpler to
reason about than a retry loop.

Appending requires the `claim` scope and that the caller currently holds the
task's lease. Reading requires only `read`, so a human can watch an agent work.

- [ ] **Step 1: Write the failing test**

Cover: appended lines come back in order; `afterSeq` pages correctly; two
concurrent appends of five lines each produce ten lines with ten distinct `seq`
values; a caller not holding the lease gets 403.

- [ ] **Step 2–4: Implement, test, commit**

```bash
git commit -m "feat: add append-only run logs per task"
```

---

### Task 3: The claim-scope MCP tools

**Files:**
- Modify: `src/routes/services/mcp/tools.rs`, `src/routes/services/mcp/mod.rs`
- Create: `src/routes/services/mcp/docs/work_claim.md`, `work_heartbeat.md`, `work_release.md`, `run_log_append.md`
- Test: `tests/mcp_claim.rs`

**Interfaces:**
- Adds to the P2 registry, under scope `claim`: `work_claim`, `work_heartbeat`, `work_release`, `run_log_append`.

Nothing structural is new here — P2's registry already filters by scope. This
task adds four entries, four dispatcher arms, and four documents.

The `work_claim` document is the important one: it should describe the whole
loop an agent is expected to run (claim, heartbeat while working, append logs,
propose the result, release), because that loop is the only thing a fresh agent
cannot infer from the tool list alone.

- [ ] **Step 1: Write the failing test**

Cover: a `claim`-scoped token sees the four tools and does not see
`task_create`; `work_claim` through MCP leases a task; a `read`-only token does
not see them at all.

- [ ] **Step 2–4: Implement, test, commit**

```bash
git commit -m "feat: expose claim-scope work tools over MCP"
```

---

### Task 4: The lease reaper and job worker

**Files:**
- Create: `src/jobs/mod.rs`, `src/jobs/worker.rs`, `src/jobs/reaper.rs`
- Modify: `src/lib.rs`, `src/main.rs`
- Test: `tests/jobs.rs`

**Interfaces:**
- Produces:
  - `acp_server::jobs::enqueue(db: &PgPool, kind: &str, payload: Value, run_after: Option<DateTime<Utc>>) -> AppResult<Uuid>`
  - `acp_server::jobs::worker::run(state: AppState, shutdown: tokio::sync::watch::Receiver<bool>)` — the loop, spawned from `main`.
  - `acp_server::jobs::reaper::sweep(state: &AppState) -> AppResult<u64>` — returns tasks freed.

The worker claims jobs with the same `FOR UPDATE SKIP LOCKED` shape as Task 1,
so two server instances can run safely. It waits on `LISTEN acp_jobs` and wakes
on either a notification or a 30-second timeout, whichever comes first — the
timeout is what makes `run_after` scheduling work without polling tightly.

`enqueue` issues `NOTIFY acp_jobs` after insert, inside the same transaction, so
a job is never announced before it is visible.

The reaper is registered as a recurring job kind, `lease_sweep`, that re-enqueues
itself for 60 seconds out. It clears `claimed_by` and `claim_expires_at` on any
task whose lease has lapsed and returns its status to `open`.

Nothing else uses the job queue yet. It exists here because the reaper needs it
and because P4's notifications will.

- [ ] **Step 1: Write the failing test**

Cover: `enqueue` then `sweep` frees a task whose lease lapsed and leaves a live
lease alone; the worker picks up an enqueued job and marks it done; two workers
against one job run it exactly once; `run_after` in the future is not picked up
early.

Test the worker loop by calling its inner step function directly rather than
spawning the task and sleeping. A test that sleeps is a test that flakes.

- [ ] **Step 2–4: Implement, test, commit**

```bash
git commit -m "feat: add job worker and lease reaper"
```

---

### Task 5: End-to-end delegation rehearsal

**Files:**
- Create: `scripts/rehearse-delegation.sh`

Not a unit test — a script that drives a running server the way Hermes will, so
the loop is proven against real HTTP rather than in-process calls.

The script must:

1. Mint a `read,write` human token and a `read,claim,propose` agent token labelled `hermes`.
2. Create a project, phase, and task; assign the task to `hermes`.
3. As the agent: claim the task, heartbeat, append three run-log lines, propose moving it to `in_review`.
4. Assert the task is NOT yet `in_review` — the proposal is pending.
5. As the human: list pending, approve the change.
6. Assert the task is now `in_review` and the run log reads back in order.
7. Print a compact summary of each step and exit non-zero on the first failure.

- [ ] **Step 1: Write the script and run it against a live server**
- [ ] **Step 2: Commit**

```bash
git commit -m "test: add end-to-end delegation rehearsal script"
```

---

## What P3 leaves to P4

- The web UI, including the approval inbox and the live run-log view.
- Any notification transport (Slack, email); the `job` queue is the seam it
  will attach to.
