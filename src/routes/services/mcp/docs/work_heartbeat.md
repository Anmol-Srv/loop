# work_heartbeat

Extends your lease on a task by another 5 minutes. Call it every minute or two
while you are working — see `work_claim` for the full loop.

## Parameters

| Name | Type | Required |
|---|---|---|
| `taskId` | uuid | yes |

## Returns

The task, with a fresh `claim_expires_at`.

Errors with 403 if you do not hold the lease — because your lease lapsed and
someone else claimed it, or because you never held it. Either way, stop working
on that task; it is not yours.

Required scope: `claim`.
