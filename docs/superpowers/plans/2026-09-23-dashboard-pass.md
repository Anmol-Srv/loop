# Dashboard pass — shared brief

Every agent on this pass reads this whole file, then its own section.

## What "done" means

A complete dashboard for a small engineering team, in the spirit of Linear
but in-house: every task, project, document and PR can be **added, viewed and
updated** from the app; the numbers are right; every page works at 1440, 1100
and 820 points wide. Agent handoffs are the *next* phase — do not build for
them, and hide agent-only UI when no agent is involved.

## The app

Rust, egui 0.36, a native macOS app (no webview, no HTML). Design system in
`src/desktop/design/` — read all of it before writing a line:
`tokens.rs` (colour/space/size/text/radius), `widgets.rs` (buttons `w::primary`
`w::secondary` `w::ghost` `w::link`, `w::field`, `w::field_multiline`,
`w::caption`, `w::empty`, `w::error`, `w::progress`), `cards.rs` (`c::chip`,
`c::Tone`, `c::status_tone`, `c::surface`), `viz.rs` (`viz::select`,
`viz::multi_select`, `viz::filter`, `viz::search`, `viz::toolbar`, `viz::clear`),
`table.rs` (`table::show`, `Col::left/right/fill(...).rank(n)`, `table::Cells`
with `row.at/strong/text/muted` by column index), `shell.rs` (`page_title`,
`back`, `section_count_with`, `with_rail`, `property`, `divider`), `avatar.rs`,
`motion.rs`.

Design references to read first:
`/Users/anmol/.claude/skills/impeccable/reference/product.md` and
`/Users/anmol/.claude/skills/impeccable/reference/layout.md`.

## See your work — this is not optional

The app is rendered offscreen, against a seeded throwaway server, into PNGs:

```bash
RENDER_DB=<your db> RENDER_PORT=<your port> ./scripts/render-pages.sh <shot prefix…>
```

Then open the PNGs with the Read tool:
`docs/design-mocks/render/pages/<shot>-{wide,mid,narrow}.png`. Shots are defined
in `tests/page_render.rs` (`home`, `mytasks`, `projects`, `project`,
`project-overdue`, `task`, `task-shipped`, `task-handoff`, `login`). The seed is
`tests/fixtures/render-seed.sql`: five people, five projects (one overdue, one
paused, one done), every task state, PRs, a commit, Figma links, notes.

Render **before** you change anything, so you know what you are fixing, and
**after**, at all three widths. Do not report a page as done that you have not
looked at. If the build fails in a file you do not own, another agent is
mid-edit: wait a minute and retry; never edit it.

## What the audit found (already fixed on the server)

- Rollups counted `status = 'done'`, which no longer exists, so every project
  read 0%. Now `done_at` (both tracks' finish line), and dropped tasks are out
  of totals.
- A blocker is resolved at `handoff`, `completed`, `shipped` or `dropped` —
  not at `done_at`. `blockersDone`/`blockersTotal` on every task row.
- Team load: `open` = open/in progress/blocked (can act); `review` = handoff or
  engineering `completed` awaiting ship (waiting on others); `blocking`.
- `status_label()` returns sentence case at the source ("In progress",
  "Active"). Stop calling `sentence()` on it.
- `with_rail` stacks the rail **above** content on narrow windows.
- A table whose fill column is dropped promotes the first left column.
- `viz::toolbar(ui, |ui| …)` wraps filter controls. **The "N of M" count moves
  out of the toolbar** into the table heading (`shell::section_count_with` or a
  caption beside it) — it collided with the last filter at 820.

## New API (live, tested)

- `PATCH /api/user/projects/{id}` — any of `name`, `description`, `status`
  (`active|paused|done|archived`), `priority` (0-4), `startDate`, `targetDate`
  (`YYYY-MM-DD`, or `null` to clear), `labelIds` (replaces the set). 400 if the
  resulting start is after the target.
- `PATCH /api/user/tasks/{id}/details` — any of `title`, `body`, `priority`,
  `assigneeId` (uuid, or `null` to unassign). Anyone with write may do this.
  Reassigning across departments moves the task's track; a status the new
  track lacks (design `handoff` → engineer) becomes `open`.
- `DELETE /api/user/artifacts/{id}` — remove a link; audited. 404 "that link is
  already gone" on a second call.
- `GET /api/user/artifacts?parentType=project&parentId={id}` and `POST` with
  `parentType: "project"` — projects have resources too.
- `/api/user/home` → `projects[]` now carry `priority` and `targetDate`.
- `/api/user/me` now carries `name`.
- Task rows carry `assigneeName`, `assigneeEmail`, `assigneePersonId`,
  `discipline` (= assignee's department), `doneAt`, `manualReason`,
  `blockersDone`, `blockersTotal`, `projectId`, `projectName`.

## Rules

- **Vocabulary.** The user dropped "discipline" for "department". Every label
  on screen says **Department**. (JSON field is still `discipline`.)
- Zero literal colours; sizes via tokens or a named `const` with a one-line
  why-comment.
- **Nunito has no arrows, carets, ticks, boxes or `↳`.** Never draw a symbol as
  text — it renders as `□`. Paint it (`painter.line_segment`, `convex_polygon`)
  or say it in words. `…` `—` `·` are safe.
- Copy: `…` not `...`; loading states end with `…` ("Loading…", "Saving…");
  error text says what to do next; numerals for counts; sentence case for
  buttons and headings (a deliberate house style, like Linear).
- Every control has hover, keyboard focus (`motion::operable`), and — if
  icon-only — `on_hover_text`. A disabled button says why (hover text or a
  caption).
- **Destructive actions confirm inline** (remove a link, drop a task,
  reassign across tracks): a small "Remove this link? [Remove] [Keep]" row, not
  a modal.
- Drafts survive navigation: keep form state in the view's `State` or egui's
  temp store keyed by the entity id, never in a local that dies with the frame.
- Long content truncates; empty states teach ("No resources yet — PRs, docs
  and Figma files live here."), never "nothing here".
- Keep each file's comment voice: comments say *why*, not what.
- Build clean: `cargo build --all-targets --features app 2>&1 | grep -E "^(error|warning)" -A 8`
  must print nothing. Plain `cargo build` does not compile the app.
- Do not run the app. Do not commit. Do not touch files you do not own.

## Report

≤250 words: what you changed per page, what you saw in the renders that made
you change it, and anything you judged out of scope.
