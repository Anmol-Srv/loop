# Airtribe Control Plane — Auth Design

**Date:** 2026-09-20
**Status:** Approved design, pre-implementation
**Supersedes:** the deferred-SSO note in `2026-09-18-airtribe-control-plane-design.md`

## 1. Purpose

Give the engineering team ordinary email-and-password sign-in, seeded from the
@airtribe.live address list, while leaving the agent credential path untouched.

Not every person will run an agent. Some will only ever update the board by
hand. The model must make that the default rather than a special case.

### Success criteria

- A seeded person can set their own password without an administrator ever
  knowing it, and without the system sending email.
- One code path turns a credential into authority, whoever presents it.
- An agent credential cannot apply a change to shared state, enforced by the
  database rather than by convention.
- Offboarding one person ends every session and every agent they own, in one
  command.

### Non-goals

- Two-factor authentication.
- Google SSO. When it lands it mints a `session` credential exactly like the
  password path, replacing one handler and nothing else.
- Self-serve password reset. Re-inviting is the same code path; a forgotten
  password is `acp-admin invite` again.
- Per-IP rate limiting. Lockout is per person, which is the right size for a
  team behind a VPN.

## 2. The credential model

Today `agent_token` does two jobs: it is a person's session and an agent's
identity. Passwords make the mismatch plain — sessions are short-lived,
numerous and self-expiring; agent credentials are long-lived, few and
deliberately minted.

The table is renamed `credential` and gains a `kind` discriminator. It stays
**one** table with **one** lookup, because exactly one code path turning a
credential into authority is a property worth protecting.

```
credential
  id           uuid
  kind         'session' | 'agent'
  label        session → the person's email;  agent → e.g. 'hermes'
  token_hash   sha-256 hex, unchanged
  owner_id     → person
  scopes       text[]
  expires_at   session → now + 30 days, slid forward on use
  last_used_at NEW — so stale sessions are visible
  revoked_at
  created_at

person
  + password_hash    argon2id, null until setup
  + password_set_at
  + failed_attempts  integer, default 0
  + locked_until     timestamptz, null

setup_code
  id, person_id, code_hash, expires_at, used_at, created_at
```

### The constraint that matters

```sql
CONSTRAINT agents_cannot_apply CHECK (
  kind <> 'agent' OR NOT (scopes && ARRAY['write', 'admin'])
)
```

An agent credential cannot hold `write` or `admin`. The proposal guardrail
stops being a rule someone can forget and becomes a property of the schema:
agent output is approved, never applied.

### Human scopes come from the role

Scopes are derived at sign-in from `person.role`, not chosen per session:

| role | scopes |
|---|---|
| `member` | `read`, `write` |
| `admin` | `read`, `write`, `admin` |

`admin` is new and gates the administrative endpoints in §5.

### Sliding expiry is not a write per request

Extending `expires_at` on every request would mean a database write per frame
from the Mac app. A session is extended only when fewer than 29 days remain —
one write per person per day.

## 3. Flows

```
SEED     acp-admin seed-team people.txt
         → person rows; no password, no credentials
         → refuses any address outside @airtribe.live

BOOTSTRAP acp-admin bootstrap-admin <email> <name>
         → seeds the person with role=admin and prints a setup code
         → the only way to create the first admin; see §4

INVITE   acp-admin invite <email>          (or: acp admin invite, with admin scope)
         → prints a single-use code, valid 48h, stored hashed
         → issuing a new code voids the previous one

SET UP   POST /api/auth/setup {email, code, password}
         → verifies, argon2id, marks the code used, signs them in
         → no separate login step afterwards

SIGN IN  POST /api/auth/login {email, password}
         → session credential, 30d sliding
         → web: httpOnly cookie · app and CLI: macOS Keychain
         → 5 failures locks the person for 15 minutes

USE      resolve() — unchanged shape, one lookup
         → touches last_used_at; slides expiry only when <29d remain

SIGN OUT POST /api/auth/logout → revokes that one session
         acp-admin revoke-person <email> → every session and agent they own
```

### Login must not enumerate the team

A wrong password and an unknown address return the identical error and take
comparable time. When no person matches, the verify still runs against a dummy
argon2 hash, so response timing does not disclose who has an account.

### Password rules

Minimum twelve characters. No composition rules — current NIST guidance is that
they push people toward predictable substitutions. The password may not equal
the email address. Nothing else.

## 4. Administrative bootstrap

The first admin is a chicken-and-egg problem: creating an admin requires admin
authority. `acp-admin` resolves it the same way it already does for the first
token — by talking to the database directly. It is the bootstrap path and must
keep working when no account exists.

