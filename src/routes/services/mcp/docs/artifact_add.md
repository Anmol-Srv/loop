# artifact_add

Attaches an artifact — a pull request, a document, or a plain link — to a
project, phase, or task. This is how an agent reports the output of its work.

With only the `propose` scope nothing is attached; a pending change is recorded.

## Parameters

| Name | Type | Required | Notes |
|---|---|---|---|
| `parentType` | string | yes | `project`, `phase`, or `task`. |
| `parentId` | uuid | yes | |
| `kind` | string | yes | `pr`, `doc`, or `link`. |
| `url` | string | yes | Non-empty. |
| `title` | string | no | Defaults to empty. |

## Returns

`{"status":"applied","entity":{…artifact…}}` or `{"status":"proposed","changeId":"…"}`.

## Example

```json
{"jsonrpc":"2.0","id":8,"method":"tools/call",
 "params":{"name":"artifact_add",
           "arguments":{"parentType":"task","parentId":"7c9e…","kind":"pr",
                        "url":"https://github.com/airtribe/acp/pull/12",
                        "title":"MCP surface"}}}
```

Required scope: `propose` or `write`.
