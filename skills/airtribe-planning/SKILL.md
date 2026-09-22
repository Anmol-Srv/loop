---
name: airtribe-planning
description: Plan mycohort-api changes from current code evidence.
version: 0.1.1
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, planning, mycohort-api, architecture, impact-analysis]
    related_skills: [airtribe-domain, airtribe-data-changes, airtribe-verification]
---

# Airtribe Change Planning

Use this skill before editing mycohort-api. The goal is an evidence-backed impact map, not a plausible architecture invented from stale instructions.

## Source hierarchy

1. Current committed code, tests, migrations, runtime scripts, and CI.
2. Recent merged and related unmerged Git history.
3. Explicit user/product decisions.
4. Existing Markdown, Claude transcript notes, and Sigil facts only after validation.

When sources disagree, state the conflict and follow the highest-ranked source.

## Planning procedure

1. **Locate the entrypoint.** Start from the requested behavior and identify the route/plugin, controller, model, job/cron, external service, and tests that currently own it.
2. **Map the data path.** List inputs, authorization, reads/writes, transactions/locks, queued work, external effects, and user-visible output.
3. **Decide the layer.**
   - Route: HTTP registration, auth hooks, validation, response contract.
   - Controller: orchestration and request-specific formatting.
   - Model: entity lifecycle, persistence, transactions, relationships, coupled audit effects.
   - Service: external provider boundary or reusable stateless integration.
   - Bull/cron: deferred, contention-prone, or scheduled work.
   - Migration: durable schema/query invariant only.
4. **Prefer extension over parallel flow.** Search for canonical model transitions and equivalent features before adding files or methods. Explain why a new file/model/route is necessary if proposed.
5. **Classify risk.** Flag authz, migration, multi-row updates, locks, queues, cron, provider writes, externally mutable state, historic-data semantics, and API contract changes.
6. **Name tests before build.** Map each acceptance criterion to an existing or new focused test and each risk to a conditional verifier gate.

## Required plan format

```markdown
## Intent
## Current evidence
## Affected flows and ownership
## Proposed change, by file/layer
## Invariants and non-goals
## Test and verification matrix
## Branch choice and base SHA
## Open questions or assumptions
```

## Clarification rules

- **Collect every open question and ask them in ONE clarify call; never re-ask a question the user has already answered.** Finish the whole evidence pass first, accumulate the open questions, then ask once. Re-asking an answered question is a defect, not diligence.
- Record each answer in the plan's `Open questions or assumptions` section so a later pass can read it instead of asking again.

Ask instead of guessing when a request uses any of these ambiguously:

- programme vs CourseGroup vs Course/cohort;
- unit/sub-course vs a specific child cohort;
- application Lead vs sales Opportunity;
- course-level Admin/User role vs operational AdminUser;
- commercial registration vs actual Enrollment/access;
- stored status vs derived/display status;
- active enrollment vs any historic enrollment.

## Verification

The plan is complete only if every changed behavior has a named owner, every new persistent side effect has a rollback/recovery story, and every ambiguity is resolved or labeled as an assumption.