```
acp-admin bootstrap-admin anmol@airtribe.live "Anmol"
  → creates or promotes the person to role=admin
  → prints a setup code
  → refuses if an admin already exists, unless --force
```

Requiring `--force` to mint a second bootstrap admin means an accidental re-run
cannot quietly hand out administrative access.

Everything else an administrator does is reachable over the API with the
`admin` scope, so day-to-day administration does not need database access:

| Action | CLI | Scope |
|---|---|---|
| seed people | `acp-admin seed-team` | direct DB |
| invite / re-invite | `acp admin invite <email>` | `admin` |
| promote or demote | `acp admin role <email> <role>` | `admin` |
| revoke a person | `acp admin revoke <email>` | `admin` |
| list sessions | `acp admin sessions` | `admin` |

## 5. API surface

| Route | Auth | Purpose |
|---|---|---|
| `POST /api/auth/login` | none | email + password → session |
| `POST /api/auth/setup` | none | email + code + password → session |
| `POST /api/auth/logout` | session | revoke this session |
| `GET  /api/user/me` | any | unchanged; now also returns `role` |
| `POST /api/user/agents` | `read` | mint an agent credential you own |
| `GET  /api/user/agents` | `read` | list your agents |
| `DELETE /api/user/agents/{id}` | `read` | revoke your own agent |
| `POST /api/admin/invite` | `admin` | issue a setup code |
| `POST /api/admin/role` | `admin` | change a person's role |
| `POST /api/admin/revoke` | `admin` | revoke a person entirely |
| `GET  /api/admin/sessions` | `admin` | active sessions across the team |

Minting an agent is a `read`-scope action on purpose: requiring `write` to
create one would stop a read-only reviewer from running a read-only agent, and
an agent can never do more than propose regardless.

**Scope vocabularies differ by kind, and do not nest.** A person holds
`read`/`write`/`admin`; an agent holds `read`/`claim`/`propose`. A subset rule
across the two would be meaningless — a human has no `claim` to delegate. The
rule is therefore by kind:

| kind | permitted scopes |
|---|---|
| `session` | `read`, `write`, `admin` — derived from the role, never chosen |
| `agent` | any of `read`, `claim`, `propose`; `write` and `admin` rejected |

`write` on an agent is refused by the database constraint, not merely by the
route, so no future code path can grant it by accident.

## 6. Clients

| Surface | Change |
|---|---|
| Mac app | Login becomes email + password; a first-time path takes email, setup code and a new password. The session still lands in the Keychain. |
| Web | The same two forms. The token-paste field is removed. |
| CLI | `acp login`, `acp logout`, `acp agent mint\|ls\|revoke`, `acp admin …`. |
| acp-admin | Gains `seed-team`, `bootstrap-admin`, `invite`, `revoke-person`. Keeps direct database access. |
| Agents / MCP | Unchanged. Same bearer token, same `resolve()`, same tools. |

## 7. Migration

Every credential currently in the database is demo data seeded during
development, and all of it carries `read,write` — which the new constraint
forbids for agents. A migration that guessed which rows were sessions and which
were agents would be fiction.

The migration therefore **drops all existing credentials**. Everyone sets up
fresh. This is honest for a system with no real users, and it means the
constraint holds from the first row onward.

The one breaking change for humans: token-paste sign-in is gone. `acp-admin`
can still mint a raw credential directly against the database, which is the
escape hatch if the password path ever fails.

## 8. Testing

The owner has asked for minimal tests. Auth is where that exception does not
apply — these are the checks that would otherwise be found in production:

1. An agent credential carrying `write` is rejected by the database.
2. A `propose`-scoped agent still cannot apply (the existing regression test,
   which must keep passing through the rename).
3. Five failed logins lock the account; a correct password during lockout still
   fails; the lock lifts on time.
4. Login timing and error text are indistinguishable for a wrong password and
   an unknown address.
5. A setup code is single-use, expires, and is voided by re-issuing.
6. `revoke-person` ends every session and every agent that person owns.
7. A session slides only when under the 29-day threshold.

## 9. Delivery order

1. **Schema** — rename, new columns, `setup_code`, the constraint, drop old rows.
2. **Admin bootstrap** — argon2, setup codes, `bootstrap-admin`, `seed-team`,
   `invite`. Nothing downstream is testable until an admin exists.
3. **Auth routes** — login, setup, logout, lockout, sliding expiry.
4. **Agent and admin APIs** — self-serve agents, the `admin`-scoped endpoints.
5. **Clients** — CLI, web, Mac app.
