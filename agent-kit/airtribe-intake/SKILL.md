---
name: airtribe-intake
description: File task-worthy messages from a source (Slack first) into Airtribe Control Plane as Triage tasks for your owner — decide what is worth tracking, categorise it, never file the same thing twice, keep threads together. Load on every intake pass.
version: 1.1.0
author: Airtribe Control Plane
license: MIT
metadata:
  hermes:
    tags: [airtribe, intake, triage, slack]
---

# Intake: turning messages into Triage tasks

You read a source on behalf of your owner ({{owner}}) and file what needs
their attention as tasks in Airtribe Control Plane ({{server}}). Everything you
file lands in **Triage** in their intake project; they accept or dismiss it.
Filing noise costs them attention, so a missed chit-chat message is fine and a
filed one is not. You only ever **read** the source — never post, react, mark
read, or change anything there.

Tools (MCP server `airtribe`; same over HTTP with your token):
`intake_recent` (what you already filed, for dedupe) · `intake_create` (file a
new task) · `intake_append` (add another message to a task you filed).

## One pass

1. Load your state file (see *Where you are up to*). If it is missing this is
   your first pass: look back 24 hours only.
2. Collect new messages since your cursors (see *Slack*), oldest first.
3. `intake_recent` once, to know what is already filed.
4. For each message: skip, append, or file (below). Advance that
   conversation's cursor only after the message is handled.

**Jev parser (use it when your environment has it).** For Hermes it is
`$HERMES_HOME/scripts/jev_intake.py` (needs `TYPESAFE_API_KEY`). Per message,
pipe it one JSON object and follow its decision:

```bash
python3 "$HERMES_HOME/scripts/jev_intake.py" <<'EOF'
{"owner": "<owner name>",
 "source": {"kind": "channel|mention|dm", "channelName": "#…", "intakeChannel": true},
 "messages": [<the thread parent and recent replies for context>, <this message last>],
 "filed": [<intake_recent items: id, title, category, status>]}
EOF
```

It returns `decision`: `create` (file it, use its `category`), `update`
(`intake_append` to `taskId`), `skip`, or `unsure` — only then decide yourself
by the rules below. You still write the title, body and reason, and you still
apply the hard rules that no model overrides: never file the owner's own or a
bot's messages, DMs are `private`. If the script fails (network, missing key),
fall back to your own judgment for this pass and say so in the pass summary.
5. Save the state file. Reply with one line:
   `filed N · appended N · already filed N · skipped N` (and nothing else
   unless something failed).

A `409` from `intake_create`/`intake_append` means that message is already
filed — count it under **already filed**, never under filed or appended, and
don't treat it as an error.

## Skip

- Your owner's own messages. Bots, integrations, joins/leaves, channel topic
  changes. Emoji-only replies, "thanks", "+1", "on it", "done".
- Pure FYI, announcements, greetings, social chat, links without an ask.
- A question that was already answered in the thread, or that your owner
  answered themselves.
- Anything you'd only file at low confidence (< 0.6). Record skips in the
  state file's `skipped` list (key + one-line reason), keep the last 200.

## Is it task-worthy?

File it when someone needs something **done or decided**:
- **bug** — something is broken, wrong, erroring, slow, or data looks off.
- **feature** — a request for new behaviour, a change, a new report/screen.
- **feedback** — a reaction to something shipped that implies a follow-up
  ("the new filter is confusing", "sales can't find X").
- **question** — a question to your owner that needs them to find out or
  decide something (not a quick yes/no they'll answer in Slack).
- **chore** — a concrete ask of your owner: "can you rerun the export",
  "please add Priya to the dashboard".

Where it came from changes the bar:
- **#issues-and-feedback** (and any channel your owner listed as an intake
  channel): every new top-level message is presumed task-worthy unless it is
  clearly chit-chat; default category **feedback** unless it is plainly a bug
  or a feature request.
- **Mentions of your owner** in other channels: only when there is an ask or
  a problem aimed at them.
- **DMs and group DMs** to your owner: only when there is an ask or a problem.
  File them with `private: true` — the team sees only that a DM exists.

## Append, don't duplicate

- A **thread reply** under a message you filed → `intake_append` to that task
  (new details, "still happening", a screenshot link). Skip replies that add
  nothing.
- The **same problem reported again** elsewhere (compare with
  `intake_recent`: same feature/screen, same symptom, within 14 days) →
  `intake_append` to the existing task with the new permalink instead of a
  new task. When unsure whether it is the same issue, file a new one.
- A reply that raises a **different** issue → file it as its own task.

## Writing the task

- `title`: what needs doing, ≤ 80 chars, specific — "Checkout fails for saved
  cards on mobile", not "Bug report" or the first line of the message.
- `body`: 2–5 plain sentences: what was reported, by whom, where, and the
  details that matter (numbers, screens, error text). Plain prose, no
  markdown. Don't copy secrets, passwords, tokens or customer personal data
  from the message — describe them ("a customer's phone number").
- `category`: bug | feature | feedback | question | chore.
- `reason`: one line on why this is worth tracking ("reports checkout errors
  for saved cards; two customers affected").
- `confidence`: 0–1, your honest estimate that the owner wants this tracked.
- `priority` (optional, 0–4, 0 highest): only when the message makes urgency
  clear (outage, customer blocked, a deadline); otherwise leave it out.
- `source`: `kind` "slack"; `key` `<team>:<channel id>:<ts>`; `url` the
  permalink; `channel` the channel id; `channelName` e.g. "#issues-and-feedback"
  or "DM"; `author` the display name; `text` the original message (trimmed to
  2,000 chars); `receivedAt` ISO time of the message; `private` true for DMs
  and group DMs.

## Slack

Use the Composio MCP server's Slack tools (find exact slugs with
`COMPOSIO_SEARCH_TOOLS` if a call fails). Read-only calls only.

- **Intake channels** (from your state file's `channels`): fetch conversation
  history newer than that channel's cursor; for messages with thread replies
  newer than the cursor, fetch the thread replies too.
- **Mentions**: search messages for `<@OWNER_ID>` newer than the mentions
  cursor (search copes with channels you don't list). Skip hits in intake
  channels — they are already covered.
- **DMs**: list conversations of type `im,mpim`; for each whose latest message
  is newer than its cursor, fetch history since the cursor.
- **Permalink**: `https://<workspace>.slack.com/archives/<channel id>/p<ts with
  the dot removed>`; for a thread reply add `?thread_ts=<parent ts>&cid=<channel id>`.
- Slack rate limits are tight for history calls: prefer one history call per
  conversation per pass, and search for mentions. On a rate-limit error, save
  state and stop; the next pass continues from the cursors.

## Where you are up to

Keep a JSON state file next to your profile (for Hermes:
`$HERMES_HOME/slack-intake.json`), created on the first pass:

```json
{
  "workspace": "airtribe",
  "ownerId": "U…",
  "channels": [{"id": "C…", "name": "#issues-and-feedback"}],
  "cursors": {"C…": "1727000000.000100", "mentions": "1727000000.000100", "D…": "…"},
  "skipped": [{"key": "T…:C…:ts", "why": "thanks"}]
}
```

Cursors are Slack `ts` strings; only move them forward. If the file is
unreadable, stop and say so rather than starting over from scratch (which
would re-read a day of messages — harmless thanks to dedupe, but slow).

## Never

- Post, reply, react, or mark anything read in the source.
- File your owner's own messages, or anything from a bot.
- Paste secrets or customer personal data into a task.
- Retry the same failing call in a loop; stop the pass and report it.
