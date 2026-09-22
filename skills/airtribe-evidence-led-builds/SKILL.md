---
name: airtribe-evidence-led-builds
description: Mirror live Airtribe patterns when building code.
version: 1.0.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, builds, evidence, fallbacks, conventions]
    related_skills: [airtribe-planning, airtribe-verification]
---

# Airtribe Evidence-Led Builds

Use while writing mycohort-api code. The live codebase — not plausibility — decides what a new path may do.

## Rules

- Treat the live codebase as the behavioral authority: before adding error handling, retries, fallbacks, recovery paths, or queue semantics, trace an equivalent current path and cite its files/tests in the task record.
- Do not invent a policy because it seems robust. If there is no equivalent code path and product has not specified the behavior, stop and surface the evidence gap.
- Reuse the existing parser/rater control flow when adding a provider fallback; do not add a parallel mechanism unless live code proves that is the project convention.
- Keep secrets outside automation. Document only non-secret environment variable names; never read, modify, request, or expose real credential files.
- Record the precedent and decision in the task folder (`/Users/anmol/Drive/Airtribe/.airtribe-worktrees/tasks/<task-id>/TASK.md`) before changing code.

## Review check

Reject a candidate when its error/fallback/retry behavior is not grounded in an existing path or an explicit product decision.
