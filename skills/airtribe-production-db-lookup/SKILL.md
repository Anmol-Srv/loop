---
name: airtribe-production-db-lookup
description: Perform bounded read-only Airtribe production DB lookups.
version: 0.2.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, production, postgres, read-only, lookup, sessions]
    related_skills: [airtribe-production-operations, airtribe-triage, airtribe-domain]
---

# Airtribe Production Database Lookup

Use this only for an already-classified `prod-read-lookup` or `prod-repl-proposal` task. It finds a narrow, verified production fact—such as a session UID and current time—without making a source branch, running a write, or exposing the database credential.

## Credential setup

The only accepted credential source is the Airtribe profile environment variable:

```text
AIRTRIBE_PROD_READONLY_DATABASE_URL
```

The lookup tool is `prod_db_query` (Hermes plugin `airtribe-prod-db`, source `hermes-plugins/airtribe-prod-db/`). It reads the URL from the profile environment and never prints it; connects through `psql` with libpq env vars, so nothing appears on a command line; runs `BEGIN READ ONLY` with `default_transaction_read_only=on`, a 15s statement timeout and a 2s lock timeout; accepts exactly one SELECT/WITH/EXPLAIN/SHOW statement; caps rows (default 20, max 200); redacts PII-named columns; verifies the connected role has no superuser/create/write grants and refuses otherwise; rolls back every path.

TLS: with no CA file the tool uses `sslmode=require` (encrypted, chain unverified). Place DigitalOcean's cluster CA at `~/.hermes/profiles/airtribe/certs/prod-db-ca.crt` (or set `AIRTRIBE_PROD_DB_CA`) and it switches to `verify-full` automatically. Never ask for a certificate or credential in chat; if the tool is missing, stop and report that the profile environment or plugin is not configured.

- Never include the URL, password, host, port, database name, or decoded connection fields in a skill, command, task brief, control-plane task record, source file, PR, log, or chat response.
- Treat a URL pasted into chat as exposed. Do not copy it from conversation history; rotate the underlying password through normal operations.
- The configured database principal must be a true database-level read-only role. The tool checks this before every lookup and rejects a write-capable role rather than trusting the URL label.

## Procedure

1. **Confirm the route and model.** Load `airtribe-triage`, then inspect current committed models, migrations, route/controller code, and focused tests. Determine the precise entity and durable identifier before production access.
   - Completion: entity table/model, expected fields, timezone semantics, and side effects are evidence-backed.

2. **Readiness.** Confirm `prod_db_query` is in your tool list. Call it once with `select 1` and check the reply says `role_read_only: true` and `transaction: READ ONLY, rolled back`. If the tool is absent or errors on configuration, stop; do not ask for the credential or certificate in chat.
   - Completion: a `select 1` round trip succeeded with `role_read_only: true`.

3. **Learn the schema from code, not from production.** Column names come from committed models and migrations; use `information_schema` through the tool only to confirm a doubt.

4. **Write a narrow lookup file outside the product repository.** Use a task-local artifact path and a single query that:
   - selects only needed non-PII fields;
   - uses durable identifiers or narrow predicates;
   - uses parameter-like literals only after the entity is understood;
   - includes `LIMIT`, normally `LIMIT 2` to prove uniqueness without exposing a broad result set;
   - contains no locks, DDL, DML, control commands, multiple statements, or privileged functions.
   - Completion: a reviewer can explain why every selected field and predicate is needed.

5. **Run the guarded query.** Call `prod_db_query` with `sql` (the single statement from step 4) and `max_rows` (normally 2). Leave `redact` at its default unless the task needs the value and it will not be stored in evidence.
   - Completion: the reply shows `role_read_only: true`, `truncated: false`, and identifies exactly one target or stops as ambiguous. Record only the redacted result and the statement in the task evidence.

6. **Use the result according to the operation class.**
   - `prod-read-lookup`: provide a minimal redacted finding with source/schema evidence.
   - `prod-repl-proposal`: hand the evidence to `airtribe-production-operations` to create an asserted human-run REPL proposal. Do not execute that proposal.
   - Completion: no write-capable credential, production write, branch, or PR was created.

## Workshop-session lookup checklist

Before searching for a session, establish:

- whether the record belongs to a Workshop or Course flow;
- the responsible session model/table and its durable UID;
- the stored time fields and timezone interpretation;
- whether publication, registration, video/calendar links, reminders, attendance, or queued work depend on the time field;
- the expected current time and the requested target time with an explicit timezone.

If the lookup finds multiple plausible sessions, missing timezone context, or an unexpected lifecycle state, stop and report the ambiguity rather than selecting a “closest” record.

## Pitfalls

- Read-only transaction mode is defense in depth, not a substitute for a database role with no write grants.
- `SELECT` can still expose sensitive data; select minimal fields and leave the tool's redaction on.
- Do not bypass the tool with `terminal` + `psql`; the tool is the audited path and the only one with the role and statement guards.
- Do not give a raw SQL query to Jev or any remote model. Jev receives only sanitized routing context, if used at all.
- Do not use the wrapper for schema migrations, operational writes, or a convenience production console.

## Verification

Run `python3 -m unittest airtribe-prod-db.test_query` from `hermes-plugins/` after changing the plugin. A real lookup is valid only if the wrapper confirms read-only mode, the result is bounded/redacted, exactly one record matches, and no credential value appears in the evidence.
