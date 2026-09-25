#!/usr/bin/env python3
"""Jev intake parser: does a message (or a short thread) create a task, update
one already filed, or neither — and which task, and what kind?

Reads one JSON object on stdin, prints one JSON decision on stdout:

  in:  {"owner": "Anmol",
        "source": {"kind": "channel" | "mention" | "dm", "channelName": "#issues-and-feedback",
                   "intakeChannel": true},
        "messages": [{"author": "Shibu", "text": "...", "reply": false}, ...],   # last one is decided
        "filed": [{"id": "uuid", "title": "...", "category": "bug", "status": "triage"}, ...]}

  out: {"decision": "create" | "update" | "skip" | "unsure", "taskId": "uuid" | null,
        "category": "bug" | ..., "confidence": {...}, "probabilities": {...}}

"unsure" means the judgments disagree or are weak; the agent then decides by
its skill's rules. Policy lives here in code (thresholds below), the judgments
come from Jev (TypeSafe System One), one request with three parallel questions.

  python3 scripts/jev_intake.py --self-test   # live check against the fixture cases
"""
import json
import os
import ssl
import sys
import urllib.request

API = "https://api.typesafe.ai/v1/systemone"
# ponytail: thresholds picked on the fixture cases below; tune on real misfires.
CREATE_AT, SKIP_AT, UPDATE_AT, TARGET_AT = 0.6, 0.6, 0.5, 0.5
MAX_CANDIDATES = 60
# python.org builds ship without CA roots; fall back to the system bundle rather
# than ever turning verification off.
_CAFILE = "/etc/ssl/cert.pem" if not ssl.get_default_verify_paths().cafile and os.path.exists("/etc/ssl/cert.pem") else None
TLS = ssl.create_default_context(cafile=_CAFILE)

ACTION = {
    "type": "choice",
    "instructions": (
        "The last entry of `messages` is a new Slack message that `owner` received (earlier "
        "entries are its thread, for context). `source` says where it came from: an intake "
        "channel where people report issues and feedback, a mention of `owner`, or a direct "
        "message. `filed` lists tasks already created for `owner` from Slack. Decide what the "
        "last message should do to `owner`'s task list."
    ),
    "criteria": {
        "create": (
            "It raises something that needs doing or deciding and is NOT one of the `filed` tasks: "
            "a bug or error, a feature or change request, product feedback that implies a follow-up, "
            "a question `owner` must look into, or a concrete ask of `owner`. In an intake channel a "
            "new top-level report counts unless it is plainly chit-chat."
        ),
        "update": (
            "It is about a task already in `filed`: a reply adding details, the same problem "
            "reported again (possibly by someone else or in another channel), or a status nudge "
            "such as 'still happening'."
        ),
        "none": (
            "Nothing to track: thanks, praise, greetings, emoji or one-word replies, FYI or "
            "announcements without an ask, social chat, or bot/deploy notices."
        ),
    },
}

CATEGORY = {
    "type": "choice",
    "instructions": "Assume the last message in `messages` is worth tracking. What kind of work is it?",
    "criteria": {
        "bug": "Something is broken, erroring, wrong, slow, or data looks off.",
        "feature": "A request for new behaviour, a change, or a new report or screen.",
        "feedback": "A reaction to something shipped that implies a follow-up (confusing, hard to find, disliked).",
        "question": "A question `owner` has to find out or decide, not a quick yes/no.",
        "chore": "A concrete operational ask of `owner`: rerun something, add someone, export data.",
    },
}


def target_question(filed):
    criteria = {t["id"]: f'{t["title"]} ({t.get("category") or "uncategorised"}, {t.get("status", "")})'
                for t in filed[:MAX_CANDIDATES]}
    criteria["none"] = "It is not about any of these tasks."
    return {
        "type": "choice",
        "instructions": (
            "Suppose the last message in `messages` is about a task `owner` already has. Which task "
            "in the options is it about — the same underlying problem or request, even if worded "
            "differently or reported by someone else? Pick none if no task matches."
        ),
        "criteria": criteria,
    }


