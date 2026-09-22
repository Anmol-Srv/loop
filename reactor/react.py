#!/usr/bin/env python3
"""react.py — the deterministic reactor for acp delegation. One tick per minute, no LLM.

    react.py            act on the claimable/held queue
    react.py --dry-run  print the commands it would run, change nothing

No LLM decides anything here. Every acp task assigned to this agent maps to exactly one
command per tick:

    claimable + task folder has a brief  -> work_claim, then bridge prepare / bridge launch
    claimable + no task folder           -> run-log "needs brief: planning required", nothing else
                                            (planning is Hermes's job, not this script's)
    held                                 -> work_heartbeat, bridge status, then one of:
                                            new commit -> verify.sh + EVENTS.md row
                                            verify clean -> run-log + task_update -> in_review
                                            tmux dead, no new commit -> bridge launch (1/5/15m
                                            backoff, 3 tries) then work_release + "stalled"

`bridge deliver` is never called: pushing a branch and opening a PR is a human decision.
Stdout carries only what a person should act on; idle ticks are silent, so a
`hermes --no-agent` cron stays quiet. The full trail is reactor/react.log.
"""
import json, os, re, subprocess, sys, time, urllib.error, urllib.request
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
STATE = HERE / ".react-state.json"
LOG = HERE / "react.log"
BASELINE = HERE / "baseline.json"
VERIFY = HERE / "verify.sh"
BRIDGE = ROOT / "scripts" / "airtribe_bridge.py"

URL = os.environ.get("ACP_URL", "http://localhost:8080").rstrip("/")
TOKEN = os.environ.get("ACP_TOKEN", "")
AGENT = os.environ.get("AIRTRIBE_AGENT", "hermes")
MAX_INFLIGHT = int(os.environ.get("AIRTRIBE_MAX_INFLIGHT", "2"))
REPO = Path(os.environ.get("AIRTRIBE_REPO", "/Users/anmol/Drive/Airtribe/mycohort-api"))
WORKTREES = Path(os.environ.get("AIRTRIBE_WORKTREES", "/Users/anmol/Drive/Airtribe/.airtribe-worktrees"))
TASKS = WORKTREES / "tasks"
BRIEFS = WORKTREES / "briefs"

BACKOFF = [60, 300, 900]          # 1 / 5 / 15 minutes, then the task is stalled
DRY = "--dry-run" in sys.argv


def now():
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def log(line, notify=False):
    with LOG.open("a") as fh:
        fh.write(f"{now()} {line}\n")
    if notify or DRY:
        print(line)


# ---------------------------------------------------------------- the two seams
# Both of these are monkeypatched wholesale by test_react.py. Everything else in
# this file is pure decision-making over their return values.

