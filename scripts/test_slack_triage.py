#!/usr/bin/env python3
"""Routing invariants for slack_triage: what Jev and the thread link decide."""

import json
import sys
import unittest
from unittest import mock
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


    def test_repeat_sentry_issue_appends_to_its_task(self):
        i = item("create")
        i["sentryTask"] = "first"
        self.assertEqual((t.route(i), i["taskId"]), ("append", "first"))


class TestSentry(unittest.TestCase):
    def test_issue_id_from_attachment_link(self):
        m = {"text": "", "attachments": [{"fallback": "TypeError in upload", "title_link":
                                          "https://airtribe.sentry.io/issues/6123456/?referrer=slack"}]}
        self.assertEqual((t.sentry_issue(m), t.text_of(m)), ("6123456", "TypeError in upload"))

    def test_plain_message_has_no_issue(self):
        self.assertIsNone(t.sentry_issue({"text": "upload is broken"}))


class TestLink(unittest.TestCase):
    def test_links_to_the_filed_thread_message(self):
        i = item("unsure", thread=("1.0", "2.0"))
        t.link([i], [{"id": "task", "title": "x", "source": {"key": f"{TEAM_CH}:1.0"}}])
        self.assertEqual(i["linked"]["id"], "task")

    def test_other_channel_is_not_linked(self):
        i = item("unsure", thread=("1.0",))
        t.link([i], [{"id": "task", "title": "x", "source": {"key": "T1:C2:1.0"}}])
        self.assertNotIn("linked", i)


class TestMulti(unittest.TestCase):
    def run_multi(self, reply, n=4):
        """multi() over n calls against a fake Composio; reply(batch size, call number) -> response."""
        def rpc(token, session, method, params, rid=None):
            tools = params["arguments"]["tools"]
            results = [{"response": reply(len(tools), c["arguments"]["n"])} for c in tools]
            return {"result": {"content": [{"text": json.dumps({"data": {"results": results}})}]}}, session

        slack = t.Slack.__new__(t.Slack)
        slack.token = slack.session = None
        with mock.patch.object(t.slack_attach, "rpc", rpc), mock.patch.object(t.time, "sleep") as sleep:
            return slack.multi([("SLUG", {"n": k}) for k in range(n)]), sleep.call_count

    def test_elided_batch_is_split(self):
        out, _ = self.run_multi(lambda size, k: {"successful": True, "data": {"n": k} if size == 1 else None})
        self.assertEqual(out, [{"n": k} for k in range(4)])

    def test_rate_limited_batch_runs_one_call_at_a_time_and_waits(self):
        tried = set()

        def reply(size, k):
            if size == 1 and k in tried:
                return {"successful": True, "data": {"n": k}}
            if size == 1:
                tried.add(k)
            return {"successful": False, "error": '{"ok":false,"error":"ratelimited"}'}

        self.assertEqual(self.run_multi(reply), ([{"n": k} for k in range(4)], 4))

    def test_rate_limited_call_gives_up(self):
        with self.assertRaisesRegex(RuntimeError, "ratelimited"):
            self.run_multi(lambda size, k: {"successful": False, "error": "ratelimited"}, n=1)

    def test_other_errors_raise_at_once(self):
        with self.assertRaisesRegex(RuntimeError, "channel_not_found"):
            self.run_multi(lambda size, k: {"successful": False, "error": "channel_not_found"})


if __name__ == "__main__":
    unittest.main()
