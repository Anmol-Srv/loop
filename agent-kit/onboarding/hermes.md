# Connect Hermes to Airtribe Control Plane

You are being connected as agent **{{handle}}** ("{{name}}") for **{{owner}}**,
against the server **{{server}}**. When you finish, {{owner}} can hand you
their tasks from the dashboard, you will be woken when something needs you,
and everything you post shows up on the task.

Do every step in **the Hermes profile you are running in** — `$HERMES_HOME`
(for a named profile that is `~/.hermes/profiles/<name>`). Run `echo
$HERMES_HOME` first and use that path below; if it is empty, you are in the
default profile, `~/.hermes`.

The token you were given is a secret. Store it once, in the profile's `.env`,
and from then on refer to it only as `${AIRTRIBE_TOKEN}`. Never print it, echo
it back in chat, write it into `config.yaml`, a script, a skill or a commit.

## 1. Store the connection

Append to `$HERMES_HOME/.env` (create the file if needed, keep it `chmod 600`),
replacing any existing `AIRTRIBE_URL` / `AIRTRIBE_TOKEN` lines. Leave every
other line alone — other `AIRTRIBE_…` variables there are not yours. Hermes'
file tools refuse to write `.env` by design, so do this step in the terminal
(e.g. `printf '%s\n' … >> "$HERMES_HOME/.env"`):

```
AIRTRIBE_URL={{server}}
AIRTRIBE_TOKEN=<the token from your onboarding message>
```

Check it works — this must print your agent's JSON, not an error:

```bash
set -a; . "$HERMES_HOME/.env"; set +a
curl -fsS --oauth2-bearer "$AIRTRIBE_TOKEN" "$AIRTRIBE_URL/api/agent/me"
```

## 2. Install the skill

The skill teaches you how to work handed-off tasks. Always take it from the
server, so it matches the server you talk to:

```bash
mkdir -p "$HERMES_HOME/skills/airtribe/airtribe-agent"
curl -fsS --oauth2-bearer "$AIRTRIBE_TOKEN" "$AIRTRIBE_URL/api/agent/skill" \
  -o "$HERMES_HOME/skills/airtribe/airtribe-agent/SKILL.md"
```

Then read it in full — it is how you will work from now on.

## 3. Register the MCP server

Add this under `mcp_servers:` in `$HERMES_HOME/config.yaml` (create the key if
it is missing; leave every other server as it is). The name is `airtribe` —
not `acp`, which Hermes already uses for something else:

```yaml
  airtribe:
    url: "{{server}}/api/services/mcp"
    headers:
      Authorization: "Bearer ${AIRTRIBE_TOKEN}"
```

`${AIRTRIBE_TOKEN}` is resolved from `.env` when Hermes connects; the token
itself never goes in the file. Verify:

```bash
hermes mcp test airtribe
```

It must connect and list tools including `agent_inbox` and `task_context`.
The tools become available to you in your **next** session; until then use
the HTTP endpoints in the skill.

## 4. Install the watcher

You should wake only when there is something to do. The server's inbox is a
plain-text summary that stays byte-identical until something changes, so a
Hermes **monitor** job can check it every minute for free and wake you only on
a change.

Create `$HERMES_HOME/scripts/airtribe-inbox.sh`:

```bash
#!/usr/bin/env bash
# Airtribe inbox for the monitor job: identical output until something changes.
set -a; . "${HERMES_HOME:-$HOME/.hermes}/.env"; set +a
curl -fsS --max-time 20 --oauth2-bearer "$AIRTRIBE_TOKEN" \
  "$AIRTRIBE_URL/api/agent/inbox"
```

`chmod +x` it, run it once (it should print your inbox, probably empty), then
schedule it:

```bash
hermes cron create "every 1m" \
  "Your Airtribe inbox changed. Load the airtribe-agent skill and handle the inbox now, following the skill." \
  --name airtribe-inbox \
  --monitor-script airtribe-inbox.sh \
  --skill airtribe-agent \
  --deliver local
```

Cron jobs only run while the Hermes gateway is running for this profile.
Check with `hermes cron status`. If the scheduler is not running, **tell
{{owner}}** that the watcher needs the gateway and ask before starting it —
do not install a background service without their go-ahead.

## 5. Say hello

```bash
curl -fsS -X POST --oauth2-bearer "$AIRTRIBE_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"runtime":"hermes","setup":{"skill":"1.3.0","mcp":true,"watcher":true}}' "$AIRTRIBE_URL/api/agent/hello"
```

Report what is actually true: `skill` is the `version:` line of the skill you
installed, `mcp` whether the MCP server connected, `watcher` whether your
watcher is scheduled and its scheduler is running. Your owner sees this.

The dashboard now shows you as **Connected**.

## 6. Report back

Tell {{owner}}, briefly: connected as {{handle}}, skill installed, MCP server
`airtribe` registered (and whether `hermes mcp test` passed), watcher
scheduled (and whether the scheduler is running), and anything that failed.
Then read your inbox once and act on it following the skill.

## Disconnecting

If {{owner}} asks you to disconnect: remove the `airtribe-inbox` cron job, the
`airtribe` MCP server, the `airtribe` skill directory, the inbox script, and the
`AIRTRIBE_URL` and `AIRTRIBE_TOKEN` lines in `.env` — exactly those two;
other `AIRTRIBE_…` variables belong to other tools. Revoking the agent in the dashboard cuts off
the token either way.