def api(method, path, body=None):
    """One acp call. Returns the `data` side of the {success, data|error} envelope.

    Raises RuntimeError on any non-2xx — a tick that cannot read the queue does nothing.
    """
    req = urllib.request.Request(
        URL + path,
        method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={"authorization": f"Bearer {TOKEN}", "content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.loads(r.read() or b"{}").get("data")
    except urllib.error.HTTPError as ex:
        detail = (ex.read() or b"").decode()[:300]
        raise RuntimeError(f"{method} {path} -> {ex.code} {detail}") from ex
    except (urllib.error.URLError, OSError) as ex:
        raise RuntimeError(f"{method} {path} -> {ex}") from ex


def sh(argv, timeout=900):
    """Run a command. Returns (returncode, combined output)."""
    p = subprocess.run([str(a) for a in argv], capture_output=True, text=True, timeout=timeout)
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def bridge(*args):
    rc, out = sh([sys.executable, str(BRIDGE), *args])
    try:
        return rc, json.loads(out)
    except json.JSONDecodeError:
        return rc, {"error": out.strip()[-400:]}


# ---------------------------------------------------------------- state
def load_state():
    try:
        st = json.loads(STATE.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        st = {}
    st.setdefault("held", {})       # task id -> {folder, worktree, branch, sha, tries, last_try, proposed}
    st.setdefault("no_brief", [])   # task ids already told "needs brief", so we say it once
    return st


def save_state(st):
    if not DRY:
        STATE.write_text(json.dumps(st, indent=2, sort_keys=True) + "\n")


# ---------------------------------------------------------------- task folders
FIELD = re.compile(r"^\*\*(?P<k>[A-Za-z ]+):\*\*\s*`?(?P<v>[^`\n]+?)`?\s*$", re.M)


def read_folder(folder):
    """The planning contract: tasks/<slug>/TASK.md names the acp task and the branch.

    ponytail: a regex over the `**Key:** value` lines, not a markdown parser. TASK.md is
    written by Hermes to a fixed template; if it ever stops being one, give it a front-matter
    block and parse that instead.
    """
    text = (folder / "TASK.md").read_text(errors="replace")
    meta = {m.group("k").strip().lower(): m.group("v").strip() for m in FIELD.finditer(text)}
    branch = meta.get("branch") or f"airtribe/{folder.name}"
    worktree = meta.get("worktree") or str(WORKTREES / f"{folder.name}-{branch.replace('/', '-')}")
    return {"folder": str(folder), "task_id": folder.name, "branch": branch,
            "worktree": worktree, "brief": str(BRIEFS / f"{folder.name}.md"),
            "acp_task": meta.get("acp task", "")}


def folder_for(task):
    """The task folder that names this acp task id, or None. Matching is on the id, never on
    the title: a folder is created by planning and must declare which acp task it serves."""
    for md in sorted(TASKS.glob("*/TASK.md")):
        if task["id"] in md.read_text(errors="replace"):
            return read_folder(md.parent)
    return None


def events_row(folder, state, event):
    line = f"| {now()[:10]} | {state} | {event} |\n"
    if DRY:
        log(f"would append to {folder}/EVENTS.md: {state} — {event}")
        return
    p = Path(folder) / "EVENTS.md"
    if not p.exists():
        p.write_text(f"# Event history: {Path(folder).name}\n\n| Timestamp (IST) | State | Event |\n|---|---|---|\n")
    with p.open("a") as fh:
        fh.write(line)


def runlog(task_id, *lines):
    if DRY:
        log(f"would run-log {task_id}: {'; '.join(lines)}")
        return
    api("POST", f"/api/user/tasks/{task_id}/logs", {"lines": list(lines)})


# ---------------------------------------------------------------- claimable work
def my_token_ids():
    """Credential ids for this agent label.

    ponytail: /api/user/work/claimable has no assigneeLabel filter and a Task carries only
    assigneeTokenId, so the label is resolved once here against /api/user/agents. If the
    server ever grows `?assigneeLabel=`, delete this.
    """
    return {a["id"] for a in (api("GET", "/api/user/agents") or []) if a["label"] == AGENT}


def start(task, st):
    """A claimable task assigned to us: claim it, then make sure it has a worktree and a
    live builder session."""
    tid = task["id"]
    f = folder_for(task)
    if not f:
        if tid not in st["no_brief"]:
            runlog(tid, "needs brief: planning required")
            st["no_brief"].append(tid)
            log(f"needs brief {tid} {task['title']!r}: no folder under {TASKS} names this task "
                f"— Hermes must plan it and write TASK.md", notify=True)
        return
    if DRY:
        log(f"would claim {tid} {task['title']!r} and prepare/launch {f['task_id']}")
        return

    if api("POST", "/api/user/work/claim", {"taskId": tid}) is None:
        log(f"claim {tid}: another worker took it first")
        return
    held = {**f, "sha": None, "tries": 0, "last_try": 0, "proposed": None}
    st["held"][tid] = held

    if not Path(f["worktree"]).is_dir():
        rc, out = bridge("prepare", "--repo", str(REPO), "--task-id", f["task_id"],
                         "--task", task["title"], "--branch", f["branch"])
        if rc != 0:
            runlog(tid, f"prepare failed: {out.get('error', '')[:200]}")
            log(f"PREPARE FAILED {tid} {f['task_id']}: {out.get('error', '')[:300]}", notify=True)
            return
        held["worktree"] = out.get("worktree", f["worktree"])
        events_row(f["folder"], "active", f"Reactor created isolated worktree on `{f['branch']}`.")
        runlog(tid, f"worktree ready at {held['worktree']}")

    launch(tid, held, "launched builder")


def launch(tid, held, why):
    rc, out = bridge("launch", "--repo", str(REPO), "--task-id", held["task_id"],
                     "--worktree", held["worktree"], "--brief", held["brief"])
    if rc != 0:
        runlog(tid, f"launch failed: {out.get('error', '')[:200]}")
        log(f"LAUNCH FAILED {tid} {held['task_id']}: {out.get('error', '')[:300]}", notify=True)
        return False
    runlog(tid, f"{why}: tmux {out.get('tmux_session')}")
    log(f"{why} {tid} {held['task_id']} in {out.get('tmux_session')}", notify=True)
    return True


# ---------------------------------------------------------------- held work
def tick_held(tid, held, st):
    api("POST", f"/api/user/work/{tid}/heartbeat")
    rc, s = bridge("status", "--repo", str(REPO), "--task-id", held["task_id"],
                   "--worktree", held["worktree"])
    if rc != 0:
        log(f"status {tid} {held['task_id']} failed: {s.get('error', '')[:200]}")
        return
    head, alive = s.get("head"), bool(s.get("active"))

    if head and head != held.get("sha"):
        held["sha"], held["tries"] = head, 0
        verify(tid, held)
        return
    if held.get("proposed") == head:
        return                      # verified and proposed; a human owns it now
    if alive:
        return                      # working; nothing to do

    tries = held.get("tries", 0)
    if tries >= len(BACKOFF):
        api("POST", f"/api/user/work/{tid}/release")
        runlog(tid, f"stalled: builder died {tries} times without a commit; lease released")
        events_row(held["folder"], "blocked", f"Builder died {tries}x without a commit; reactor released the lease.")
        log(f"STALLED {tid} {held['task_id']}: released after {tries} relaunches with no commit "
            f"— attach and look, or re-plan", notify=True)
        st["held"].pop(tid, None)
        return
    waited = time.time() - held.get("last_try", 0)
    if waited < BACKOFF[tries]:
        return
    held["tries"], held["last_try"] = tries + 1, time.time()
    launch(tid, held, f"relaunch {tries + 1}/{len(BACKOFF)} (tmux dead, no new commit)")


def verify(tid, held):
    rc, out = sh(["bash", str(VERIFY), held["worktree"], held["sha"]])
    tail = " ".join(out.split())[-300:]
    verdict = {0: "clean", 1: "failed"}.get(rc, "not attempted")
    runlog(tid, f"verify {held['sha'][:12]}: {verdict}", tail)
    events_row(held["folder"], "verifying" if rc else "verified",
               f"Candidate `{held['sha'][:12]}` verification {verdict}.")
    if rc != 0:
        log(f"verify {verdict} {tid} {held['task_id']} {held['sha'][:12]}: {tail[-200:]}", notify=True)
        return
    runlog(tid, f"verified {held['sha']}")
    api("PATCH", f"/api/user/tasks/{tid}", {"status": "in_review"})
    held["proposed"] = held["sha"]
    log(f"verified {tid} {held['task_id']} {held['sha'][:12]} — proposed in_review "
        f"(pending a human approval)", notify=True)


# ---------------------------------------------------------------- main
def main():
    if not TOKEN:
        print("ACP_TOKEN is not set — mint one with `acp-admin mint hermes --scopes read,claim,propose`")
        return 2
    st = load_state()

    for tid, held in list(st["held"].items()):
        try:
            tick_held(tid, held, st)
        except RuntimeError as ex:
            # A 403 here means the lease is gone: someone else claimed it, or it lapsed while
            # the laptop slept. Drop it and let the next tick treat it as fresh work.
            log(f"held {tid} {held.get('task_id')}: {ex}", notify=True)
            if " -> 403" in str(ex):
                st["held"].pop(tid, None)

    try:
        mine = my_token_ids()
        claimable = api("GET", "/api/user/work/claimable") or []
    except RuntimeError as ex:
        log(f"queue unreachable: {ex}")
        save_state(st)
        return 0
    for task in claimable:
        if len(st["held"]) >= MAX_INFLIGHT:
            log(f"cap reached: {len(st['held'])} in flight (AIRTRIBE_MAX_INFLIGHT={MAX_INFLIGHT}); "
                f"{task['id']} waits")
            break
        if task["id"] in st["held"] or task.get("assigneeTokenId") not in mine:
            continue
        try:
            start(task, st)
        except RuntimeError as ex:
            log(f"start {task['id']}: {ex}", notify=True)

    save_state(st)
    return 0


if __name__ == "__main__":
    sys.exit(main())
