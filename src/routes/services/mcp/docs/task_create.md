# task_create

Creates a task inside a phase.

**What happens depends on your scope.** With `write` the task is created and
returned. With only `propose` **nothing is created**: the full intent is
recorded as a pending change and you get back a `changeId` for a human to
approve or reject. Do not assume the task exists after a proposed call.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `phaseId` | uuid | yes | The phase to create the task in. |
| `title` | string | yes | Non-empty. |
| `body` | string | no | Defaults to empty. |
| `priority` | integer | no | 0 (highest) to 4. Defaults to 2. |

## Returns

Applied: `{"status":"applied","entity":{…task…}}`.
Proposed: `{"status":"proposed","changeId":"…"}`.

## Example

```json
{"jsonrpc":"2.0","id":6,"method":"tools/call",
 "params":{"name":"task_create",
           "arguments":{"phaseId":"…","title":"wire the MCP surface","priority":1}}}
```

Required scope: `propose` or `write`.
