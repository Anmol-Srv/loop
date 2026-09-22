# Task tracks, evidence and notes

Supersedes the single status ladder in
`2026-09-18-airtribe-control-plane-design.md`.

## The problem

One status ladder (`open → in_progress → in_review → done`) was wrong for both
halves of the team. Engineering finishes twice — the work is done, and later it
is in production — and the ladder had one word for both. Design does not ship
at all; it hands over. `in_review` was a state nobody used.

Worse, a status flip was free. A task could be called done with nothing behind
it, so the board recorded an opinion rather than a fact.

## Tracks

A task's track is **its assignee's department**. The department lives on the
person, so a task cannot disagree with the person holding it, and moving
someone between departments moves their work with them.

| Track | Flow |
| --- | --- |
| engineering (`frontend`, `backend`) | `open` → `in_progress` → `completed` → `shipped` |
| design | `open` → `in_progress` → `handoff` → `completed` |

`blocked` and `dropped` are reachable from anywhere on either track.

`completed` appears in both and means different things: mid-flow for
engineering, terminal for design. That is deliberate — it is the honest word in
both places, and the alternative was inventing a synonym for one of them.

An unassigned task has no department, so it has no track and can only sit in
`open`.

### One finish line

`done_at` is stamped when a task reaches **its own track's** terminal state —
`shipped` for engineering, `completed` for design — and cleared on the way out.
Every "is this finished" question in the app reads `done_at`, so the dashboard
counts one thing across two tracks and a reopened task stops claiming a finish
date.

## Evidence

`artifact.kind` covers `pr · commit · figma · doc · link`. Two transitions
require one:

- engineering → `completed` needs a `pr` or a `commit`
- design → `handoff` needs a `figma`

Both accept `manual_reason` instead: a sentence saying why there is nothing to
link. Work really does get done in a console, and the reason is stored on the
task rather than waved through, so the board can still answer "how did this get
done".

Enforced in the controller, not the form, so the CLI and the MCP surface obey
the same rule.

## Who may move what

Whoever holds the task, or an admin — the rule already in place.

`shipped` is the exception: anyone on the team may set it. Shipping records a
fact about production rather than a claim about ownership, and the person who
notices a deploy went out is often not the person who wrote it.

## Notes

`note` (task, author, body, time) is the team talking on a task. Notes gate
nothing and are not evidence, and they need only `read` scope — a note changes
nothing on the board, and an agent that can read a task should be able to say
what it found.

Deliberately not `change` rows: the approval machinery exists for things that
alter the board, and routing a question to a teammate through it would mean
approving a comment before anyone could read it.

## Project properties

`start_date`, `target_date`, `priority` (0–4), and labels.

Labels are a shared vocabulary — a `label` table plus a `project_label` join —
rather than a `text[]` per project. Free text would let "Backend" and "backend"
become two labels nothing could filter on together, and gives a colour nowhere
to live. Creating a label is idempotent on its name, so typing an existing one
in the inline form attaches it instead of failing.

## Known debt

- A task's track can change under it. Move a designer to backend while they
  hold a task in `handoff` and that task is now on the engineering track, in a
  state the track does not have. Nothing crashes — `statuses_for` only gates
  new transitions — but the row reads oddly until someone moves it. A migration
  that rewrites in-flight statuses on a department change would fix it; it has
  not been written because the case has not come up.
- Labels attach to projects only. Tasks will want them.
