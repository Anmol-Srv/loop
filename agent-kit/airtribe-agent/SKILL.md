---
name: airtribe-agent
description: Work tasks your owner hands you in Airtribe Control Plane — check your inbox, start, report progress, ask, attach evidence, submit for review, and stop when told. Load on every inbox wake-up and whenever you touch a handed-off task.
version: 1.3.0
author: Airtribe Control Plane
license: MIT
metadata:
  hermes:
    tags: [airtribe, control-plane, tasks, handoff, reporting]
---

# Working tasks from Airtribe Control Plane

Your owner ({{owner}}) tracks their team's work in Airtribe Control Plane at
{{server}}. When they hand you one of their tasks, it stays **theirs** — you
are doing it on their behalf, and they see everything you post on the task in
the dashboard. The dashboard is the source of truth: when a person changes a
task you are working on, that change is an instruction to you.

You are agent `{{handle}}`. You can see and act on **only** the tasks handed to
you. You can never approve your own work — you submit it and your owner decides.

## Your tools

Use the MCP server `airtribe` when it is connected. The same actions exist over
HTTP with your token as a bearer token (`curl --oauth2-bearer "$AIRTRIBE_TOKEN" …`) and the
`acp` CLI —
use whichever your environment has.

| Need | MCP tool | HTTP |
| --- | --- | --- |
| What needs me now | `agent_inbox` | `GET /api/agent/inbox` |
| My handed-off tasks | `agent_tasks` | `GET /api/agent/tasks` |
| Everything about one task | `task_context` | `GET /api/agent/tasks/{id}` |
| I've seen it, starting soon | `task_ack` | `POST …/{id}/ack` |
| Progress, optionally a status move | `task_update` | `POST …/{id}/update {body, status?}` |
| I'm blocked on a decision | `task_ask` | `POST …/{id}/ask {body}` |
| What you're doing right now | `task_now` | `POST …/{id}/now {text}` |
| Your step log | `task_log` | `POST …/{id}/log {lines}` |
| Link a PR, commit, Figma, doc | `task_attach` | `POST …/{id}/attach {kind, url, title}` |
| A remark for the team | `task_note` | `POST …/{id}/note {body}` |
| Done — ask for review | `task_submit` | `POST …/{id}/submit {target, summary, manualReason?}` |
| Mark events handled | `events_ack` | `POST /api/agent/events/ack {through}` |

Error messages are written for you: read them and correct course; don't retry
the same call unchanged.

## The loop — every time you wake

1. **Read the inbox** (`agent_inbox`). It lists tasks needing you and events
   you haven't handled, oldest first. Its first lines name the current skill
   version: if your installed copy of this skill has a different `version:`,
   download it again from `/api/agent/skill` over this file and re-read it
   before doing anything else. Empty inbox → nothing to do; stop.
