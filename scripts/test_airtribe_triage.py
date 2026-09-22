#!/usr/bin/env python3
"""Policy-table invariants for airtribe_triage."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import airtribe_triage as t


class TestPolicies(unittest.TestCase):
    def test_every_class_has_every_key(self):
        for name in t.POLICIES:
            self.assertEqual(list(t.policy(name)), t.KEYS, name)

    def test_only_code_routes_create_worktrees(self):
        worktree = {n for n in t.POLICIES if t.policy(n)["creates_worktree"]}
        self.assertEqual(worktree, {"code-change", "mixed-operations-and-code"})

    def test_no_class_may_execute_production_write(self):
        for name in t.POLICIES:
            self.assertFalse(t.policy(name)["may_execute_production_write"], name)

    def test_ambiguous_routes_to_clarify(self):
        self.assertEqual(t.policy("ambiguous")["next_owner"], "hermes-clarify")


if __name__ == "__main__":
    unittest.main()
