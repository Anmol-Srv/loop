# Ship readiness — 23 September 2026

**Question:** can the control plane be hosted for the five-person team
(Anmol, Dhaval, Chinmay, Evana, Pratik) and used day to day?

**Verdict: not yet — but close.** The product works: every flow passed when
exercised as the person who would really do it (41 of 41 checks), and the
application's own security held up under direct probing. What stops it is
almost entirely *getting it to people*: the server image does not run, the
app cannot be pointed at a hosted server, there is no TLS, and macOS will
refuse to open the app on anyone else's machine. Those four are hours of work,
not days. Seven more issues should be fixed before five people share one
board, because they either let a rule be skipped or let one person silently
undo another's work.

## How this was reviewed

Three independent read-only reviews, each against its own throwaway server
and database, then every blocker re-checked against the code before it went
into this report.

| Review | What it did |
| --- | --- |
| Flows | Scripted every flow over HTTP as the real actor — Anmol creating, Evana handing off, Chinmay completing, Dhaval shipping someone else's work — then checked every rollup against a hand count. Read the app's networking for multi-user behaviour. |
| Security | Probed login, tokens, every route as a regular member, the agent/MCP surface, SQL construction, error output and transport. |
| Hosting | Built the Docker image and ran it, applied every migration to an empty database, onboarded a whole team from zero using only `acp-admin`, and checked the macOS bundle's signature, architecture and Gatekeeper verdict. |

## What already works

- **Flows: 41 of 41 checks pass.** Project creation with tasks, dates,
  priority and labels; both tracks end to end; the evidence gates (design
  handoff needs Figma, eng completion needs a PR or a stated reason); only the
  assignee moves a task; notes; editing, reassigning across tracks, removing
  links; and every rollup — home, project flow, task list — agreeing with a
  hand count, with dropped tasks out of totals and `doneAt` set only at each
  track's finish line.
- **Application security is sound for a small team.** Tokens are ~244 random
  bits stored only as SHA-256 hashes; the client credential file is `0600`;
  admin routes refuse members; agent credentials cannot hold `write` (enforced
  by a database constraint) and their board edits queue for approval; every
  SQL value is bound, never interpolated; 500s return "internal server error"
  and keep detail in the server log.
- **Migrations apply cleanly to an empty database** — all ten in 0.15 s.
- **87 automated tests pass.**

## Blockers — nobody can use it until these are done

| # | Problem | Evidence | Fix |
| --- | --- | --- | --- |
| B1 | **The Docker image builds but its binaries will not start.** | Build stage `rust:1.96-slim` is Debian trixie (glibc 2.41); runtime stage is `debian:bookworm-slim` (glibc 2.36). `docker run` → `GLIBC_2.38 not found`. `Dockerfile:4,12` | Runtime stage `FROM debian:trixie-slim`. One line. |
| B2 | **The app can only talk to `localhost`.** | `creds.rs:89-90` reads `ACP_URL` from the environment, which a double-clicked `.app` never has. The login screen shows the URL but cannot change it. | A "Server" field on the login screen, saved beside the credential. Order: env var, saved value, default. |
| B3 | **No TLS; the server listens on every interface.** | `main.rs:29-30` binds `0.0.0.0` with a plain listener; no TLS anywhere in the server. Every request carries a bearer token; login carries a password. | Caddy (or nginx) in front with a certificate, or reach it only over Tailscale. Bind the app to `127.0.0.1`. |
| B4 | **macOS will refuse to open the app on a teammate's Mac.** | `codesign -dv` → `Signature=adhoc`, no team ID; `spctl -a -vv` → `rejected`. Once downloaded and quarantined, right-click → Open no longer works on current macOS. | Now: ship with one documented step, `xattr -dr com.apple.quarantine "/Applications/Airtribe Control Plane.app"`. Later: Developer ID ($99/yr) and notarization. |

## High — fix before five people share one board

| # | Problem | Evidence | Fix |
| --- | --- | --- | --- |
| H1 | **Anyone can ship an open task, skipping the PR check.** | Dhaval set Anmol's *open* task to `shipped`: 200, `doneAt` stamped, no PR, no reason. The evidence gate checks only the status moved *to* (`task.rs:167-172`); nothing checks the status moved *from*. | Enforce the track's order: `shipped` only from `completed`, `completed` only from `in_progress`, `handoff` only from `in_progress`. |
| H2 | **Screens go stale, and a stale move silently undoes a teammate's.** | Every page is fetched once and cached (`net.rs:104-108`); nothing polls, nothing refreshes on window focus. The server applies whatever status is sent, so Chinmay working from a stale page can move back a task Dhaval just shipped. | Refetch on window focus and every ~30 s on Home and My Tasks; send the status the user saw and return 409 if it changed. |
| H3 | **Saving a title overwrites someone else's description.** | Task Save sends `{title, body}` together (`task.rs:490`), project Save `{name, description}` (`board.rs:555-558`); the draft survives navigation, so it can be hours old. Last write wins. | Send only changed fields, with an `updatedAt` precondition. |
| H4 | **A password reset does not sign anyone out, and sessions never expire while used.** | Expiry slides 30 days on each use (`auth.rs:64-75`); neither reset path touches existing sessions. Re-inviting someone after a leak leaves the other session live. | Revoke a person's sessions when their password is set; cap a session at 90 days from creation. |
| H5 | **`acp-admin set-password` accepts any password.** | Set `abc`; login with `abc` returned a token. The tool only warns. | Refuse under 12 characters, or have it issue a setup code instead. The four test passwords set earlier this week must be replaced. |
| H6 | **One slow request freezes the whole app.** | Requests run one at a time on one worker (`net.rs:59-68`) and the HTTP client has no timeout (`client.rs:24`). A stalled request leaves every page spinning. | A 15 s timeout; run requests concurrently. |
| H7 | **Departments can be changed by the person themselves, and by nobody else.** | `PATCH /people/me` lets Chinmay switch to design; his shipped eng task then reads `design / shipped`, a state that track does not have. There is no admin route or `acp-admin` command to set a department, and `manager` is accepted by the database but refused by the API. | Make department an admin action (`acp-admin set-department`, `set-role` including `manager`); apply the same track reset reassignment uses. |

