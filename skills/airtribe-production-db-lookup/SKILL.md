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

Production TLS additionally requires:

```text
NODE_EXTRA_CA_CERTS=/Users/anmol/.hermes/profiles/airtribe/certs/prod-db-ca.pem
```

- Set the URL through the local profile secret workflow, using `templates/airtribe-profile.env.example` as the variable-name reference.
- Never include the URL, password, host, port, database name, or decoded connection fields in a skill, command, task brief, control-plane task record, source file, PR, log, or chat response.
- Treat a URL pasted into chat as exposed. Do not copy it from conversation history; rotate the underlying password through normal operations.
- The configured database principal must be a true database-level read-only role. The wrapper checks privileged flags plus relation, sequence, and schema-create grants before every lookup; it rejects a role with write capability rather than trusting the URL label.
- Obtain the CA PEM/bundle only from the approved Airtribe infrastructure or secret-management owner. Keep it outside every repository, protect it from group/world writes, and never replace `rejectUnauthorized: true` with an insecure TLS setting.
- Run `/Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/install_airtribe_prod_db_ca.js` once to select and validate the PEM through a local native picker. It copies the bundle to the profile, writes only `NODE_EXTRA_CA_CERTS`, and never prints the certificate path or contents.
- Run the guarded client only through `/Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/run_prod_readonly_query.js`. It parses only the two required profile variables, then spawns a new Node child with `NODE_EXTRA_CA_CERTS` present from process start. This safely avoids a Desktop restart and a late dotenv-load TLS failure.

## Procedure

1. **Confirm the route and model.** Load `airtribe-triage`, then inspect current committed models, migrations, route/controller code, and focused tests. Determine the precise entity and durable identifier before production access.
   - Completion: entity table/model, expected fields, timezone semantics, and side effects are evidence-backed.

2. **Install or replace the approved CA bundle when needed.** From the `mycohort-api` dependency root, use `terminal` to run:
   ```text
   node /Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/install_airtribe_prod_db_ca.js
   ```
   This opens a native local picker; select only the approved PEM from the infrastructure owner. It does not connect to production.
   - Completion: output confirms `certificate_installed: true` and `profile_env_updated: true` without revealing paths or contents.

3. **Run a local readiness check.** From the `mycohort-api` dependency root, use `terminal` to run:
   ```text
   node /Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/run_prod_readonly_query.js --doctor
   ```
   It reports only local readiness, driver availability, and whether the approved CA bundle is configured/readable—never the URL or certificate path. The runner launches the query process with the CA present at Node startup but the doctor itself does not connect, so `database_role_verified_read_only` remains false at this stage.
   - Completion: the output says `ready: true`. If absent or invalid, stop; do not ask for the credential in chat.

4. **Write a narrow lookup file outside the product repository.** Use a task-local artifact path and a single query that:
   - selects only needed non-PII fields;
   - uses durable identifiers or narrow predicates;
   - uses parameter-like literals only after the entity is understood;
   - includes `LIMIT`, normally `LIMIT 2` to prove uniqueness without exposing a broad result set;
   - contains no locks, DDL, DML, control commands, multiple statements, or privileged functions.
   - Completion: a reviewer can explain why every selected field and predicate is needed.

5. **Run the guarded query.** Use `terminal` from the `mycohort-api` dependency root to run:
   ```text
   node /Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/run_prod_readonly_query.js --sql-file <task-artifact-query.sql> --max-rows 2
   ```
   The wrapper resolves the repository `pg` driver, requires TLS verification, opens `BEGIN TRANSACTION READ ONLY`, verifies that the connected role has no privileged/write/create grants, sets connection/statement/lock timeouts, rejects write-like SQL, rolls back on every path, and redacts common PII-named fields.
   - Completion: output confirms both `transaction_read_only: true` and `database_role_verified_read_only: true`, returns no more than the requested bound, and identifies exactly one target or stops as ambiguous.

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
- `SELECT` can still expose sensitive data; select minimal fields and leave the wrapper’s redaction on.
- Do not invoke `prod_readonly_query.js` directly from a shell that late-loads the profile `.env`; use `run_prod_readonly_query.js` so Node sees the custom CA before startup.
- Do not give a raw SQL query to Jev or any remote model. Jev receives only sanitized routing context, if used at all.
- Do not use the wrapper for schema migrations, operational writes, or a convenience production console.

## Verification

Run `node tests/test_prod_readonly_query.js` and `node tests/test_prod_db_ca_setup.js` from the control-plane repo root after changing the helpers. A real lookup is valid only if the wrapper confirms read-only mode, the result is bounded/redacted, exactly one record matches, and no credential value appears in the evidence.
