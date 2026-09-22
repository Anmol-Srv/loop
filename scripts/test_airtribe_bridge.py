#!/usr/bin/env python3
"""Smallest useful checks for airtribe_bridge: branch listing, worktree
creation, and the two refusals that matter (protected branch, uncommitted
candidate). tmux/claude/gh subcommands are not exercised."""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

BRIDGE = Path(__file__).resolve().parent / "airtribe_bridge.py"
GIT_ENV = {
    **os.environ,
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull,
}


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, env=GIT_ENV,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def bridge(*args):
    proc = subprocess.run(["python3", str(BRIDGE), *args], capture_output=True, text=True, env=GIT_ENV)
    return proc.returncode, json.loads(proc.stdout or "{}")


def make_repo(root):
    repo = Path(root) / "repo"
    repo.mkdir()
    git(repo, "init", "-b", "master")
    (repo / "a.txt").write_text("one\n")
    git(repo, "add", "-A")
    git(repo, "commit", "-m", "base")
    return repo


class TestBridge(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = make_repo(self.tmp.name)
        self.root = Path(self.tmp.name) / "worktrees"

    def test_candidates_lists_non_protected_branch_only(self):
        git(self.repo, "branch", "airtribe/t1-widget")
        code, out = bridge("candidates", "--repo", str(self.repo), "--task", "widget")
        self.assertEqual(code, 0)
        names = [c["branch"] for c in out["candidates"]]
        self.assertIn("airtribe/t1-widget", names)
        self.assertNotIn("master", names)

    def test_prepare_refuses_protected_branch(self):
        code, out = bridge("prepare", "--repo", str(self.repo), "--task-id", "t1",
                           "--task", "x", "--branch", "master",
                           "--worktrees-root", str(self.root))
        self.assertEqual(code, 2)
        self.assertIn("master", out["error"])
        self.assertFalse(self.root.exists())

    def test_prepare_creates_worktree(self):
        code, out = bridge("prepare", "--repo", str(self.repo), "--task-id", "t1",
                           "--task", "add widget", "--branch", "airtribe/t1-widget",
                           "--worktrees-root", str(self.root))
        self.assertEqual(code, 0, out)
        self.assertTrue(Path(out["worktree"], "a.txt").exists())
        self.assertTrue(Path(out["brief"]).exists())

    def test_verify_refuses_when_candidate_is_not_the_committed_branch_head(self):
        _, prepared = bridge("prepare", "--repo", str(self.repo), "--task-id", "t1",
                             "--task", "add widget", "--branch", "airtribe/t1-widget",
                             "--worktrees-root", str(self.root))
        Path(prepared["worktree"], "a.txt").write_text("uncommitted\n")
        code, out = bridge("verify", "--repo", str(self.repo), "--task-id", "t1",
                           "--worktree", prepared["worktree"],
                           "--artifacts-root", str(self.root / "artifacts"),
                           "--gates-json", "[]")
        self.assertEqual(code, 2)
        self.assertIn("dirty", out["error"])


if __name__ == "__main__":
    unittest.main()
