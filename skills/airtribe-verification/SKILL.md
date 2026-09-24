---
name: airtribe-verification
description: Independently verify mycohort candidate commits.
version: 0.2.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, testing, review, evidence, worktree, pull-request]
    related_skills: [airtribe-task-lifecycle, airtribe-data-changes, airtribe-evidence-led-builds]
---

# Airtribe Independent Verification

Use after a builder creates a candidate commit. The builder's transcript and self-reported test results are diagnostic input, never completion evidence.

## Candidate-SHA contract

Verification is valid only when:

```text
candidate_sha == branch_head == verified_sha == reviewed_sha == pr_head
```

Any new commit invalidates prior evidence and reruns the affected gates.

## Baseline verifier procedure

1. Confirm candidate branch/worktree is not `master`/`main`, has a recorded base SHA, and has no uncommitted code changes.
2. Create a fresh detached verifier worktree at the exact candidate SHA (`airtribe_bridge.py verify` does this). Do not reuse the builder's `node_modules`, runtime state, or Claude session.
3. Record Git identity, changed-file list, diff-check output, Node and pnpm versions, and lockfile identity.
4. Run, in order:
   - `pnpm install --frozen-lockfile`
   - `pnpm lint` before Jest writes coverage
   - `NODE_ENV=test pnpm test`
5. Capture command, cwd, environment names (never values), start/end time, exit code, complete safe output, test counts, skipped tests, and coverage artifact.
6. Tear down the verifier worktree only after evidence is persisted.

The gate list lives in `/Users/anmol/Drive/Airtribe/airtribe-control-plane/config/gates.json` (override with `--gates` / `--gates-json`).

## Conditional gates

- **Migration/index/backfill:** disposable Postgres at base schema; candidate apply; schema/data assertion; candidate-only rollback and reapply.
- **SQL/transaction/lock semantics:** isolated Postgres with synthetic data.
- **Bull/Redis:** isolated Redis and controlled worker lifecycle.
- **App/global plugin startup:** smoke test with providers disabled/mocked and Bull disabled unless under test.
- **External providers:** explicit sandbox/contract test only when boundary behavior changes.
- **Staging:** explicit task-specific choice with target branch, access path, and evidence; never automatic.

## Review procedure

Run a separate read-only reviewer after deterministic gates. It checks scope, architecture ownership, API/permission mapping, data lifecycle, failure/retry behavior, race conditions, and missing tests. Bind its verdict to base SHA and candidate SHA.

Before reviewing, read `references/review-ratchets.md` and apply every line in it as a check.

## Ratchet

After **every** review that finds a defect class, append a one-line check for that class to `references/review-ratchets.md`. Where the check is mechanical (lint rule, grep, test), also add it to the verify gates file so it runs automatically and never has to be caught by a human again. A defect class found twice is a ratchet that was not written.

## PR procedure

Only after verification and review: push the candidate branch and open/update a PR targeting `master`. Record PR URL, head SHA, and hosted CI URL/status. Do not merge, approve, or modify `master`.

## Verification

A green task records all evidence in an artifact and in the task record. A failed, skipped, or unavailable conditional gate is explicit in the PR; it is never converted to "passed" by prose.
