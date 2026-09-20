# run_log_append

Appends lines to a task's run log — the stream a human watches while you work.

Lines are append-only and ordered per task by a server-allocated `seq`; you
cannot supply, edit, or delete one. Anyone with `read` scope can follow along
via `GET /api/user/tasks/{id}/logs?afterSeq=N`.

You must currently hold the task's lease. If your lease lapsed, appending fails
with 403 — the same signal `work_heartbeat` gives you.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `taskId` | uuid | yes | A task you currently hold. |
| `lines` | string[] | yes | One or more lines. Empty array is rejected. |

Batch a handful of lines per call rather than one call per line.

## Returns

The last `seq` written, as a number.

## Example

```json
{"jsonrpc":"2.0","id":4,"method":"tools/call",
 "params":{"name":"run_log_append",
           "arguments":{"taskId":"7c9e…","lines":["cargo build: clean","opened PR #41"]}}}
```

Required scope: `claim`.
