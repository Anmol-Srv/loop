#!/usr/bin/env python3
"""One Slack intake pass, in code. Replaces the agent-driven pass: the model is
asked only to write titles and bodies (and to settle what Jev is unsure about),
in one tool-free call, and only when there is something to file.

  1. One Composio call searches every watched place, bounded by the cursors.
  2. Drops the owner's own messages, bots, already-skipped keys, duplicates.
  3. One Composio call fetches every thread a new message sits in; names are
     resolved from a cached user list.
  4. Context: what was already filed (intake/recent). A reply in a thread whose
     parent (or any earlier message) was filed is linked to that task.
  5. Jev decides each message in parallel: skip, append to a filed task, file.
  6. Skips and confident appends are applied directly; files and unsure ones
     go to the model in one batch, then are filed and their files attached.
  7. Cursors advance only past handled messages; run_report; one-line counts.

Usage: slack_triage.py [--dry-run]   (dry run: decide and print, write nothing)
"""
import concurrent.futures as cf
import datetime as dt
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import jev_intake  # noqa: E402
import slack_attach  # noqa: E402

HOME = os.environ.get("HERMES_HOME") or os.path.expanduser("~/.hermes/profiles/slack-agent")
STATE = os.path.join(HOME, "slack-intake.json")
USERS = os.path.join(HOME, "state", "slack-users.json")
URL, TOKEN = os.environ.get("AIRTRIBE_URL", "").rstrip("/"), os.environ.get("AIRTRIBE_TOKEN", "")
JEV_KEY = os.environ.get("TYPESAFE_API_KEY", "")
OWNER = "Anmol"
UA = "airtribe-slack-agent/1"  # Cloudflare bans the default Python-urllib agent (403, error 1010)
PAGE, MAX_PAGES = 40, 6
DRY = "--dry-run" in sys.argv


# ── Loop API ──────────────────────────────────────────────────────────────

def api(method, path, body=None):
    req = urllib.request.Request(f"{URL}{path}", json.dumps(body).encode() if body is not None else None, {
        "Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json", "User-Agent": UA}, method=method)
    try:
        with urllib.request.urlopen(req, context=slack_attach.TLS, timeout=30) as r:
            return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:300]


# ── Slack through Composio ────────────────────────────────────────────────

