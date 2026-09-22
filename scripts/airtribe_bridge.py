#!/usr/bin/env python3
"""Local control-plane primitives for mycohort-api coding tasks.

This script deliberately does not fetch, merge, rebase, or interact with
database/provider credentials. It creates isolated worktrees, starts the real
interactive Claude process in tmux, independently verifies an exact committed
candidate in a fresh detached worktree, and can push that verified candidate
and create or reuse its PR. It has no merge operation.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

PROTECTED_BRANCHES = {"master", "main"}
TASK_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$")
MAX_OUTPUT_CHARS = 30_000


class BridgeError(RuntimeError):
    """An expected policy or environment failure."""


def run(
    argv: list[str],
    *,
    cwd: Path | None = None,
    check: bool = True,
    env: dict[str, str] | None = None,
    timeout: int | None = None,
) -> subprocess.CompletedProcess[str]:
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    result = subprocess.run(
        argv,
        cwd=str(cwd) if cwd else None,
        env=merged_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    if check and result.returncode:
        command = shlex.join(argv)
        raise BridgeError(f"Command failed ({result.returncode}): {command}\n{result.stdout}")
    return result


def git(repo: Path, *args: str, check: bool = True) -> str:
    return run(["git", "-C", str(repo), *args], check=check).stdout.strip()


def canonical_repo(path: str) -> Path:
    candidate = Path(path).expanduser().resolve()
    try:
        root = git(candidate, "rev-parse", "--show-toplevel")
    except BridgeError as exc:
        raise BridgeError(f"Not a Git repository: {candidate}") from exc
    return Path(root).resolve()


def common_git_dir(path: Path) -> Path:
    """Return the shared Git directory for a primary or linked worktree."""
    raw = git(path, "rev-parse", "--git-common-dir")
    candidate = Path(raw)
    if not candidate.is_absolute():
        candidate = path / candidate
    return candidate.resolve()


def belongs_to_repo(repo: Path, worktree: Path) -> bool:
    """Compare Git common directories, not worktree-specific top-level paths."""
    try:
        return common_git_dir(repo) == common_git_dir(worktree)
    except BridgeError:
        return False


def require_task_id(value: str) -> str:
    if not TASK_ID_RE.fullmatch(value):
        raise BridgeError("task-id must be 1-80 characters: letters, digits, '.', '_' or '-'")
    return value


def require_non_protected_branch(branch: str) -> str:
    if branch in PROTECTED_BRANCHES:
        raise BridgeError(f"Protected branch refused: {branch}")
    return branch


def branch_exists(repo: Path, branch: str) -> bool:
    return run(["git", "-C", str(repo), "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"], check=False).returncode == 0


def branch_head(repo: Path, branch: str) -> str:
    return git(repo, "rev-parse", branch)


def branch_age_days(repo: Path, branch: str) -> float:
    timestamp = int(git(repo, "log", "-1", "--format=%ct", branch))
    return max(0.0, (dt.datetime.now(dt.timezone.utc).timestamp() - timestamp) / 86400)


def worktree_branches(repo: Path) -> dict[str, Path]:
    entries: dict[str, Path] = {}
    current_path: Path | None = None
    for line in git(repo, "worktree", "list", "--porcelain").splitlines():
        if line.startswith("worktree "):
            current_path = Path(line.split(" ", 1)[1]).resolve()
        elif line.startswith("branch refs/heads/") and current_path:
            entries[line.removeprefix("branch refs/heads/")] = current_path
    return entries


def default_workspace_root(repo: Path) -> Path:
    return repo.parent / ".airtribe-worktrees"


def task_slug(task_id: str, branch: str) -> str:
    clean_branch = re.sub(r"[^A-Za-z0-9._-]+", "-", branch.replace("/", "-"))
    return f"{task_id}-{clean_branch}"[:120]


def json_output(payload: dict[str, Any], exit_code: int = 0) -> None:
    print(json.dumps(payload, indent=2, sort_keys=True, default=str))
    raise SystemExit(exit_code)


def get_changed_files(repo: Path, base: str, target: str) -> list[str]:
    result = run(["git", "-C", str(repo), "diff", "--name-only", f"{base}...{target}"], check=False)
    return [line for line in result.stdout.splitlines() if line]


def command_record(
    argv: list[str], cwd: Path, env: dict[str, str] | None, timeout: int
) -> dict[str, Any]:
    started = dt.datetime.now(dt.timezone.utc)
    try:
        completed = run(argv, cwd=cwd, check=False, env=env, timeout=timeout)
        timed_out = False
        output = completed.stdout
        exit_code = completed.returncode
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        raw = exc.stdout or ""
        output = raw.decode() if isinstance(raw, bytes) else raw
        exit_code = None
    ended = dt.datetime.now(dt.timezone.utc)
    if len(output) > MAX_OUTPUT_CHARS:
        output = output[:MAX_OUTPUT_CHARS] + "\n[truncated by control plane]"
    return {
        "argv": argv,
        "cwd": str(cwd),
        "environment_names": sorted((env or {}).keys()),
        "started_at": started.isoformat(),
        "ended_at": ended.isoformat(),
        "elapsed_seconds": round((ended - started).total_seconds(), 3),
        "exit_code": exit_code,
        "timed_out": timed_out,
        "output": output,
        "passed": exit_code == 0 and not timed_out,
    }


def cmd_candidates(args: argparse.Namespace) -> None:
    repo = canonical_repo(args.repo)
    base = args.base
    git(repo, "rev-parse", "--verify", base)
    now = dt.datetime.now(dt.timezone.utc)
    rows: list[dict[str, Any]] = []
    fmt = "%(refname:short)\x1f%(committerdate:unix)\x1f%(subject)"  # git for-each-ref has no %xNN
    raw = git(repo, "for-each-ref", "--sort=-committerdate", f"--format={fmt}", "refs/heads")
    task_terms = {term.lower() for term in re.findall(r"[A-Za-z][A-Za-z0-9_-]{2,}", args.task)}
    checked_out = worktree_branches(repo)
    for line in raw.splitlines():
        try:
            branch, stamp, subject = line.split("\x1f", 2)
        except ValueError:
            continue
        if branch in PROTECTED_BRANCHES:
            continue
        age = max(0.0, (now.timestamp() - int(stamp)) / 86400)
        is_merged = run(["git", "-C", str(repo), "merge-base", "--is-ancestor", branch, base], check=False).returncode == 0
        changed_files = get_changed_files(repo, base, branch)
        searchable = " ".join([branch, subject, *changed_files]).lower()
        matched_terms = sorted(term for term in task_terms if term in searchable)
        # This is a transparent lexical signal. Hermes must perform semantic review before
        # declaring overlap high; the script records evidence rather than pretending to reason.
        overlap_score = len(matched_terms)
        rows.append(
            {
                "branch": branch,
                "head": branch_head(repo, branch),
                "subject": subject,
                "age_days": round(age, 2),
                "merged_into_base": is_merged,
                "checked_out_at": str(checked_out[branch]) if branch in checked_out else None,
                "changed_files": changed_files,
                "matched_terms": matched_terms,
                "lexical_overlap_score": overlap_score,
                "eligible_by_age": age <= args.max_age_days,
            }
        )
    json_output({"repo": str(repo), "base": base, "max_age_days": args.max_age_days, "candidates": rows})


def write_brief(
    brief_path: Path,
    *,
    task_id: str,
    task: str,
    branch: str,
    base_sha: str,
    mode: str,
    acceptance: list[str],
    forbidden: list[str],
) -> None:
    brief_path.parent.mkdir(parents=True, exist_ok=True)
    rendered = [
        f"# Airtribe task {task_id}",
        "",
        "## Request",
        task,
        "",
        "## Execution contract",
        f"- Branch: `{branch}` ({mode})",
        f"- Base SHA: `{base_sha}`",
        "- Do not merge, rebase, or commit on master/main.",
        "- Read current code and focused tests before editing.",
        "- Commit the candidate before claiming implementation is complete.",
        "- Report tests run and tests still required; do not call them independently verified.",
        "",
        "## Acceptance criteria",
    ]
    rendered.extend(f"- {item}" for item in acceptance or ["Fulfil the request without violating existing contracts."])
    rendered.extend(["", "## Forbidden scope"])
    rendered.extend(f"- {item}" for item in forbidden or ["Unrelated refactors and protected-branch mutations."])
    rendered.extend(["", "## Handoff", "Leave a committed candidate and a concise implementation summary for independent verification.", ""])
    brief_path.write_text("\n".join(rendered), encoding="utf-8")


def cmd_prepare(args: argparse.Namespace) -> None:
    repo = canonical_repo(args.repo)
    task_id = require_task_id(args.task_id)
    branch = require_non_protected_branch(args.branch)
    git(repo, "check-ref-format", "--branch", branch)
    base = args.base
    base_sha = git(repo, "rev-parse", "--verify", base)
    root = Path(args.worktrees_root).expanduser().resolve() if args.worktrees_root else default_workspace_root(repo)
    target = root / task_slug(task_id, branch)
    brief_path = root / "briefs" / f"{task_id}.md"
    if target.exists():
        raise BridgeError(f"Refusing to reuse existing task worktree path: {target}")

    branches = worktree_branches(repo)
    if args.reuse:
        if not branch_exists(repo, branch):
            raise BridgeError(f"Cannot reuse missing branch: {branch}")
        if branch in branches:
            raise BridgeError(f"Cannot reuse {branch}; it is already checked out at {branches[branch]}")
        age = branch_age_days(repo, branch)
        if age > args.max_age_days:
            raise BridgeError(f"Cannot reuse stale branch {branch}: {age:.1f} days old")
        mode = "reused"
        source = branch
    else:
        if branch_exists(repo, branch):
            raise BridgeError(f"Branch already exists; use --reuse only after overlap/age review: {branch}")
        mode = "new"
        source = base

    plan = {
        "repo": str(repo),
        "task_id": task_id,
        "branch": branch,
        "base": base,
        "base_sha": base_sha,
        "mode": mode,
        "worktree": str(target),
        "brief": str(brief_path),
    }
    if args.dry_run:
        json_output({"dry_run": True, **plan})

    root.mkdir(parents=True, exist_ok=True)
    if args.reuse:
        run(["git", "-C", str(repo), "worktree", "add", str(target), source])
    else:
        run(["git", "-C", str(repo), "worktree", "add", "-b", branch, str(target), source])
    run([
        "git",
        "-C",
        str(repo),
        "worktree",
        "lock",
        "--reason",
        f"Airtribe task {task_id}",
        str(target),
    ])
    write_brief(
        brief_path,
        task_id=task_id,
        task=args.task,
        branch=branch,
        base_sha=base_sha,
        mode=mode,
        acceptance=args.acceptance,
        forbidden=args.forbidden,
    )
    json_output({"prepared": True, **plan})


def session_name(task_id: str) -> str:
    return f"airtribe-{require_task_id(task_id)}"


def cmd_launch(args: argparse.Namespace) -> None:
    repo = canonical_repo(args.repo)
    task_id = require_task_id(args.task_id)
    worktree = Path(args.worktree).expanduser().resolve()
    if worktree == repo:
        raise BridgeError("Refusing to launch a worker in the primary repository checkout")
    if not worktree.is_dir() or not belongs_to_repo(repo, worktree):
        raise BridgeError(f"Worktree does not belong to repository: {worktree}")
    branch = git(worktree, "branch", "--show-current")
    require_non_protected_branch(branch)
    brief = Path(args.brief).expanduser().resolve()
    if not brief.is_file():
        raise BridgeError(f"Task brief not found: {brief}")
    name = session_name(task_id)
    if run(["tmux", "has-session", "-t", name], check=False).returncode == 0:
        raise BridgeError(f"tmux session already exists: {name}")

    prompt = (
        f"You are the builder for Airtribe task {task_id}. Read the task brief at {brief}. "
        "Inspect current code and focused tests before editing. Keep the task in this worktree. "
        "Do not merge, rebase, or modify master/main. Commit the candidate when ready and report "
        "what remains for independent verification."
    )
    claude_argv = [
        "env",
        "CLAUDE_CODE_NO_FLICKER=1",
        "claude",
        "--permission-mode",
        "default",
        "--add-dir",
        str(brief.parent),
        prompt,
    ]
    command = shlex.join(claude_argv)
    run(["tmux", "new-session", "-d", "-s", name, "-x", str(args.width), "-y", str(args.height), "-c", str(worktree), command])
    json_output(
        {
            "launched": True,
            "task_id": task_id,
            "tmux_session": name,
            "worktree": str(worktree),
            "branch": branch,
            "brief": str(brief),
            "attach_command": f"tmux attach -t {name}",
        }
    )


def cmd_status(args: argparse.Namespace) -> None:
    repo = canonical_repo(args.repo)
    task_id = require_task_id(args.task_id)
    worktree = Path(args.worktree).expanduser().resolve()
    if not belongs_to_repo(repo, worktree):
        raise BridgeError(f"Worktree does not belong to repository: {worktree}")
    name = session_name(task_id)
    active = run(["tmux", "has-session", "-t", name], check=False).returncode == 0
    pane = ""
    if active:
        pane = run(["tmux", "capture-pane", "-t", name, "-p", "-S", str(-abs(args.lines))], check=False).stdout
    json_output(
        {
            "task_id": task_id,
            "tmux_session": name,
            "active": active,
            "worktree": str(worktree),
            "branch": git(worktree, "branch", "--show-current"),
            "head": git(worktree, "rev-parse", "HEAD"),
            "status": git(worktree, "status", "--short"),
            "pane_tail": pane[-MAX_OUTPUT_CHARS:],
        }
    )


def load_gates(path: Path) -> list[dict[str, Any]]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise BridgeError(f"Cannot load gate config {path}: {exc}") from exc
    gates = payload.get("default_gates")
    if not isinstance(gates, list) or not gates:
        raise BridgeError("Gate config must contain a non-empty default_gates list")
    for gate in gates:
        if not isinstance(gate, dict) or not isinstance(gate.get("argv"), list):
            raise BridgeError("Every gate requires an argv list")
    return gates


def safe_version(argv: list[str], cwd: Path) -> str:
    result = run(argv, cwd=cwd, check=False)
    return result.stdout.strip()[:1000]


def cmd_verify(args: argparse.Namespace) -> None:
    repo = canonical_repo(args.repo)
    task_id = require_task_id(args.task_id)
    builder_worktree = Path(args.worktree).expanduser().resolve()
    if builder_worktree == repo:
        raise BridgeError("Refusing to verify the primary repository checkout")
    if not belongs_to_repo(repo, builder_worktree):
        raise BridgeError(f"Worktree does not belong to repository: {builder_worktree}")
    branch = git(builder_worktree, "branch", "--show-current")
    require_non_protected_branch(branch)
    dirty = git(builder_worktree, "status", "--porcelain")
    if dirty:
        raise BridgeError("Candidate worktree is dirty; commit or discard changes before verification")
    candidate_sha = git(builder_worktree, "rev-parse", "HEAD")
    base_sha = git(repo, "merge-base", args.base, candidate_sha)
    if args.gates_json:
        try:
            gates = json.loads(args.gates_json)
        except json.JSONDecodeError as exc:
            raise BridgeError(f"--gates-json must be JSON: {exc}") from exc
        if not isinstance(gates, list) or not all(isinstance(gate, dict) and isinstance(gate.get("argv"), list) for gate in gates):
            raise BridgeError("--gates-json must be a list of objects containing argv lists")
    else:
        gates_path = Path(args.gates).expanduser().resolve() if args.gates else Path(__file__).resolve().parents[1] / "config" / "gates.json"
        gates = load_gates(gates_path)

    artifacts_root = Path(args.artifacts_root).expanduser().resolve() if args.artifacts_root else default_workspace_root(repo) / "artifacts"
    artifacts_root.mkdir(parents=True, exist_ok=True)
    verifier = Path(tempfile.mkdtemp(prefix=f"verify-{task_id}-", dir=str(artifacts_root)))
    evidence_path = artifacts_root / f"{task_id}-{candidate_sha[:12]}.json"
    evidence: dict[str, Any] = {
        "schema_version": 1,
        "task_id": task_id,
        "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "repo": str(repo),
        "builder_worktree": str(builder_worktree),
        "branch": branch,
        "base_ref": args.base,
        "base_sha": base_sha,
        "candidate_sha": candidate_sha,
        "verified_sha": candidate_sha,
        "verifier_worktree": str(verifier),
        "git_status_before": dirty,
        "changed_files": get_changed_files(repo, base_sha, candidate_sha),
        "diff_check": None,
        "toolchain": {},
        "gates": [],
        "success": False,
    }
    try:
        run(["git", "-C", str(repo), "worktree", "add", "--detach", str(verifier), candidate_sha])
        diff_check = command_record(["git", "diff", "--check", f"{base_sha}...{candidate_sha}"], verifier, None, args.timeout)
        evidence["diff_check"] = diff_check
        evidence["toolchain"] = {
            "node": safe_version(["node", "--version"], verifier),
            "pnpm": safe_version(["pnpm", "--version"], verifier),
            "git": safe_version(["git", "--version"], verifier),
        }
        if not diff_check["passed"]:
            evidence["gates"] = []
        else:
            for gate in gates:
                record = command_record(
                    [str(value) for value in gate["argv"]],
                    verifier,
                    {str(k): str(v) for k, v in gate.get("env", {}).items()},
                    args.timeout,
                )
                record["name"] = gate.get("name", shlex.join(record["argv"]))
                evidence["gates"].append(record)
                if not record["passed"]:
                    break
        evidence["success"] = bool(evidence["diff_check"]["passed"]) and all(gate["passed"] for gate in evidence["gates"]) and len(evidence["gates"]) == len(gates)
    finally:
        evidence_path.write_text(json.dumps(evidence, indent=2, sort_keys=True), encoding="utf-8")
        run(["git", "-C", str(repo), "worktree", "remove", "--force", str(verifier)], check=False)
        if verifier.exists():
            shutil.rmtree(verifier, ignore_errors=True)
    json_output({"success": evidence["success"], "evidence": str(evidence_path), "candidate_sha": candidate_sha}, 0 if evidence["success"] else 1)


def load_verified_evidence(path: Path, candidate_sha: str, branch: str) -> dict[str, Any]:
    try:
        evidence = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise BridgeError(f"Cannot load verification evidence {path}: {exc}") from exc
    if evidence.get("success") is not True:
        raise BridgeError("Verification evidence does not record success")
    if evidence.get("candidate_sha") != candidate_sha or evidence.get("verified_sha") != candidate_sha:
        raise BridgeError("Verification evidence SHA does not match the current candidate")
    if evidence.get("branch") != branch:
        raise BridgeError("Verification evidence branch does not match the current candidate")
    return evidence


def cmd_deliver(args: argparse.Namespace) -> None:
    """Push a verified candidate and create or reuse a non-merge PR."""
    repo = canonical_repo(args.repo)
    worktree = Path(args.worktree).expanduser().resolve()
    if worktree == repo:
        raise BridgeError("Refusing to deliver from the primary repository checkout")
    if not belongs_to_repo(repo, worktree):
        raise BridgeError(f"Worktree does not belong to repository: {worktree}")
    branch = git(worktree, "branch", "--show-current")
    require_non_protected_branch(branch)
    if git(worktree, "status", "--porcelain"):
        raise BridgeError("Candidate worktree is dirty; commit or discard changes before delivery")
    candidate_sha = git(worktree, "rev-parse", "HEAD")
    base = args.base
    if base not in PROTECTED_BRANCHES:
        raise BridgeError(f"PR base must be a protected integration branch, got: {base}")
    evidence_path = Path(args.evidence).expanduser().resolve()
    load_verified_evidence(evidence_path, candidate_sha, branch)
    if not args.body and not args.body_file:
        raise BridgeError("Delivery requires --body or --body-file so gh never opens an interactive editor")

    push_command = ["git", "-C", str(worktree), "push", "-u", args.remote, f"HEAD:refs/heads/{branch}"]
    existing_command = [
        "gh", "pr", "list", "--head", branch, "--base", base, "--state", "open", "--limit", "1", "--json", "url",
    ]
    create_command = ["gh", "pr", "create", "--base", base, "--head", branch, "--title", args.title]
    if args.body_file:
        create_command.extend(["--body-file", str(Path(args.body_file).expanduser().resolve())])
    else:
        create_command.extend(["--body", args.body])

    plan = {
        "branch": branch,
        "base": base,
        "candidate_sha": candidate_sha,
        "evidence": str(evidence_path),
        "push_command": push_command,
        "existing_pr_command": existing_command,
        "create_pr_command": create_command,
        "merge_supported": False,
    }
    if args.dry_run:
        json_output({"dry_run": True, **plan})

    run(push_command, cwd=worktree)
    existing = run(existing_command, cwd=worktree).stdout
    try:
        existing_prs = json.loads(existing)
    except json.JSONDecodeError as exc:
        raise BridgeError(f"Could not parse gh pr list output: {exc}") from exc
    if isinstance(existing_prs, list) and existing_prs:
        pr_url = existing_prs[0].get("url")
        action = "reused_existing_pr"
    else:
        pr_url = run(create_command, cwd=worktree).stdout.strip()
        action = "created_pr"
    json_output({"published": True, "action": action, "pr_url": pr_url, **plan})


def cmd_cleanup(args: argparse.Namespace) -> None:
    repo = canonical_repo(args.repo)
    worktree = Path(args.worktree).expanduser().resolve()
    workspace_root = Path(args.worktrees_root).expanduser().resolve() if args.worktrees_root else default_workspace_root(repo)
    if worktree == repo or workspace_root not in worktree.parents:
        raise BridgeError(f"Refusing to remove a worktree outside {workspace_root}: {worktree}")
    if not worktree.exists():
        raise BridgeError(f"Worktree does not exist: {worktree}")
    branch = git(worktree, "branch", "--show-current")
    require_non_protected_branch(branch)
    run(["git", "-C", str(repo), "worktree", "unlock", str(worktree)], check=False)
    command = ["git", "-C", str(repo), "worktree", "remove"]
    if args.force:
        command.append("--force")
    command.append(str(worktree))
    run(command)
    json_output({"removed": True, "worktree": str(worktree), "branch": branch})


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    sub = result.add_subparsers(dest="command", required=True)

    candidates = sub.add_parser("candidates", help="List branch-reuse evidence; Hermes evaluates semantic overlap.")
    candidates.add_argument("--repo", required=True)
    candidates.add_argument("--task", required=True)
    candidates.add_argument("--base", default="master")
    candidates.add_argument("--max-age-days", type=float, default=30.0)
    candidates.set_defaults(func=cmd_candidates)

    prepare = sub.add_parser("prepare", help="Create a protected isolated worktree and task brief.")
    prepare.add_argument("--repo", required=True)
    prepare.add_argument("--task-id", required=True)
    prepare.add_argument("--task", required=True)
    prepare.add_argument("--branch", required=True)
    prepare.add_argument("--base", default="master")
    prepare.add_argument("--reuse", action="store_true")
    prepare.add_argument("--max-age-days", type=float, default=30.0)
    prepare.add_argument("--worktrees-root")
    prepare.add_argument("--acceptance", action="append", default=[])
    prepare.add_argument("--forbidden", action="append", default=[])
    prepare.add_argument("--dry-run", action="store_true")
    prepare.set_defaults(func=cmd_prepare)

    launch = sub.add_parser("launch", help="Start interactive Claude Code in a named tmux session.")
    launch.add_argument("--repo", required=True)
    launch.add_argument("--task-id", required=True)
    launch.add_argument("--worktree", required=True)
    launch.add_argument("--brief", required=True)
    launch.add_argument("--width", type=int, default=160)
    launch.add_argument("--height", type=int, default=48)
    launch.set_defaults(func=cmd_launch)

    status = sub.add_parser("status", help="Report tmux, branch, and worktree status without mutation.")
    status.add_argument("--repo", required=True)
    status.add_argument("--task-id", required=True)
    status.add_argument("--worktree", required=True)
    status.add_argument("--lines", type=int, default=80)
    status.set_defaults(func=cmd_status)

    verify = sub.add_parser("verify", help="Verify a committed candidate in a fresh detached worktree.")
    verify.add_argument("--repo", required=True)
    verify.add_argument("--task-id", required=True)
    verify.add_argument("--worktree", required=True)
    verify.add_argument("--base", default="master")
    verify.add_argument("--gates", help="Path to a gate JSON config.")
    verify.add_argument("--gates-json", help="Inline JSON list of {name, argv, env?} gates; intended for tests.")
    verify.add_argument("--artifacts-root")
    verify.add_argument("--timeout", type=int, default=600)
    verify.set_defaults(func=cmd_verify)

    deliver = sub.add_parser("deliver", help="Push a verified candidate and create or reuse its PR; never merges.")
    deliver.add_argument("--repo", required=True)
    deliver.add_argument("--worktree", required=True)
    deliver.add_argument("--evidence", required=True)
    deliver.add_argument("--title", required=True)
    body = deliver.add_mutually_exclusive_group(required=True)
    body.add_argument("--body")
    body.add_argument("--body-file")
    deliver.add_argument("--base", default="master")
    deliver.add_argument("--remote", default="origin")
    deliver.add_argument("--dry-run", action="store_true")
    deliver.set_defaults(func=cmd_deliver)

    cleanup = sub.add_parser("cleanup", help="Remove an Airtribe-owned non-protected worktree.")
    cleanup.add_argument("--repo", required=True)
    cleanup.add_argument("--worktree", required=True)
    cleanup.add_argument("--worktrees-root")
    cleanup.add_argument("--force", action="store_true")
    cleanup.set_defaults(func=cmd_cleanup)
    return result


def main() -> None:
    args = parser().parse_args()
    try:
        args.func(args)
    except BridgeError as exc:
        json_output({"success": False, "error": str(exc)}, 2)
    except FileNotFoundError as exc:
        json_output({"success": False, "error": f"Required executable not found: {exc.filename}"}, 2)


if __name__ == "__main__":
    main()