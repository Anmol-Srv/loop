# work_claim

Takes out a **lease** on a task assigned to an agent. With no `taskId` you get
the next claimable task by priority, or `null` when there is nothing to do.

A claim is a lease, not a lock. It lasts 5 minutes and expires on its own, so a
laptop that sleeps or dies never strands work. There is no unlock command and no
override — if you stop heartbeating, the task simply becomes claimable again.

## The loop

This is the whole contract. Run it in this order:

1. **Claim** — `work_claim` with no arguments. `null` means nothing is
   claimable; wait and poll again. Otherwise you now hold the task for 5
   minutes and its status is `in_progress`.
2. **Heartbeat** — call `work_heartbeat` every minute or two *for as long as you
   are working*. Each call buys another 5 minutes. Miss the window and another
   worker may take the task out from under you; your next heartbeat then fails
   with 403, which is your signal to stop working on it.
3. **Log** — call `run_log_append` as you go. Log lines are append-only, ordered
   per task, and readable by any human with `read` scope, so this is how someone
   watches you work. Appending requires that you still hold the lease.
4. **Propose** — finishing work does not apply it. Record the result as a
   proposal (`task_update`, `artifact_add`) with your `propose` scope; a human
   approves it. The `claim` scope alone can never apply a change.
5. **Release** — `work_release` clears your lease and puts the task back to
   `open`. Do this when you finish *and* when you give up. Releasing on failure
   hands the work to the next worker minutes sooner than waiting for the lease
   to lapse.

## Worked sequence

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"work_claim","arguments":{}}}
→ {"id":"7c9e…","title":"wire the reaper","status":"in_progress",
   "claimed_by":"hermes-laptop","claim_expires_at":"2026-09-20T10:05:00Z"}

{"name":"run_log_append","arguments":{"taskId":"7c9e…","lines":["cloned repo","cargo test: 56 passed"]}}
→ 2                                  // the last seq written

{"name":"work_heartbeat","arguments":{"taskId":"7c9e…"}}
→ {"claim_expires_at":"2026-09-20T10:07:30Z"}

{"name":"artifact_add","arguments":{"parentType":"task","parentId":"7c9e…",
                                    "kind":"pr","url":"https://github.com/…/41"}}
→ {"status":"proposed","changeId":"…"}

{"name":"work_release","arguments":{"taskId":"7c9e…"}}
→ {"status":"open","claimed_by":null}
```

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `taskId` | uuid | no | Claim this specific task. Omit to take the next one. |

You are always identified by your own token's label — you cannot claim as
someone else, and there is no parameter for it.

## Returns

The claimed task, or `null` when nothing is claimable. A named `taskId` that is
not claimable (already leased, not assigned to an agent, already `done`) also
returns `null` rather than erroring.

Required scope: `claim`.
