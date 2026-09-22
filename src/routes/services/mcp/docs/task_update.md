# task_update

Changes a task's status, or reassigns it. Give **either** `status` **or** one of
`personEmail` / `agentLabel` in a single call — not both at once.

With only the `propose` scope the task is left untouched and a pending change is
recorded instead; the task must already exist, so a bad `taskId` still errors.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `taskId` | uuid | yes | |
| `status` | string | no | Depends on the track (below). |
| `manualReason` | string | no | Why the work is finished with nothing to point at. |

A task's track is its assignee's department:

| Track | Flow |
| --- | --- |
| engineering (`frontend`, `backend`) | `open` → `in_progress` → `completed` → `shipped` |
| design | `open` → `in_progress` → `handoff` → `completed` |

`blocked` and `dropped` are reachable from anywhere on either track. Asking for
a status the task's track does not have is a `BAD_REQUEST` that names the legal
set.

Two transitions want evidence first: engineering `completed` needs a `pr` or
`commit` artifact, and design `handoff` needs a `figma` one. Either will accept
`manualReason` instead — the work is recorded as done without a link rather
than waved through.
| `personEmail` | string | no | Assign to a person by email. |
| `agentLabel` | string | no | Assign to the newest active token with this label. |

Passing neither `status` nor an assignee unassigns the task.

## Returns

`{"status":"applied","entity":{…task…}}` or `{"status":"proposed","changeId":"…"}`.

## Example — mark a task completed

```json
{"jsonrpc":"2.0","id":7,"method":"tools/call",
 "params":{"name":"task_update",
           "arguments":{"taskId":"7c9e…","status":"completed"}}}
```

Required scope: `propose` or `write`.
