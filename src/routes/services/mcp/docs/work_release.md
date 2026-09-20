# work_release

Drops your lease and returns the task to `open` so another worker can take it.

Release when you finish and when you give up. Either way it hands the task on
minutes sooner than letting the lease lapse. Releasing does **not** mark the
task done — that is a separate `task_update` proposal a human approves.

## Parameters

| Name | Type | Required |
|---|---|---|
| `taskId` | uuid | yes |

## Returns

The task, with `claimed_by` cleared and status back to `open`.

Errors with 403 if you do not hold the lease.

Required scope: `claim`.
