#!/usr/bin/env python3
"""Unit tests for react.py. The two seams — api() and sh() — are replaced wholesale, so no
HTTP, no git, no tmux and no Claude process is ever touched. Run: python3 -m unittest -v
test_react (or `python3 test_react.py`)."""
import json, os, subprocess, sys, tempfile, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import react  # noqa: E402

TASK = {"id": "11111111-1111-1111-1111-111111111111", "title": "migrate the report",
        "assigneeTokenId": "tok-hermes", "status": "open"}
SHA = "8c04e020552ca7041c3735b723f9b2022d320e10"


class Harness(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.folder = root / "tasks" / "lead-parsing-jev"
        self.worktree = root / "wt"
        self.folder.mkdir(parents=True)
        react.TASKS = root / "tasks"
        react.BRIEFS = root / "briefs"
        react.STATE = root / ".react-state.json"
        react.LOG = root / "react.log"
        react.TOKEN = "test-token"
        react.AGENT = "hermes"
        react.MAX_INFLIGHT = 2
        react.DRY = False
        self.calls, self.cmds = [], []
        self.status = {"active": True, "head": SHA, "branch": "airtribe/x"}
        self.verify_rc = 0
        react.api = self.fake_api
        react.sh = self.fake_sh
        self.addCleanup(self.tmp.cleanup)

    def write_task_md(self, worktree=True):
        (self.folder / "TASK.md").write_text(
            f"# Task: lead-parsing-jev\n\n"
            f"**ACP task:** `{TASK['id']}`\n"
            f"**Branch:** `airtribe/lead-parsing-jev`\n"
            f"**Worktree:** `{self.worktree}`\n")
        if worktree:
            self.worktree.mkdir(exist_ok=True)

    # ---- seams
    def fake_api(self, method, path, body=None):
        self.calls.append((method, path, body))
        if path == "/api/user/agents":
            return [{"id": "tok-hermes", "label": "hermes"}, {"id": "tok-other", "label": "nobody"}]
        if path == "/api/user/work/claimable":
            return [TASK]
        if path == "/api/user/work/claim":
            return TASK
        return {}

    def fake_sh(self, argv, timeout=900):
        argv = [str(a) for a in argv]
        self.cmds.append(argv)
        if "verify.sh" in argv[1]:
            return self.verify_rc, "full suite: 23 failed tests / 52 failed suites"
        sub = argv[2]
        if sub == "prepare":
            self.worktree.mkdir(exist_ok=True)
            return 0, json.dumps({"prepared": True, "worktree": str(self.worktree)})
        if sub == "launch":
            return 0, json.dumps({"launched": True, "tmux_session": "airtribe-lead-parsing-jev"})
        if sub == "status":
            return 0, json.dumps(self.status)
        raise AssertionError(f"unexpected bridge subcommand {sub}")

    def subs(self):
        return [c[2] for c in self.cmds if len(c) > 2 and not c[1].endswith("verify.sh")]

    def held(self, **over):
        h = {"folder": str(self.folder), "task_id": "lead-parsing-jev", "branch": "airtribe/x",
             "worktree": str(self.worktree), "brief": "/tmp/brief.md", "sha": SHA,
             "tries": 0, "last_try": 0, "proposed": None}
        h.update(over)
        return h


class TestReactor(Harness):
    def test_claimable_with_brief_prepares_and_launches(self):
        self.write_task_md(worktree=False)
        react.main()
        self.assertEqual(self.subs(), ["prepare", "launch"])
        self.assertIn(("POST", "/api/user/work/claim", {"taskId": TASK["id"]}), self.calls)

    def test_claimable_with_existing_worktree_only_launches(self):
        self.write_task_md()
        react.main()
        self.assertEqual(self.subs(), ["launch"])

    def test_claimable_without_brief_only_run_logs(self):
        react.main()                            # no TASK.md anywhere
        self.assertEqual(self.subs(), [])
        logs = [c for c in self.calls if c[1].endswith("/logs")]
        self.assertEqual(logs[0][2], {"lines": ["needs brief: planning required"]})
        react.main()                            # said once, not every tick
        self.assertEqual(len([c for c in self.calls if c[1].endswith("/logs")]), 1)

    def test_dead_tmux_relaunches_with_backoff_then_releases(self):
        self.write_task_md()
        self.status = {"active": False, "head": SHA}
        st = react.load_state()
        st["held"][TASK["id"]] = self.held()
        h = st["held"][TASK["id"]]

        react.tick_held(TASK["id"], h, st)                     # first relaunch, immediately
        self.assertEqual(self.subs(), ["status", "launch"])
        self.assertEqual(h["tries"], 1)

        react.tick_held(TASK["id"], h, st)                     # inside the 5-minute backoff
        self.assertEqual(self.subs(), ["status", "launch", "status"])
        self.assertEqual(h["tries"], 1)

        h["last_try"] -= react.BACKOFF[1]                      # 5 minutes later
        react.tick_held(TASK["id"], h, st)
        self.assertEqual(h["tries"], 2)
        h["last_try"] -= react.BACKOFF[2]                      # 15 minutes later
        react.tick_held(TASK["id"], h, st)
        self.assertEqual(h["tries"], 3)
        self.assertEqual(self.subs().count("launch"), 3)

        react.tick_held(TASK["id"], h, st)                     # 4th time: give up
        self.assertEqual(self.subs().count("launch"), 3)
        self.assertIn(("POST", f"/api/user/work/{TASK['id']}/release", None), self.calls)
        self.assertNotIn(TASK["id"], st["held"])
        self.assertIn("stalled", str([c for c in self.calls if c[1].endswith("/logs")]))
        self.assertIn("released the lease", (self.folder / "EVENTS.md").read_text())

    def test_new_commit_runs_verify(self):
        self.write_task_md()
        st = react.load_state()
        st["held"][TASK["id"]] = h = self.held(sha="olddeadbeef")
        react.tick_held(TASK["id"], h, st)
        verifies = [c for c in self.cmds if c[1].endswith("verify.sh")]
        self.assertEqual(verifies, [["bash", str(react.VERIFY), str(self.worktree), SHA]])
        self.assertEqual(h["sha"], SHA)

    def test_verify_clean_proposes_review(self):
        self.write_task_md()
        st = react.load_state()
        st["held"][TASK["id"]] = h = self.held(sha="olddeadbeef")
        react.tick_held(TASK["id"], h, st)
        self.assertIn(("PATCH", f"/api/user/tasks/{TASK['id']}", {"status": "in_review"}), self.calls)
        self.assertEqual(h["proposed"], SHA)
        self.assertIn(f"verified {SHA}", str(self.calls))
        # already proposed: the next tick heartbeats and nothing else
        self.cmds.clear(); self.calls.clear()
        react.tick_held(TASK["id"], h, st)
        self.assertEqual(self.subs(), ["status"])
        self.assertEqual([c[1] for c in self.calls if "heartbeat" not in c[1]], [])

    def test_verify_failed_does_not_propose(self):
        self.write_task_md()
        self.verify_rc = 1
        st = react.load_state()
        st["held"][TASK["id"]] = h = self.held(sha="olddeadbeef")
        react.tick_held(TASK["id"], h, st)
        self.assertNotIn("PATCH", [c[0] for c in self.calls])
        self.assertIsNone(h["proposed"])

    def test_inflight_cap(self):
        self.write_task_md()
        react.MAX_INFLIGHT = 1
        st = react.load_state()
        st["held"]["someone-else"] = self.held()
        react.save_state(st)
        react.main()
        self.assertNotIn("launch", self.subs())

    def test_never_calls_deliver(self):
        self.write_task_md()
        react.main()
        self.assertNotIn("deliver", self.subs())

    def test_missing_token_exits_2(self):
        env = {**os.environ, "ACP_TOKEN": ""}
        p = subprocess.run([sys.executable, str(Path(__file__).parent / "react.py")],
                           capture_output=True, text=True, env=env)
        self.assertEqual(p.returncode, 2)
        self.assertEqual(len(p.stdout.strip().splitlines()), 1)
        self.assertIn("ACP_TOKEN", p.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
