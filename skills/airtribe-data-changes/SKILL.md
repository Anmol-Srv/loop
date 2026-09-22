---
name: airtribe-data-changes
description: Design and verify mycohort database changes.
version: 0.1.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, postgres, knex, migrations, indexes, backfills]
    related_skills: [airtribe-planning, airtribe-domain, airtribe-verification]
---

# Airtribe Data Changes

Use when a mycohort-api task changes schema, indexes, raw SQL objects, data backfills, query semantics, or persistent concurrency behavior.

## Decision rules

- Add a migration only for durable schema, index, constraint, trigger/function, or required data-transition changes. Do not use a migration to patch transient application behavior.
- Connect every new index or materialized object to the query/filter it supports. State the expected selectivity, write cost, and rollback behavior.
- Extend the existing Objection model when table shape or entity behavior changes. Use raw SQL only when Knex/Objection cannot express the required operation safely.
- Treat null, timezone, uniqueness, historic/parked records, and backfill idempotency as explicit design decisions.

## Migration procedure

1. Inspect comparable current migrations and the affected model/query.
2. Generate the migration through the repository `pnpm knex` wrapper; never invent a timestamp prefix.
3. Write a real inverse `down` for every new object. For raw SQL, name objects explicitly and tear each down.
4. Use transaction opt-out only when required (for example concurrent indexes); document failure/recovery behavior.
5. Make backfills bounded, idempotent, resumable where possible, and safe on historical data.
6. Plan apply, schema/data assertion, candidate-only rollback, and reapply against a disposable PostgreSQL database already at the base SHA.

## Concurrency and integrity

- Put multi-row business invariants behind transactions, locks, constraints, or a documented combination. A pre-read alone is not sufficient.
- Distinguish a process mutex/advisory lock from full transaction atomicity.
- Do not collapse historic/reporting records into current access state without explicit business approval.

## Prohibited shortcuts

- Never run `migrate:latest`, rollback, seeds, one-time scripts, or production-like restore flows against a shared developer database as a default check.
- Never whole-history rollback to test one candidate migration.
- Never use a mock-Knex SQL-shape test as proof of row membership, lock, constraint, or planner behavior.

## Verification

Data changes require focused unit/SQL tests plus an isolated PostgreSQL gate. Evidence includes base and candidate SHA, database isolation descriptor with no secret URL, migration output, schema/data assertions, candidate-batch rollback/reapply, and cleanup result.
