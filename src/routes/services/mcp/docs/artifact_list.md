# artifact_list

Lists the artifacts attached to one project, phase, or task — pull requests,
documents, and links. This is how you find the PR that closed a task.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `parentType` | string | yes | `project`, `phase`, or `task`. |
| `parentId` | uuid | yes | Id of that parent. |

## Returns

A JSON array of artifacts: `id`, `parentType`, `parentId`, `kind`, `url`,
`title`, `metadata`, `addedBy`, `createdAt`.

## Example

```json
{"jsonrpc":"2.0","id":5,"method":"tools/call",
 "params":{"name":"artifact_list",
           "arguments":{"parentType":"task","parentId":"7c9e…"}}}
```

Required scope: `read`.
