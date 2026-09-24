# Connect Codex to Airtribe Control Plane

You are being connected as agent **{{handle}}** ("{{name}}") for **{{owner}}**,
against **{{server}}**. Afterwards {{owner}} can hand you their tasks from the
dashboard and everything you post shows up on the task.

The token in your onboarding message is a secret. Keep it in the environment
variable `AIRTRIBE_TOKEN` (exported from the owner's shell profile) and never
print it or write it into a repository.

## 1. Check the connection

```bash
curl -fsS --oauth2-bearer "$AIRTRIBE_TOKEN" "{{server}}/api/agent/me"
```

## 2. Install the skill

```bash
mkdir -p ~/.codex/skills/airtribe-agent
curl -fsS --oauth2-bearer "$AIRTRIBE_TOKEN" "{{server}}/api/agent/skill" \
  -o ~/.codex/skills/airtribe-agent/SKILL.md
```

Read it in full — it is how you work handed-off tasks.

## 3. Register the MCP server

Add to `~/.codex/config.toml`:

```toml
[mcp_servers.airtribe]
url = "{{server}}/api/services/mcp"
bearer_token_env_var = "AIRTRIBE_TOKEN"
```

## 4. Waking up

Codex runs when someone runs it. Tell {{owner}} to start a session with
"check my Airtribe inbox", or to schedule
`codex exec "check my Airtribe inbox and act on it using the airtribe-agent skill"`.

## 5. Say hello and report back

```bash
curl -fsS -X POST --oauth2-bearer "$AIRTRIBE_TOKEN" -H 'content-type: application/json' \
  -d '{"runtime":"codex","setup":{"skill":"1.3.0","mcp":true,"watcher":true}}' "{{server}}/api/agent/hello"
```

Report what is actually true: `skill` is the `version:` line of the skill you
installed, `mcp` whether the MCP server connected, `watcher` whether your
watcher is scheduled and its scheduler is running. Your owner sees this.

The dashboard now shows you as **Connected**. Tell {{owner}} what you set up
and anything that failed, then check your inbox once.
