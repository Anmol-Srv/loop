# Ship blockers — the contract

Source: `docs/superpowers/reports/2026-09-23-ship-readiness.md`. Every agent on
this pass reads this file and the report's section for its own items.

Three owners, disjoint files. Nobody edits a file outside their list. If a
build fails in a file you do not own, another owner is mid-edit — wait a
minute and retry. Never kill a process you did not start: track your own with
`$!` or its port, never `pkill -f acp-server`.

| Owner | Items | Files |
| --- | --- | --- |
| **Server** | H1, H2 (server), H3 (server), H4, H5, H7, and three Mediums: agent mint clamp, artifact URL scheme, `add-person` normalisation | `src/controllers/**`, `src/routes/**`, `src/models/**`, `src/middleware/**`, `src/bin/acp-admin.rs`, `migrations/` (new files only), `tests/*.rs` (new files, or new tests appended) |
| **Client** | B2, H2 (client), H3 (client), H6, sign out on 401, save confirmation on the project page | `src/desktop/**` (not `design/`, see below), `src/cli/creds.rs`, `src/cli/client.rs` |
| **Parent** | B1, B3, B4, SIGTERM, logging, health check, `image` off the server build, bundle, compose stack, staging doc, integration test | `Dockerfile`, `Cargo.toml`, `src/main.rs`, `src/config/`, `deploy/`, `scripts/`, `docs/deploy/` |

`src/desktop/design/**` is shared: the Client owner may add to it but must not
change an existing function's signature.

## The API contract — build to this exactly

### Errors

A precondition failure is **409** with the existing error shape:
`{"success": false, "error": {"code": "CONFLICT", "message": "…"}}`. The message
is written for a person ("Dhaval moved this to Shipped a moment ago.").

### H1 — transitions follow the track

`GET /api/user/tracks` (read scope) returns the one table both sides use:

```json
{ "eng":    { "open": ["in_progress", "blocked", "dropped"], … },
  "design": { "open": ["in_progress", "blocked", "dropped"], … },
  "evidence": { "eng": { "completed": ["pr", "commit"] },
                "design": { "handoff": ["figma"] } },
  "anyone": ["shipped"] }
```

Legal moves (the server is the authority; the table above is generated from
the same Rust function that enforces it):

| Track | From | To |
| --- | --- | --- |
| eng | open | in_progress |
| eng | in_progress | completed, open |
| eng | completed | shipped, in_progress |
| eng | shipped | in_progress |
| design | open | in_progress |
| design | in_progress | handoff, open |
| design | handoff | completed, in_progress |
| design | completed | in_progress |
| both | any except dropped | blocked, dropped |
| both | blocked | in_progress |
| both | dropped | open |

A move not in the table is **400** naming the legal next states. `shipped` is
still settable by anyone — but only from `completed`.

### H2 — a move states what it moved from

`PATCH /api/user/tasks/{id}` accepts an optional `expectedStatus`. When present
and not equal to the current status → **409**, and nothing changes. The app
always sends it.

### H3 — edits state what they edited

`PATCH /api/user/tasks/{id}/details` and `PATCH /api/user/projects/{id}` accept
an optional `expectedUpdatedAt` — the `updatedAt` string exactly as the server
last returned it. When present and not equal → **409**, nothing changes. The
app sends it, and sends **only the fields the user changed**.

### H7 — departments and roles are admin actions

- `PATCH /api/user/people/me` no longer accepts `department` (400 if sent).
- `PATCH /api/admin/people/{id}` (admin scope) accepts `department`
  (`design|frontend|backend`) and/or `role` (`member|manager|admin`).
- A department change resets that person's in-flight tasks the way
  reassignment does: any status the new track lacks becomes `open`, `doneAt`
  is recomputed.
- `acp-admin set-department <email> <dept>` and `acp-admin set-role <email>
  <role>` do the same from the CLI.

### H4, H5 — accounts

- Setting a password by any path (setup code, `acp-admin set-password`)
  revokes that person's existing sessions.
- A session is refused 90 days after it was created, however recently it was
  used.
- `acp-admin set-password` refuses passwords under 12 characters.

### Mediums the server owner also takes

- An agent credential cannot mint another credential (403); `validDays` is
  clamped to 1–90; requested scopes must be a subset of the caller's.
- Artifact URLs must be `http://` or `https://` (400 otherwise). Commits are
  exempt — their `url` holds a hash.
- `acp-admin add-person` normalises and domain-checks email the way `seed-team`
  does.

## The client's side (B2, H2, H3, H6)

- **B2** — a "Server" field on the login screen, prefilled, saved next to the
  credential file in Application Support. Resolution order: `ACP_URL` env
  var, saved value, `http://localhost:8080`. Signed in, the sidebar footer
  shows which server.
- **H2** — refetch the visible page when the window regains focus and every
  30 s while it has focus; send `expectedStatus` on every move; on 409 show the
  server's message, refetch, and leave the user on the page.
- **H3** — send only changed fields plus `expectedUpdatedAt`; on 409 keep the
  user's draft, show the message, and refetch so they can reapply.
- **H6** — 15 s timeout on every request; requests run concurrently.
- A **401** on any request signs out with "Your session ended — sign in
  again."
- The project page confirms a save ("Saved.") the way the task page does.
- The status dropdown and primary action offer only the moves in
  `GET /api/user/tracks` for the task's track and current status.

## Verification

Each owner: `cargo build --all-targets --features app` clean, a test for every
server rule (`DATABASE_URL=postgres://localhost:5433/acp_dev cargo test
--features app`), and — for the client — the affected pages rendered with
`RENDER_DB=<yours> RENDER_PORT=<yours> ./scripts/render-pages.sh <shots>` and
looked at. Report in ≤250 words.
