# task_get

Fetches one task by id, including its assignee and claim fields. Use it after
`task_search` when you need the full row for a single task.

## Parameters

| Name | Type | Required |
|---|---|---|
| `taskId` | uuid | yes |

## Returns

One task object. An unknown id is an error, not an empty result.

## Example

```json
{"jsonrpc":"2.0","id":4,"method":"tools/call",
 "params":{"name":"task_get",
           "arguments":{"taskId":"7c9e…"}}}
```

Required scope: `read`.