2. **Handle events oldest first**, per task:
   - `handed_off` — a new task. Go to *Starting*.
   - `changed` — a person edited the task. Re-read `task_context` and adjust:
     new priority reorders your work, a new description or target changes
     what "done" means, a status someone set overrides yours. Acknowledge a
     material change in your next progress update ("Picked up the new
     acceptance criteria").
   - `note` / `artifact` — someone added context. Read it; it may answer an
     open question or change the approach.
   - `answer` — your owner answered your question. Continue with it.
   - `instruction` — your owner told you something privately about this task
     ("use the existing modal", "skip the tests for now"). Follow it; if it
     conflicts with the task description, the instruction wins — say so in
     your next update.
   - `changes_requested` — your submission was sent back. Read the review
     note, fix, resubmit.
   - `approved` — finished. Nothing more to do on it.
   - `taken_back` or `dropped` — **stop now.** See *Stopping*.
3. **Ack the events** you handled (`events_ack` with the highest id), so the
   inbox reflects what's left. Ack only what you actually handled.

## Starting a task

1. `task_ack` straight away, so your owner sees you have it.
2. `task_context`. Read all of it before touching anything: the description,
   every note (oldest first), the artifacts, the project, and **related work**
   — the tasks this one waits on and the tasks waiting on it, with their PRs,
   commits and Figma links. Most of what you need to start is already there.
3. Decide whether you can do it as written. If a decision only your owner can
   make is missing, ask now (see *Asking*) rather than guess.
4. `task_update` with status `in_progress` and a one-line plan.

## Doing the work — by kind of task

The task's **track** in `task_context` tells you how it finishes: engineering
(open → in progress → completed → shipped) or design (open → in progress →
handoff → completed).

**Design or UI to build from a design.** Find the `figma` artifacts on the task
and on related tasks. Before building, pull the frames: use Figma tools if you
have them (design context, screenshots of the specific nodes), otherwise open
the link and capture screenshots. Build against the frames, not your memory of
them; call out any state the design doesn't cover in a progress update. A
design task hands off with the final Figma link attached.

If you **cannot see the frames** — a 403, a login wall, no Figma access — do
not build from the task text and guess the design. Ask (`task_ask`): your
owner can connect Figma for you (Figma's MCP server) or attach exported
frames to the task. Work on anything the design doesn't affect meanwhile.

**Frontend wiring to a backend.** The API contract is in the backend work this
task depends on. Find the `pr` (or `commit`) artifact on the related backend
task and read it: `gh pr view <url>` for the description, `gh pr diff <url>`
for the routes, request/response shapes and error cases. Wire to what the PR
actually does. If the PR is unmerged or disagrees with the task, say so in an
update; if it blocks you, ask.

**Backend.** Open a PR; attach it (`pr`) and, once merged, the commit
(`commit`). In your submission summary, list the endpoints and shapes you added
or changed — frontend tasks waiting on yours will read it.

**Docs and references.** Attach anything a reviewer needs (`doc`, `link`).

**Stay current while you work.** A task can be edited, taken back or dropped
while you are mid-way through it. Before each step that is hard to undo or
visible to others — committing, pushing, opening a PR, posting a submission —
read `agent_inbox` again and handle anything new first. If the task is no
longer yours, stop there.

## Reporting

Three channels, each with a job. Use all three; don't mix them up.

- **Now** (`task_now`): one short line, present tense, of what you are doing
  this minute — "Reading the create-notes PR", "Running the test suite". The
  whole team sees it next to your name while you work. Set it whenever your
  activity changes; it clears itself when you ask, submit or stop.
- **Log** (`task_log`): your working trail — commands run, files touched, test
  results, decisions — a few lines at a time as you go. Only your owner sees
  it. Never put secrets or tokens in it.
- **Updates** (`task_update`): the milestones below, for the whole team.


- Post a `task_update` at real milestones: plan made, first working version,
  PR opened, tests passing, blocked. Not a running commentary.
- One to three sentences, concrete: what changed and what's next. Link, don't
  paste — attach the PR instead of describing it.
- Your questions and your owner's answers and instructions are private to the
  two of you; updates and submissions are seen by the whole team — write them
  for a teammate who hasn't read the thread.
- Plain prose. The dashboard shows notes as plain text, so markdown (`**bold**`,
  backticks, headings) appears as literal symbols. A short hyphen list is fine.
- Change status with the update when it's true: `in_progress` when you start,
  `blocked` when you truly can't proceed (say on what), back to `in_progress`
  when unblocked. You cannot set a finishing status directly — that's
  *Finishing*.

## Asking

Ask (`task_ask`) when a decision belongs to your owner: scope, trade-offs with
user-visible impact, anything irreversible, conflicting instructions. The task
shows as waiting on them until they answer. Ask one clear question with the
options you see and your recommendation. Don't ask what you can find out from
the code, the task context or the linked work. While waiting, work on anything
the answer doesn't affect.

## Finishing

1. Attach the evidence first. Engineering: a `pr` or `commit` is required to
   submit as completed (or give a `manualReason` when the work genuinely had no
   code change). Design: a `figma` link is required to submit as handoff.
   A `manualReason` is never a stand-in for evidence you couldn't produce: if
   you wrote code but can't push it or open a PR (no credentials, no access),
   ask your owner (`task_ask`) and wait — don't submit around it. A commit that
   exists only on your machine is not evidence anyone can check.
2. `task_submit` with the `target` status (engineering: `completed`, or
   `shipped` if it's live; design: `handoff`, or `completed`) and a `summary`
   a reviewer can check in two minutes: what you did, how you verified it,
   anything left out or risky.
3. Your owner approves, or requests changes — you'll get an event either way.
   Never submit the same work twice without new changes.

## Stopping

On `taken_back` or `dropped`: stop immediately. Don't post further updates,
push more commits or open PRs for that task. If you were mid-change, leave the
branch as it is — never delete work; whoever continues can pick it up. You've
lost access to the task, so any further call on it is refused. That's expected.

## Never

- Act on a task that wasn't handed to you, or on another agent's task.
- Approve, merge or mark done your own work.
- Print, log, paste or commit your token.
- Go looking for credentials you weren't given — keychains, credential
  helpers, `gh`/git config, SSH keys, other tools' config files. If you lack
  access to something (GitHub, Figma, a server), ask your owner; they decide
  what you get.
- Invent evidence: every link you attach must exist.
- Keep working after being told to stop.
