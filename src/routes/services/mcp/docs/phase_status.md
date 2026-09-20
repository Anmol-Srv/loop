# phase_status

Returns a project's phases in position order, each with its status and whether
it is a gate. Use it to answer "where is this project" without pulling tasks.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `projectId` | uuid | yes | From `project_list`. |

## Returns

A JSON array of phases: `id`, `projectId`, `position`, `name`, `status`
(`planned`, `active`, `blocked`, `done`), `gate`, `createdAt`, `updatedAt`.

## Example

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call",
 "params":{"name":"phase_status",
           "arguments":{"projectId":"3f1c8b2e-0000-4000-8000-000000000001"}}}
```

Required scope: `read`.
