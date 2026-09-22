---
name: airtribe-async-integrations
description: Safely change Airtribe jobs and external syncs.
version: 0.1.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, bull, cron, redis, sheets, idempotency, integrations]
    related_skills: [airtribe-planning, airtribe-data-changes, airtribe-verification]
---

# Airtribe Async and External Integrations

Use for Bull workers, cron jobs, Redis/mutex behavior, provider APIs, Google Sheets, retry paths, and reconciliation code.

## Design rules

- Move deferred or contention-prone work out of request paths into a named queue/worker when existing architecture supports it.
- Wire a job end-to-end: producer payload, queue registration/configuration, worker export, domain rehydration, failure handling, and observability.
- State the idempotency key and collision domain. Choose serialization at the sync-run, row, entity, or append-placement layer; one lock rarely protects all of them.
- Treat external state as mutable and partially failing. Do not use row position as durable identity when an external identifier exists.

## Reconciliation rules

- Define field ownership: source-of-truth, externally editable, derived, and do-not-write-on-null fields.
- Resolve durable identifiers before writing; cached row indexes/positions are repairable state only.
- Validate headers/schema/column mappings explicitly. Isolate a malformed formula/column rather than silently shifting subsequent writes.
- Make dry-run and apply use the same validation, duplicate checks, and diagnostics.
- On transient write failures, choose and test compensation/retry behavior; do not create duplicate external rows by retrying blindly.
- Preserve blocked/unsynced diagnostic state so operators can reconcile it later.

## Testing matrix

- Unit tests mock provider clients and assert request payload, retry, timeout, failure, and compensation paths.
- Queue/cron changes test producer-to-payload and worker delegation; use isolated Redis/worker execution when locks/retry/scheduling change.
- Provider sandbox/contract checks are conditional and explicit. Staging is exceptional, never a normal PR gate.

## Verification

The candidate evidence must name the idempotency mechanism, lock scope, external identifiers, dry-run behavior, provider mocks/sandbox boundaries, and recovery path. A successful one-off write is not proof that repeated or concurrent runs are safe.
