# task_search

Searches tasks. Every filter is optional and they combine with AND; calling it
with no arguments returns every task, ordered by priority then age.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `projectId` | uuid | no | All tasks in the project, across phases. |
| `phaseId` | uuid | no | Tasks in one phase. |
| `status` | string | no | `open`, `in_progress`, `in_review`, `blocked`, `done`, `dropped`. |
| `assigneeEmail` | string | no | Email of the assigned person. |
| `assigneeKind` | string | no | `human` or `agent`. |

## Returns

A JSON array of tasks.

## Example — what is open in the build phase

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call",
 "params":{"name":"task_search",
           "arguments":{"phaseId":"…","status":"open"}}}
```

Required scope: `read`.
