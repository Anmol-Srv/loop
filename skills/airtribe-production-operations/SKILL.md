---
name: airtribe-production-operations
description: Produce safe Airtribe production lookup and REPL runbooks.
version: 0.1.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, production, database, repl, operations, read-only]
    related_skills: [airtribe-triage, airtribe-domain, airtribe-data-changes]
---

# Airtribe Production Operations

Use this for `prod-read-lookup` and `prod-repl-proposal` routes. It supports production **read-only discovery** and produces a human-run production REPL proposal when a record needs correction. It does not grant authority to execute a production write, create a branch, or open a PR.

## Credential boundary

- Keep the production read-only URL only in the dedicated Airtribe profile secret store/environment, for example `AIRTRIBE_PROD_READONLY_DATABASE_URL`. Never put the URL, a password, or a raw connection string in this skill, a repository, a task brief, a transcript, or a task record.
- For database discovery, load `airtribe-production-db-lookup` and use its approved read-only client/wrapper. Confirm the connection role is read-only before querying.
- Never make the write-capable production credential available to Hermes, Claude, Codex, Jev, or a generated script. The final write command is for the human to run manually in the approved production REPL.

## Procedure

1. **Resolve the object model locally first.** Inspect the committed current model, migration history, routes/controllers, and focused tests. Identify the primary entity, durable identifier, mutable fields, timezone semantics, associated records, queue/notification effects, and invariants.
   - Completion: the expected pre-state and all likely side effects are explicit; no schema field is guessed.

2. **Perform the smallest production read.** Use the approved read-only path to query only fields needed to identify the record and confirm the pre-state. Prefer UID and narrow predicates; do not dump unrelated rows or customer data.
   - Completion: exactly one target record is identified, or the task stops as ambiguous.

3. **Prepare a human-run change proposal.** The proposed REPL code must:
   - query by durable UID and assert exactly one matching row;
   - assert the expected old value/state before mutation;
   - run inside the repository’s established transaction/error pattern when supported;
   - mutate only named fields;
   - state required queue, cache, notification, audit, or derived-data handling after source inspection;
   - include a post-condition lookup and a practical rollback/repair note.
   - Completion: the command cannot silently modify the wrong record or a changed pre-state.

4. **Return a runbook, not an execution claim.** Include the redacted lookup result, proposed REPL command, validation command, expected result, risks, and explicit statement that the human must run the production write.
   - Completion: no production write was executed by an agent and no sensitive connection material appears in output.

## Session and schedule changes

For a request such as moving a community workshop session:

- establish whether the entity is a Workshop session, Course session, or another schedule representation before writing any query;
- resolve timezone from the entity/configuration or ask—never assume “7:30 pm” is IST merely because the operator is in India;
- inspect publication state, attendance/registration, calendar/video integration, reminders, and derived schedule fields before proposing the mutation;
- if multiple dates/times or recurring-session fields can govern behavior, stop and request product confirmation rather than updating the obvious timestamp.

## Pitfalls

- A read-only URL protects lookup authority, not data minimization. Do not copy production records into prompts or logs.
- Never turn a one-off operational correction into a fake code change just to obtain a PR.
- Do not use an update statement that lacks a durable-ID assertion and expected-pre-state check.
- If a persistent bug caused the bad data, create a linked `mixed-operations-and-code` task for the code fix; keep the immediate repair and the PR separately auditable.

## Verification

A finished operations response contains source/schema evidence, a unique redacted target identity, precondition checks, the exact proposed manual command, post-condition query, and declared human execution boundary. It never contains a production credential or an unverified claim that the production record was changed.
