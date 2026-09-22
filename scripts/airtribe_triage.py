#!/usr/bin/env python3
"""Policy contract for Airtribe route classes (see skills/airtribe-triage).

The policy is a fixed table, not a decision: Hermes picks the class, this
prints the hard boundary that class implies.
"""

import argparse
import json
import sys

# key order is the documented output contract
KEYS = [
    "operation",
    "summary",
    "creates_branch",
    "creates_worktree",
    "external_builder_allowed",
    "requires_pr",
    "production_read_allowed",
    "may_generate_production_write_command",
    "may_execute_production_write",
    "requires_human_production_execution",
    "next_owner",
    "required_evidence",
]

# ponytail: plain table; no rule engine until a class needs conditional policy.
POLICIES = {
    "codebase-query": {
        "summary": "Read-only explanation, location, history, or analysis of the codebase.",
        "creates_branch": False,
        "creates_worktree": False,
        "external_builder_allowed": False,
        "requires_pr": False,
        "production_read_allowed": False,
        "may_generate_production_write_command": False,
        "requires_human_production_execution": False,
        "next_owner": "hermes",
        "required_evidence": [
            "cited files, symbols, and commits supporting the answer",
            "confirmation that no branch, worktree, or PR was created",
        ],
    },
    "prod-read-lookup": {
        "summary": "Narrow verified production fact via the guarded read-only path.",
        "creates_branch": False,
        "creates_worktree": False,
        "external_builder_allowed": False,
        "requires_pr": False,
        "production_read_allowed": True,
        "may_generate_production_write_command": False,
        "requires_human_production_execution": False,
        "next_owner": "hermes",
        "required_evidence": [
            "entity/table and durable identifier resolved from committed code",
            "wrapper output confirming transaction_read_only and verified read-only role",
            "minimal redacted bounded result identifying exactly one target",
        ],
    },
    "prod-repl-proposal": {
        "summary": "Production data must change; the artifact is a human-run REPL runbook.",
        "creates_branch": False,
        "creates_worktree": False,
        "external_builder_allowed": False,
        "requires_pr": False,
        "production_read_allowed": True,
        "may_generate_production_write_command": True,
        "requires_human_production_execution": True,
        "next_owner": "human-operator",
        "required_evidence": [
            "redacted lookup proving exactly one target record and its pre-state",
            "proposed REPL command asserting durable UID, single row, and expected old value",
            "post-condition query, side-effect handling, and rollback note",
            "explicit statement that the human executes the production write",
        ],
    },
    "local-repl": {
        "summary": "Check or repair a confirmed local/disposable environment.",
        "creates_branch": False,
        "creates_worktree": False,
        "external_builder_allowed": False,
        "requires_pr": False,
        "production_read_allowed": False,
        "may_generate_production_write_command": False,
        "requires_human_production_execution": False,
        "next_owner": "hermes",
        "required_evidence": [
            "proof the target is local/disposable, not production",
            "commands run with their output",
            "cleanup result",
        ],
    },
    "code-change": {
        "summary": "Persistent product code, test, schema, or config must change.",
        "creates_branch": True,
        "creates_worktree": True,
        "external_builder_allowed": True,
        "requires_pr": True,
        "production_read_allowed": False,
        "may_generate_production_write_command": False,
        "requires_human_production_execution": False,
        "next_owner": "claude-code-worker",
        "required_evidence": [
            "branch, worktree, base SHA, and candidate SHA",
            "independent verifier gate output at the candidate SHA",
            "separate read-only review verdict bound to that SHA",
            "PR URL and CI head SHA; humans merge",
        ],
    },
    "mixed-operations-and-code": {
        "summary": "Immediate data correction plus a durable product fix, tracked separately.",
        "creates_branch": True,
        "creates_worktree": True,
        "external_builder_allowed": True,
        "requires_pr": True,
        "production_read_allowed": True,
        "may_generate_production_write_command": True,
        "requires_human_production_execution": True,
        "next_owner": "human-operator+claude-code-worker",
        "required_evidence": [
            "two linked task records: operations runbook and code change",
            "runbook evidence per prod-repl-proposal, not blocked on the PR",
            "code evidence per code-change; the PR must not perform the data correction",
        ],
    },
    "ambiguous": {
        "summary": "Environment, entity, authority, or artifact cannot be safely inferred.",
        "creates_branch": False,
        "creates_worktree": False,
        "external_builder_allowed": False,
        "requires_pr": False,
        "production_read_allowed": False,
        "may_generate_production_write_command": False,
        "requires_human_production_execution": False,
        "next_owner": "hermes-clarify",
        "required_evidence": [
            "the facts that could not be inferred (environment, entity, authority, artifact, timezone)",
            "one precise clarifying question",
        ],
    },
}


def policy(operation):
    body = POLICIES[operation]
    full = {"operation": operation, "may_execute_production_write": False, **body}
    return {key: full[key] for key in KEYS}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="command", required=True)
    classify = sub.add_parser("classify", help="Print the policy JSON for one route class.")
    classify.add_argument("--operation", required=True, choices=sorted(POLICIES))
    sub.add_parser("list", help="Print the route class names.")

    args = ap.parse_args()
    if args.command == "list":
        print("\n".join(sorted(POLICIES)))
    else:
        print(json.dumps(policy(args.operation), indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