## Medium — worth doing in the first week

- **An agent can mint more agents with any lifetime.** The "only a person can
  own an agent" check never fires, because an agent's `person_id` is its
  owner's (`agent.rs:56-62`, `auth.rs:80`); a read/propose token minted a token
  valid until 2126. Carry the credential kind on `Caller`, refuse agents, clamp
  `validDays` to 1–90, never grant scopes the caller lacks.
- **A session that expires mid-use is not noticed** until Refresh. Sign out on
  any 401.
- **`SIGTERM` kills the server outright** (only `ctrl_c` is handled,
  `main.rs:34`), so every deploy drops in-flight requests.
- **Logs are silent by default** (`RUST_LOG` unset → nothing), coloured when
  enabled, and there is no request log. Set `RUST_LOG=info`, turn off ANSI.
- **No backups.** Use managed Postgres with point-in-time recovery and test
  one restore.
- **No rate limit on login**, and argon2 runs on the async runtime. Per-account
  lockout works, but anyone can lock a teammate out for 15 minutes. Limit per
  IP at the proxy; move argon2 into `spawn_blocking`.
- **Artifact URLs accept any scheme** (`artifact.rs:30`), and the app opens
  them with `open`. Accept only `http(s)://`.
- **`acp-admin add-person` skips email normalisation and the domain check.**
  Use `seed-team`, which checks, or route it through the same function.
- **The project page gives no confirmation after a save.** The task page does.
- **Intel Macs cannot run the app** — it is arm64 only. Build universal if
  anyone is on Intel.

## Low — note and move on

- The per-department flow does not add up to the project total when tasks are
  unassigned.
- Reassigning a handed-off design task back to `open` makes an already-shipped
  dependent show its blocker as unresolved again.
- The Docker `HEALTHCHECK` runs `acp --version` rather than hitting
  `/health/ready`.
- The `image` crate is compiled into the server though only the app uses it.
- The job worker uses `LISTEN`, so `DATABASE_URL` must be a direct or
  session-mode connection, not a transaction pooler.
- No client version check, so an old app against a newer server fails
  silently. Add `minClientVersion` to `/health`.
- With only read/propose, an agent can create labels, post notes and change its
  owner's department without approval.
- Login timing differs by ~0.5 ms between "no such account" and "wrong
  password". Acceptable for a known team of five.

## Decided, not a problem

- **Any member can edit any project or task's details.** Every edit writes an
  audit row, nothing destroys data beyond a link, nothing escalates privilege.
  Right for five people who trust each other; revisit if the team grows.
- **No delete for tasks or projects.** `dropped` and `archived` cover it and
  keep history.
- **No self-service password change.** An admin re-invite issues a setup code;
  fine for five people.

## Minimal hosting plan

1. Fix B1 and the two ops items that bite on the first deploy: `SIGTERM`, and
   `RUST_LOG=info` with ANSI off.
2. Managed Postgres (Neon, Supabase or RDS) with point-in-time recovery; use
   its **direct** connection string.
3. Run the container on one small VM or a Fly/Render instance with
   `DATABASE_URL` and `PORT=8080`, bound to localhost. It migrates itself on
   boot.
4. Caddy in front: `acp.<domain> { reverse_proxy 127.0.0.1:8080 }` — automatic
   TLS. Probe `/health/ready`.
5. Onboard: `acp-admin bootstrap-admin`, then `seed-team` with the other four,
   then `invite` each for a setup code. Set departments and Dhaval's manager
   role once H7 lands (SQL until then).
6. Ship the app with the Server field (B2), a universal build if anyone is on
   Intel, and the one-line `xattr` step (B4).
7. After launch: Developer ID and notarization, a version check, one restore
   drill.

## Order of work

1. **Blockers B1–B4** — a few hours; the team can start after these.
2. **H1, H5 and H7** before real data goes in — they protect the rules and the
   accounts.
3. **H2, H3, H4 and H6** in the first week — they only bite once several
   people are working at once, but that is the point of hosting it.
4. Medium, then Low, as they come up.