def ask(state, questions, key):
    body = json.dumps({"state": state, "model": "jev-latest", "questions": questions}).encode()
    req = urllib.request.Request(API, data=body, method="POST", headers={
        "Authorization": f"Bearer {key}", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=30, context=TLS) as r:
        return json.load(r)["answers"]


def decide(item, key):
    filed = item.get("filed") or []
    state = {"owner": item.get("owner", "the owner"), "source": item.get("source", {}),
             "messages": item["messages"][-8:], "filed": [{"title": t["title"], "category": t.get("category")}
                                                          for t in filed[:MAX_CANDIDATES]]}
    questions = {"action": ACTION, "category": CATEGORY}
    if filed:
        questions["target"] = target_question(filed)
    a = ask(state, questions, key)

    action, cat = a["action"], a["category"]
    p = action["probabilities"]
    target = a.get("target")
    out = {"taskId": None, "category": cat["choice"],
           "confidence": {"action": action["confidence"], "category": cat["confidence"]},
           "probabilities": {"action": p}}
    if target:
        out["confidence"]["target"] = target["confidence"]
        out["probabilities"]["target"] = {k: v for k, v in target["probabilities"].items() if v >= 0.05}

    matched = target and target["choice"] != "none" and target["probabilities"][target["choice"]] >= TARGET_AT
    if p.get("update", 0) >= UPDATE_AT and matched:
        out.update(decision="update", taskId=target["choice"])
    elif p.get("create", 0) >= CREATE_AT and not (matched and p.get("update", 0) >= 0.3):
        out["decision"] = "create"
    elif p.get("none", 0) >= SKIP_AT:
        out["decision"] = "skip"
    else:
        out["decision"] = "unsure"
    return out


def self_test(key):
    filed = [{"id": "t-upi", "title": "Fix UPI payment 500 errors on Safari", "category": "bug", "status": "triage"},
             {"id": "t-csv", "title": "Export filtered leads table results to CSV", "category": "feature", "status": "triage"}]
    intake = {"kind": "channel", "channelName": "#issues-and-feedback", "intakeChannel": True}
    cases = [
        ("bug report", intake, [{"author": "Shibu", "text": "Checkout crashes on Android when applying a coupon code, 4 learners stuck."}], "create", None, "bug"),
        ("thanks", intake, [{"author": "Meera", "text": "Thanks team, the new cohort page looks great!"}], "skip", None, None),
        ("thread follow-up", intake, [{"author": "Shibu", "text": "Payment page throws a 500 with UPI on Safari."},
                                      {"author": "Shibu", "text": "Still happening, 3 more learners in the last hour.", "reply": True}], "update", "t-upi", None),
        ("same issue elsewhere", {"kind": "mention", "channelName": "#random"},
         [{"author": "Kiran", "text": "@Anmol fyi UPI payments are failing on the payment page for Safari users"}], "update", "t-upi", None),
        ("chore ask", {"kind": "mention", "channelName": "#general"},
         [{"author": "Dhaval", "text": "@Anmol can you rerun the payments sync for yesterday? Finance sheet is missing Sept 24."}], "create", None, "chore"),
        ("lunch", {"kind": "dm", "channelName": "DM"}, [{"author": "Rahul", "text": "lunch?"}], "skip", None, None),
        ("feedback", intake, [{"author": "Priya", "text": "The new filter bar on the leads page is confusing - sales can't find the date range."}], "create", None, "feedback"),
    ]
    fails = 0
    for name, source, messages, want, want_task, want_cat in cases:
        got = decide({"owner": "Anmol", "source": source, "messages": messages, "filed": filed}, key)
        ok = got["decision"] == want and (want_task is None or got["taskId"] == want_task) \
            and (want_cat is None or got["category"] == want_cat)
        fails += not ok
        print(f'{"PASS" if ok else "FAIL"}  {name}: {got["decision"]} {got["taskId"] or ""} {got["category"]} '
              f'(action {got["confidence"]["action"]:.2f})')
    return fails


def main():
    key = os.environ.get("TYPESAFE_API_KEY")
    if not key:
        sys.exit("TYPESAFE_API_KEY is not set")
    if "--self-test" in sys.argv:
        sys.exit(1 if self_test(key) else 0)
    print(json.dumps(decide(json.load(sys.stdin), key)))


if __name__ == "__main__":
    main()
