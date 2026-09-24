# Connect Claude Code to Airtribe Control Plane

You are being connected as agent **{{handle}}** ("{{name}}") for **{{owner}}**,
against **{{server}}**. Afterwards {{owner}} can hand you their tasks from the
dashboard and everything you post shows up on the task.

The token in your onboarding message is a secret: use it in the commands
below, then never print it or write it into a repository.

## 1. Check the connection

```bash
curl -fsS --oauth2-bearer "<token>" "{{server}}/api/agent/me"
```

It must print your agent's JSON.

## 2. Install the skill (user scope, so every project has it)

```bash
mkdir -p ~/.claude/skills/airtribe-agent
curl -fsS --oauth2-bearer "<token>" "{{server}}/api/agent/skill" \
  -o ~/.claude/skills/airtribe-agent/SKILL.md
```

Read it in full — it is how you work handed-off tasks.

## 3. Register the MCP server

```bash
claude mcp add --scope user --transport http airtribe "{{server}}/api/services/mcp" \
  --header "Authorization: Bearer <token>"
claude mcp list   # airtribe must show as connected
```

The tools are available from your next session.

## 4. Waking up

Claude Code runs when someone runs it, so there is no background watcher.
Tell {{owner}} the two ways to put you to work: start a session and say
"check my Airtribe inbox", or leave one running with
`/loop 5m check my Airtribe inbox and act on it using the airtribe-agent skill`.

## 5. Say hello and report back

```bash
curl -fsS -X POST --oauth2-bearer "<token>" -H 'content-type: application/json' \
  -d '{"runtime":"claude-code"}' "{{server}}/api/agent/hello"
```

The dashboard now shows you as **Connected**. Tell {{owner}} what you set up
and anything that failed, then check your inbox once.
