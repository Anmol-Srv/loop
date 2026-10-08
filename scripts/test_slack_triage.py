#!/usr/bin/env python3
"""Routing invariants for slack_triage: what Jev and the thread link decide."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import slack_triage as t

TEAM_CH = "T1:C1"


def item(decision, task=None, linked=None, thread=()):
    i = {"key": f"{TEAM_CH}:3.0", "jev": {"decision": decision, "taskId": task},
         "thread": [{"ts": ts} for ts in thread]}
    if linked:
        i["linked"] = {"id": linked}
    return i


class TestRoute(unittest.TestCase):
    def test_reply_in_filed_thread_appends_to_it(self):
        i = item("unsure", linked="parent")
        self.assertEqual((t.route(i), i["taskId"]), ("append", "parent"))

    def test_jev_target_beats_the_thread_link(self):
        # a forwarded DM thread can carry a report about another filed task
        i = item("update", task="other", linked="parent")
        self.assertEqual((t.route(i), i["taskId"]), ("append", "other"))

    def test_new_issue_in_filed_thread_goes_to_the_model(self):
        self.assertEqual(t.route(item("create", linked="parent")), "write")

    def test_unlinked(self):
        self.assertEqual(t.route(item("skip")), "skip")
        self.assertEqual(t.route(item("create")), "write")
        self.assertEqual(t.route(item("unsure")), "write")
        self.assertEqual(t.route(item("update")), "write")  # update without a task: the model picks
        i = item("update", task="x")
        self.assertEqual((t.route(i), i["taskId"]), ("append", "x"))


class TestLink(unittest.TestCase):
    def test_links_to_the_filed_thread_message(self):
        i = item("unsure", thread=("1.0", "2.0"))
        t.link([i], [{"id": "task", "title": "x", "source": {"key": f"{TEAM_CH}:1.0"}}])
        self.assertEqual(i["linked"]["id"], "task")

    def test_other_channel_is_not_linked(self):
        i = item("unsure", thread=("1.0",))
        t.link([i], [{"id": "task", "title": "x", "source": {"key": "T1:C2:1.0"}}])
        self.assertNotIn("linked", i)


if __name__ == "__main__":
    unittest.main()
