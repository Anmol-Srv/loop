# task_update

Changes a task's status, or reassigns it. Give **either** `status` **or** one of
`personEmail` / `agentLabel` in a single call — not both at once.

With only the `propose` scope the task is left untouched and a pending change is
recorded instead; the task must already exist, so a bad `taskId` still errors.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `taskId` | uuid | yes | |
| `status` | string | no | `open`, `in_progress`, `in_review`, `blocked`, `done`, `dropped`. |
| `personEmail` | string | no | Assign to a person by email. |
| `agentLabel` | string | no | Assign to the newest active token with this label. |

Passing neither `status` nor an assignee unassigns the task.

## Returns

`{"status":"applied","entity":{…task…}}` or `{"status":"proposed","changeId":"…"}`.

## Example — mark a task done

```json
{"jsonrpc":"2.0","id":7,"method":"tools/call",
 "params":{"name":"task_update",
           "arguments":{"taskId":"7c9e…","status":"done"}}}
```

Required scope: `propose` or `write`.