class Slack:
    def __init__(self):
        tok = json.load(open(slack_attach.TOKEN))
        if tok.get("expires_at", 0) - time.time() < 120:  # Hermes owns the refresh; this triggers it
            subprocess.run(["hermes", "-p", "slack-agent", "mcp", "test", "composio"],
                           capture_output=True, timeout=90)
            tok = json.load(open(slack_attach.TOKEN))
        self.token = tok["access_token"]
        _, self.session = slack_attach.rpc(self.token, None, "initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "slack-triage", "version": "1"}})
        slack_attach.rpc(self.token, self.session, "notifications/initialized", rid=None)

    def multi(self, calls):
        """Run [(slug, args)] in one Composio call; returns each call's data, or None where Composio
        elided a response too big to return inline (the caller asks for less). Raises on failure."""
        reply, self.session = slack_attach.rpc(self.token, self.session, "tools/call", {
            "name": "COMPOSIO_MULTI_EXECUTE_TOOL", "arguments": {
                "tools": [{"tool_slug": s, "arguments": a} for s, a in calls],
                "sync_response_to_workbench": False, "thought": "slack intake pass", "current_step": "INTAKE"}}, 2)
        results = json.loads(reply["result"]["content"][0]["text"])["data"]["results"]
        out = []
        for (slug, _), r in zip(calls, results):
            resp = r.get("response") or {}
            if not resp.get("successful"):
                raise RuntimeError(f"{slug}: {resp.get('error') or r.get('error') or 'failed'}")
            out.append(resp.get("data"))
        return out


def users(slack, need):
    """id -> display name; ids not in the cache are looked up together, once."""
    names = json.load(open(USERS)) if os.path.exists(USERS) else {}
    missing = sorted(u for u in need if u and u not in names)
    if missing:
        for u, data in zip(missing, slack.multi([("SLACK_RETRIEVE_DETAILED_USER_INFORMATION", {"user": u})
                                                 for u in missing])):
            m = (data or {}).get("user") or {}
            p = m.get("profile") or {}
            names[u] = p.get("display_name") or m.get("real_name") or p.get("real_name") or m.get("name") or u
        if not DRY:
            os.makedirs(os.path.dirname(USERS), exist_ok=True)
            json.dump(names, open(USERS, "w"))
    return names


# ── Helpers ───────────────────────────────────────────────────────────────

def iso(ts):
    return dt.datetime.fromtimestamp(float(ts), dt.timezone.utc).isoformat().replace("+00:00", "Z")


def day_before(ts):
    return (dt.datetime.fromtimestamp(float(ts), dt.timezone.utc) - dt.timedelta(days=1)).strftime("%Y-%m-%d")


def resolve(text, names):
    return re.sub(r"<@([UW][A-Z0-9]+)>", lambda m: f"<@{m[1]}|{names.get(m[1], m[1])}>", text or "")[:4000]


def thread_ts(match):
    m = re.search(r"thread_ts=([\d.]+)", match.get("permalink", ""))
    return m[1] if m else match["ts"]


def channel_name(ch):
    if ch.get("is_im"):
        return "DM"
    if ch.get("is_mpim"):
        return "Group DM"
    return f'#{ch.get("name", ch["id"])}'


# ── The pass ──────────────────────────────────────────────────────────────

def collect(slack, state):
    """New messages since the cursors, one Composio call per results page, oldest first."""
    owner, cursors = state["ownerId"], state["cursors"]
    intake_ids = {c["id"] for c in state.get("channels", [])}
    if "dms" not in cursors:  # one watermark for every DM, from the old per-DM cursors
        cursors["dms"] = max((v for k, v in cursors.items() if k[0] in "DG"), default=f"{time.time() - 86400:.6f}")
    places = [(c["id"], f'in:{c["name"]}') for c in state.get("channels", [])]
    places += [("mentions", f"<@{owner}>"), ("dms", "to:me")]
    for key, _ in places:
        cursors.setdefault(key, f"{time.time() - 86400:.6f}")

    skipped = {s["key"] for s in state.get("skipped", [])}
    found, pending, size, convo = {}, [(k, q, 1) for k, q in places], PAGE, {}
    while pending:
        datas = slack.multi([("SLACK_SEARCH_MESSAGES", {"query": f"{q} after:{day_before(cursors[k])}",
                                                         "sort": "timestamp", "count": size, "page": p})
                             for k, q, p in pending])
        if any(d is None for d in datas):  # too big to come back inline: smaller pages, from the top
            if size <= 5:
                raise RuntimeError("Slack search results elided even at 5 per page")
            size //= 2
            pending = [(k, q, 1) for k, q, _ in pending]
            continue
        nxt = []
        for (place, q, p), data in zip(pending, datas):
            matches = data["messages"]["matches"]
            for m in matches:
                if thread_ts(m) == m["ts"]:  # top-level: context for later messages in the conversation
                    convo.setdefault(m["channel"]["id"], {})[m["ts"]] = m
                ch = m["channel"]
                if float(m["ts"]) <= float(cursors[place]) or not m.get("user") or m["user"] == owner:
                    continue
                if place == "mentions" and ch["id"] in intake_ids:
                    continue  # the channel pass covers it
                if place == "dms" and not (ch.get("is_im") or ch.get("is_mpim")):
                    continue
                key = f'{m.get("team") or state.get("team", "")}:{ch["id"]}:{m["ts"]}'
                if key in skipped or key in found:
                    continue
                found[key] = {"key": key, "place": place, "m": m,
                              "kind": "channel" if ch["id"] in intake_ids else ("dm" if place == "dms" else "mention")}
            older = matches and float(matches[-1]["ts"]) > float(cursors[place])
            if len(matches) == size and older and p < MAX_PAGES:
                nxt.append((place, q, p + 1))  # the page is all new: there may be more
        pending = nxt
    for i in found.values():
        i["convo"] = [c for c in sorted(convo.get(i["m"]["channel"]["id"], {}).values(), key=lambda c: float(c["ts"]))
                      if 0 < float(i["m"]["ts"]) - float(c["ts"]) < 6 * 3600]
    return sorted(found.values(), key=lambda x: float(x["m"]["ts"]))


def with_threads(slack, items, names):
    """Attach each reply's earlier thread messages (oldest first, ≤30)."""
    roots = sorted({(i["m"]["channel"]["id"], thread_ts(i["m"])) for i in items if thread_ts(i["m"]) != i["m"]["ts"]})
    threads = {}
    if roots:
        for root, data in zip(roots, slack.multi([("SLACK_FETCH_MESSAGE_THREAD_FROM_A_CONVERSATION",
                                                   {"channel": c, "ts": t, "limit": 100}) for c, t in roots])):
            threads[root] = data.get("messages", [])
    need = {i["m"]["user"] for i in items} | {t.get("user") for ms in threads.values() for t in ms if t.get("user")}
    need |= {c.get("user") for i in items for c in i["convo"] if c.get("user")}
    for i in items:
        for u in re.findall(r"<@([UW][A-Z0-9]+)>", i["m"].get("text", "")):
            need.add(u)
    names.update(users(slack, need))
    for i in items:
        m, ch = i["m"], i["m"]["channel"]
        earlier = [t for t in threads.get((ch["id"], thread_ts(m)), []) if float(t["ts"]) < float(m["ts"])][-30:]
        # ponytail: length heuristic for "this refers back" ("ye kr dijiye please"); a self-contained DM
        # judged with unrelated earlier DMs gets matched to the wrong task. Upgrade: ask Jev if it refers back.
        if not earlier and (ch.get("is_im") or ch.get("is_mpim")) and len(m.get("text", "")) < 120:
            earlier = i["convo"][-8:]  # a DM is one conversation: the last few hours of it are its context
        i["thread"] = [{"author": names.get(t.get("user"), t.get("user") or "bot"), "text": resolve(t.get("text"), names),
                        "ts": t["ts"], "receivedAt": iso(t["ts"]), "user": t.get("user"),
                        "files": t.get("files") or []} for t in earlier]
        i["source"] = {"kind": "slack", "key": i["key"], "url": m["permalink"], "channel": ch["id"],
                       "channelName": channel_name(ch), "author": names.get(m["user"], m.get("username") or m["user"]),
                       "text": resolve(m.get("text"), names), "receivedAt": iso(m["ts"]),
                       "private": bool(ch.get("is_im") or ch.get("is_mpim"))}
        if i["thread"]:
            i["source"]["thread"] = [{k: t[k] for k in ("author", "text", "ts", "receivedAt")} for t in i["thread"]]


def link(items, filed):
    """A reply under a message that was filed belongs to that task."""
    by_key = {f["source"]["key"]: f for f in filed if f.get("source", {}).get("key")}
    team_ch = lambda i: i["key"].rsplit(":", 1)[0]  # noqa: E731
    for i in items:
        for t in i["thread"]:
            hit = by_key.get(f'{team_ch(i)}:{t["ts"]}')
            if hit:
                i["linked"] = hit
                break


def judge(item, filed):
    if not JEV_KEY:
        return {"decision": "unsure", "error": "no TYPESAFE_API_KEY"}
    msgs = [{"author": t["author"], "text": t["text"], "reply": n > 0} for n, t in enumerate(item["thread"][-7:])]
    msgs.append({"author": item["source"]["author"], "text": item["source"]["text"], "reply": bool(item["thread"])})
    src = {"kind": item["kind"], "channelName": item["source"]["channelName"], "intakeChannel": item["kind"] == "channel"}
    try:
        return jev_intake.decide({"owner": OWNER, "source": src, "messages": msgs, "filed": filed}, JEV_KEY)
    except Exception as e:  # Jev down: the model decides this one
        return {"decision": "unsure", "error": str(e)[:200]}


def route(item):
    """skip | append | write (to the model), from Jev and the thread link."""
    j, linked = item["jev"], item.get("linked")
    if linked:
        if j["decision"] == "skip":
            return "skip"
        if j["decision"] in ("update", "unsure"):
            # Jev weighed every filed task, the thread's own included: a reply can be about another one.
            item["taskId"] = j.get("taskId") or linked["id"]
            return "append"
        return "write"  # Jev thinks it is a new issue raised in a filed thread: the model settles it
    if j["decision"] == "update" and j.get("taskId"):
        item["taskId"] = j["taskId"]
        return "append"
    return {"skip": "skip", "create": "write"}.get(j["decision"], "write")


PROMPT = """You triage Slack messages into tasks for {owner}. Reply with ONLY a JSON array, one object per item, no prose.

For each item decide `action`:
- "create": someone needs something done or decided by {owner}: a bug, a feature/change request, feedback on something shipped that implies a follow-up, a question {owner} must look into, or a concrete ask. In an intake channel (kind "channel") a new top-level report is presumed task-worthy unless it is plainly chit-chat.
- "append": it is about one of the item's `related` tasks (same underlying problem, a follow-up, "still happening") — give that `taskId`.
- "skip": thanks, +1, greetings, FYI without an ask, a question already answered in the thread, or anything you would file with confidence < 0.6.
`jev` is a classifier's opinion; follow it unless the text clearly says otherwise. `linkedTask` means the thread's parent was already filed as that task: append unless the message raises a clearly different issue.

For "create" also give:
- "title": what needs doing, <= 80 chars, specific ("Checkout fails for saved cards on mobile"), never the first line of the message.
- "body": 2-5 plain sentences: what was reported, by whom, where, the details that matter (numbers, screens, error text), using the thread for context. No markdown. Never copy secrets, passwords, tokens or customer personal data (emails, phones) — describe them instead.
- "category": bug | feature | feedback | question | chore (intake channel default: feedback unless plainly a bug or feature).
- "reason": one line on why it is worth tracking.
- "confidence": 0-1, your honest estimate {owner} wants this tracked.
- "priority": 0-4 (0 highest) ONLY when urgency is explicit (outage, customer blocked, deadline); otherwise omit.
For "skip" give "why" (a few words).

Output shape: [{{"i": 0, "action": "create", "title": "...", "body": "...", "category": "bug", "reason": "...", "confidence": 0.8}}, {{"i": 1, "action": "append", "taskId": "..."}}, {{"i": 2, "action": "skip", "why": "thanks"}}]

Items:
{items}
"""


def write(batch, filed):
    """One tool-free model call for every message that needs words or a judgment."""
    titles = {f["id"]: f'{f["title"]} ({f.get("category") or "?"}, {f["status"]})' for f in filed}
    items = []
    for n, it in enumerate(batch):
        probs = (it["jev"].get("probabilities") or {}).get("target") or {}
        related = [{"taskId": k, "title": titles[k]} for k in sorted(probs, key=probs.get, reverse=True) if k in titles][:5]
        if it.get("linked") and it["linked"]["id"] not in {r["taskId"] for r in related}:
            related.insert(0, {"taskId": it["linked"]["id"], "title": titles.get(it["linked"]["id"], it["linked"]["title"])})
        items.append({"i": n, "kind": it["kind"], "channel": it["source"]["channelName"], "author": it["source"]["author"],
                      "text": it["source"]["text"],
                      "thread": [f'{t["author"]}: {t["text"][:600]}' for t in it["thread"][-10:]],
                      "jev": {k: it["jev"].get(k) for k in ("decision", "category", "confidence")},
                      "linkedTask": it["linked"]["id"] if it.get("linked") else None, "related": related})
    qfile = os.path.join(HOME, "state", "triage-prompt.txt")
    open(qfile, "w").write(PROMPT.format(owner=OWNER, items=json.dumps(items, indent=1, ensure_ascii=False)))
    out = subprocess.run(["hermes", "-p", "slack-agent", "chat", "-Q", "-t", "none", "--query-file", qfile],
                         capture_output=True, text=True, timeout=240).stdout
    m = re.search(r"\[.*\]", out, re.S)
    if not m:
        raise RuntimeError(f"model gave no JSON: {out[:200]!r}")
    return {d["i"]: d for d in json.loads(m[0])}


def attach_files(slack, task_id, item):
    """The message's files, and those of the thread messages sent with it."""
    sets = [(item["key"], item["m"].get("files") or [])]
    team_ch = item["key"].rsplit(":", 1)[0]
    sets += [(f'{team_ch}:{t["ts"]}', t["files"]) for t in item["thread"] if t["files"]]
    n = 0
    for key, files in sets:
        for f in files:
            if (f.get("mimetype") or "").lower() not in slack_attach.MIME_OK:
                continue
            try:
                name, mime, content, slack.session = slack_attach.download(slack.token, slack.session, f["id"])
                if len(content) <= slack_attach.FILE_LIMIT:
                    status, _ = slack_attach.attach(URL, TOKEN, task_id, key, name, mime, content)
                    n += status in (200, 201)
            except Exception as e:
                print(f'attach {f["id"]}: {e}', file=sys.stderr)
    return n


def main():
    started = dt.datetime.now(dt.timezone.utc).isoformat()
    counts = {"filed": 0, "appended": 0, "alreadyFiled": 0, "skipped": 0}
    errors = []
    state = json.load(open(STATE))
    slack = Slack()
    names = {}

    items = collect(slack, state)
    if not items:
        print("filed 0 · appended 0 · already filed 0 · skipped 0")
        return 0
    with_threads(slack, items, names)
    status, recent = api("GET", "/api/agent/intake/recent?days=30")
    filed = recent["data"] if status == 200 else []
    if status != 200:
        errors.append(f"intake/recent {status}")
    link(items, filed)
    with cf.ThreadPoolExecutor(8) as ex:
        for item, j in zip(items, ex.map(lambda i: judge(i, filed), items)):
            item["jev"] = j

    done, skips = set(), []
    for it in items:
        it["route"] = route(it)
    batch = [it for it in items if it["route"] == "write"]
    words = {}
    if batch:
        try:
            words = write(batch, filed)
        except Exception as e:
            errors.append(f"model: {e}")
    for n, it in enumerate(batch):
        w = words.get(n)
        if not w:
            continue
        it["route"] = {"create": "create", "append": "append", "skip": "skip"}.get(w.get("action"), "skip")
        it["words"] = w
        if it["route"] == "append":
            it["taskId"] = w.get("taskId") or (it.get("linked") or {}).get("id")
            if not it["taskId"]:
                it["route"] = "create" if w.get("title") else "skip"

    for it in items:
        r = it["route"]
        if DRY:
            w = it.get("words", {})
            print(f'{r:7} {it["source"]["channelName"]:22} {it["source"]["author"][:14]:14} '
                  f'{(w.get("title") or it.get("taskId") or w.get("why") or it["jev"].get("decision"))!s:60.60} '
                  f'| {it["source"]["text"][:60]!r}')
            continue
        if r == "write":
            continue  # the model failed: retried next pass
        if r == "skip":
            why = (it.get("words") or {}).get("why") or f'jev {it["jev"].get("decision")}'
            skips.append({"key": it["key"], "why": why})
            counts["skipped"] += 1
            done.add(it["key"])
            continue
        if r == "append":
            status, body = api("POST", f'/api/agent/intake/{it["taskId"]}/append', {"source": it["source"], "text": ""})
            ok_key = "appended"
            task_id = it["taskId"]
        else:
            w = it["words"]
            body_ = {"source": it["source"], "title": w["title"][:80], "body": w.get("body", ""),
                     "category": w.get("category") or it["jev"].get("category") or "feedback",
                     "reason": w.get("reason") or "", "confidence": float(w.get("confidence", 0.7))}
            if isinstance(w.get("priority"), int):
                body_["priority"] = w["priority"]
            status, body = api("POST", "/api/agent/intake", body_)
            ok_key = "filed"
            task_id = body["data"]["id"] if status in (200, 201) else None
        if status in (200, 201):
            counts[ok_key] += 1
            attach_files(slack, task_id, it)
            done.add(it["key"])
        elif status == 409:
            counts["alreadyFiled"] += 1
            done.add(it["key"])
        else:
            errors.append(f'{r} {it["key"]}: {status} {body}')

    if DRY:
        return 0
    # Advance each place's cursor through its handled messages, stopping at the first unhandled one.
    for place in {it["place"] for it in items}:
        for it in (x for x in items if x["place"] == place):
            if it["key"] not in done:
                break
            state["cursors"][place] = it["m"]["ts"]
    state["skipped"] = (state.get("skipped", []) + skips)[-200:]
    tmp = STATE + ".tmp"
    json.dump(state, open(tmp, "w"), indent=2)
    os.replace(tmp, STATE)

    line = (f'filed {counts["filed"]} · appended {counts["appended"]} · '
            f'already filed {counts["alreadyFiled"]} · skipped {counts["skipped"]}')
    run_status = "partial" if errors else "ok"
    api("POST", "/api/agent/runs", {"startedAt": started, "status": run_status, "summary": line, "counts": counts,
                                    **({"error": "; ".join(errors)[:1000]} if errors else {})})
    print(line)
    if errors:
        print("partial: " + "; ".join(errors)[:500])
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        if not DRY:
            api("POST", "/api/agent/runs", {"status": "failed", "error": f"{type(e).__name__}: {e}"[:1000]})
        print(f"failed: {type(e).__name__}: {e}")
        sys.exit(1)
