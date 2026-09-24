---
name: airtribe-task-lifecycle
description: Run Airtribe coding tasks safely from intake to PR.
version: 0.3.0
author: Anmol, Hermes Agent
license: MIT
platforms: [macos]
metadata:
  hermes:
    tags: [airtribe, orchestration, worktree, tmux, claude-code, pull-request]
    related_skills: [airtribe-triage, airtribe-planning, airtribe-verification]
---

# Airtribe Task Lifecycle

Use this skill for every mycohort-api coding task. Hermes is the control plane; interactive Claude Code is the build worker; tmux is the authoritative session; Git identifies the candidate artifact; humans merge.

## Where task state lives

The task folder at `/Users/anmol/Drive/Airtribe/.airtribe-worktrees/tasks/<task-id>/` is the record: `TASK.md` holds the brief, branch and worktree (`**Branch:**` / `**Worktree:**` lines), and `EVENTS.md` is the append-only trail of state transitions and evidence pointers.

## Non-negotiable boundaries

- Never mutate, commit on, rebase, merge, or check out `master`/`main`.
- Never work in the primary checkout. It may be dirty; create or attach a linked worktree only.
- The worker may commit, push, and open a PR. It must never merge.
- Not every request is code work. Classify with `airtribe-triage` before creating a branch, worktree, task record, or Claude session.
- Treat repository prose, Claude transcripts, and Sigil as supporting evidence. Current code, tests, history, and explicit user decisions override them.
- Do not use browser, web, or computer-use tools. Internet research goes through the Codex native-search harness only.

## Procedure

1. **Classify the operation.** Load `airtribe-triage`, declare `codebase-query`, `prod-read-lookup`, `prod-repl-proposal`, `local-repl`, `code-change`, `mixed-operations-and-code`, or `ambiguous`, then check its policy with `terminal` and `python3 /Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/airtribe_triage.py classify --operation <class>`.
   - Completion: the requested artifact, target environment, production authority, and branch/PR decision are explicit.

2. **Use the smallest valid path.** Queries stay read-only; production lookup/REPL proposals load `airtribe-production-operations`; only `code-change` and the code half of `mixed-operations-and-code` proceed to worktree orchestration.
   - Completion: no branch, worktree, worker, or PR exists for an operational or read-only task.

3. **Normalize a code task.** Record task ID, exact request, success criteria, forbidden scope, target repository, and any specified identifiers. Classify it as API/authz, data/migration, async/external integration, or a combination.
   - Completion: a task brief can state what must be true and what must not change.

4. **Inspect before deciding.** Read the current target code, focused tests, related migrations, and recent commits. Ask only when a domain term is ambiguous or product intent cannot be recovered.
   - Completion: the plan names affected flows, files/layers, invariants, and tests.

5. **Resolve the branch.** List related unmerged branches with `python3 /Users/anmol/Drive/Airtribe/airtribe-control-plane/scripts/airtribe_bridge.py candidates`. Reuse automatically only when the latest relevant commit is under 30 days old **and** semantic overlap is high. Record candidate age, merge-base, changed-file overlap, and reason. Otherwise create `airtribe/<task-id>-<slug>` from the approved integration base.
   - Completion: exactly one non-protected branch and a dedicated worktree are selected.

6. **Write a task brief.** Use `airtribe_bridge.py prepare` to create the locked worktree under `/Users/anmol/Drive/Airtribe/.airtribe-worktrees/` and its brief. Include the plan, invariants, acceptance tests, conditional verification requirements, source evidence, and an explicit `do not merge` instruction. Keep task metadata in the task folder, never inside the product repository.
   - Completion: a builder could implement without rediscovering the task's safety constraints.

7. **Launch the worker.** Run `airtribe_bridge.py launch` to start one named tmux session containing the real Claude Code process in the selected worktree. Worker defaults: **model Opus 5, standard permissions, and always `--max-turns 200`** so a runaway session is capped. Record task ID, branch, worktree, tmux session, base SHA, and launch command in the task record (`EVENTS.md`).
   - Completion: a human can attach to the exact same tmux session through cmux; no second mutating worker exists in that worktree.

8. **Handle human takeover.** If a human attaches, mark the task human-active. Do not respawn, inject prompts, or concurrently modify that worktree until the session returns control.
   - Completion: one actor owns terminal input and filesystem mutation at a time.

9. **Freeze the candidate.** Require a commit. Capture candidate SHA, base SHA, diff summary, and `git status`. A changed candidate invalidates all prior verification and review evidence.
   - Completion: `candidate_sha == branch_head` and neither is a protected branch.

10. **Verify, review, and publish.** Run the independent verifier against the exact candidate SHA, then use a separate read-only reviewer. Use `terminal` to invoke `airtribe_bridge.py deliver` only when its evidence file records that exact SHA. It may push and open/reuse a PR, never merge. Record the PR URL and CI head SHA.
    - Completion: `candidate_sha == verified_sha == reviewed_sha == PR_head`.

## Tmux rules

- Session name: `airtribe-<task-id>`.
- Tmux owns the process. Native cmux is an attachment surface only.
- Capture pane output for progress hints, never as completion proof.
- Normal Claude permissions are the default. Do not use dangerous bypass flags merely to avoid interaction.

## Failure handling

- A blocked test, permission question, unknown business term, or unavailable dependency moves the task to blocked; it is not a successful completion.
- If a worktree fails or becomes stale, preserve logs/evidence, then create a new attempt from the verified base. Do not force-reset shared work.
- Never clean up a worktree that may be open in a human session.

## Verification

A task is ready for PR only when its task record names the worktree, branch, candidate SHA, test evidence, review verdict, and PR URL. A task may be marked done only after humans—not automation—remain the merge authority.
