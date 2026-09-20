# project_list

Lists every project in the control plane. A project is the top-level container;
phases hang off it, and tasks hang off phases.

Start here when you do not yet know a `projectId`.

## Parameters

None.

## Returns

A JSON array of projects: `id`, `key`, `name`, `createdAt`, `updatedAt`.

## Example

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call",
 "params":{"name":"project_list","arguments":{}}}
```

```json
{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text",
 "text":"[{\"id\":\"3f1c…\",\"key\":\"acp\",\"name\":\"Control Plane\"}]"}]}}
```

Required scope: `read`.
