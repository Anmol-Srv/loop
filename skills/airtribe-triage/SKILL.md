---
name: airtribe-triage
description: Route Airtribe requests before creating a worktree.
version: 0.1.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, triage, routing, jev, production, worktree]
    related_skills: [airtribe-task-lifecycle, airtribe-production-operations, airtribe-planning]
---

# Airtribe Request Triage

Use this before planning, opening a task record, or creating a worktree. Its job is to select the **operation path**, not merely an LLM executor. A task that can be resolved by inspection or an operational runbook must not become a branch and PR.

## Route classes

| Class | Use when | Artifact | Branch/PR |
|---|---|---|---|
| `codebase-query` | The user needs an explanation, location, history, or read-only analysis. | Evidence-backed answer. | Never. |
| `prod-read-lookup` | A production fact is required and the dedicated read-only access path suffices. | Minimal redacted result. | Never. |
| `prod-repl-proposal` | Production data must change, but the requested output is a human-run REPL command/runbook. | Read-only evidence plus proposed command. | Never. |
| `local-repl` | A local/disposable environment check or repair is requested. | Local evidence and cleanup result. | Never by default. |
| `code-change` | Persistent product code, test, schema, or config must change. | Verified commit and PR. | Always. |
| `mixed-operations-and-code` | An immediate data correction and a durable product fix are both needed. | Linked operations runbook and code PR. | PR only for code portion. |
| `ambiguous` | Environment, entity, authority, or desired artifact cannot be safely inferred. | One precise clarification. | Never until resolved. |

## Procedure

1. **Extract the route facts.** Record the target environment, exact entity type, requested outcome, whether source code must persistently change, and whether a human expects a command or delegated execution.
   - Completion: classify the request into exactly one route class, or `ambiguous`.

2. **Apply the local policy contract.** Use `terminal` to run
   `python3 /Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/airtribe_triage.py classify --operation <class>`.
   Treat its output as a hard boundary: it determines whether worktrees, PRs, production reads, or a production write proposal are permitted.
   - Completion: the task record names the class and policy-derived allowed actions.

3. **Use Jev only as an executor second opinion.** For a non-sensitive, already-classified task, call the Jev harness through `terminal` with a compact sanitized description. Record its executor/confidence/reason. Jev may choose Hermes, Claude, or Codex; it cannot override the route class or production policy.
   - Do not include credentials, raw database output, personal data, session UIDs, source code, or production connection details in the Jev prompt.
   - A low-confidence result, absent Jev credential, or a non-code route resolves to Hermes—not a speculative external worker.

4. **Choose the path.**
   - `codebase-query`: Hermes performs read-only inspection and answers with citations.
   - `prod-read-lookup` and `prod-repl-proposal`: load `airtribe-production-db-lookup` and `airtribe-production-operations`; no worktree or external coding worker.
   - `local-repl`: execute only against a confirmed local/disposable target and retain cleanup evidence.
   - `code-change`: load planning/domain skills, create the dedicated worktree, then optionally let Jev choose the builder.
   - `mixed-operations-and-code`: make two linked task records. The operations runbook must not wait for a PR, and the code PR must not silently perform the production correction.
   - Completion: exactly one route path is active; unneeded branch/PR machinery was not started.

## Decision rules

- A request phrased as "change production data" is **not** a coding task by default.
- "Find the UID and give me the command" is `prod-repl-proposal`, even if source inspection is needed first.
- A one-off data repair that exposes a recurring product bug is `mixed-operations-and-code`; do not disguise the repair as a code deploy.
- If time is stated without an unambiguous timezone, classify as `ambiguous` until the timezone is confirmed or recovered from the relevant entity/configuration.
- Use branch age and semantic overlap only after the class is `code-change` or the code half of `mixed-operations-and-code`.

## Verification

The route record must state: class, target environment, executor choice and whether Jev was consulted, branch/PR decision, production authority, required evidence, and any one unanswered question. A route is invalid if it creates a branch for a read-only query or lets an executor decide its own production authority.
