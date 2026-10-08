#!/usr/bin/env python3
"""Download Slack file(s) via Composio and attach them straight to an Airtribe
task in one shot, so the model never shuttles base64 bytes through its own
context window.

Usage: slack_attach.py <taskId> <sourceKey> <slack file id> [<slack file id> ...]

`sourceKey` is the message the files came with (the task's own source_key
when it's the filed message itself, or the reply's key when attaching to an
`intake_append`ed message) - same string the airtribe-intake skill already
uses for `intake_append`/`intake_attach`.

Reads the profile's own Composio MCP access token from mcp-tokens/composio.json
(same pattern as scripts/slack_activity.py) and never refreshes it - if it's
expired, run `hermes -p slack-agent mcp test composio` first. Reads
AIRTRIBE_URL / AIRTRIBE_TOKEN from the environment (Hermes loads the
profile's .env into every tool call already, same as jev_intake.py reads
TYPESAFE_API_KEY).

Never fails the whole batch: skips files over 8 MB or not image/pdf (the
message text is already filed either way) and prints one line per file so
the caller can report counts.
"""
import base64
import json
import os
import ssl
import sys
import urllib.error
import urllib.request

HOME = os.environ.get("HERMES_HOME") or os.path.expanduser("~/.hermes/profiles/slack-agent")
TOKEN = os.path.join(HOME, "mcp-tokens", "composio.json")
COMPOSIO_URL = "https://connect.composio.dev/mcp"
TLS = ssl.create_default_context(cafile="/etc/ssl/cert.pem" if os.path.exists("/etc/ssl/cert.pem") else None)
FILE_LIMIT = 8 * 1024 * 1024  # matches FILE_LIMIT in src/controllers/agent.rs
MIME_OK = {"image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"}


def rpc(token, session, method, params=None, rid=1):
    headers = {"Authorization": f"Bearer {token}", "Content-Type": "application/json",
               "Accept": "application/json, text/event-stream", "MCP-Protocol-Version": "2025-06-18"}
    if session:
        headers["Mcp-Session-Id"] = session
    body = {"jsonrpc": "2.0", "method": method}
    if rid:
        body["id"] = rid
    if params is not None:
        body["params"] = params
    req = urllib.request.Request(COMPOSIO_URL, json.dumps(body).encode(), headers)
    with urllib.request.urlopen(req, context=TLS, timeout=30) as r:
        session = r.headers.get("Mcp-Session-Id") or session
        raw = r.read().decode()
    data = [line[5:].strip() for line in raw.splitlines() if line.startswith("data:")]
    raw = data[-1] if data else raw
    return (json.loads(raw) if raw.strip() else None), session


def download(token, session, file_id):
    """SLACK_DOWNLOAD_SLACK_FILE returns a time-limited signed URL, not bytes
    directly - fetch it ourselves rather than round-tripping through the
    Composio remote workbench."""
    reply, session = rpc(token, session, "tools/call", {"name": "COMPOSIO_MULTI_EXECUTE_TOOL", "arguments": {
        "tools": [{"tool_slug": "SLACK_DOWNLOAD_SLACK_FILE", "arguments": {"file": file_id}}],
        "sync_response_to_workbench": False,
        "thought": "download an attachment to relay to Airtribe",
        "current_step": "ATTACH",
    }}, 2)
    outer = json.loads(reply["result"]["content"][0]["text"])
    result = outer["data"]["results"][0]["response"]
    if not result.get("successful"):
        raise RuntimeError(result.get("error") or "download failed")
    data = result["data"]
    info = data["file_info"]
    url = data["file_content"]["s3url"]
    req = urllib.request.Request(url)
    with urllib.request.urlopen(req, context=TLS, timeout=60) as r:
        content = r.read()
    return info.get("name") or file_id, (info.get("mimetype") or "").lower(), content, session


def attach(base_url, api_token, task_id, source_key, name, mime, content):
    body = json.dumps({
        "name": name,
        "mime": mime,
        "dataBase64": base64.b64encode(content).decode(),
        "sourceKey": source_key,
    }).encode()
    req = urllib.request.Request(
        f"{base_url.rstrip('/')}/api/agent/intake/{task_id}/files", body,
        # Cloudflare in front of the API bans the default "Python-urllib" agent (403, error 1010).
        {"Authorization": f"Bearer {api_token}", "Content-Type": "application/json",
         "User-Agent": "airtribe-slack-agent/1"}, method="POST",
    )
    try:
        with urllib.request.urlopen(req, context=TLS, timeout=30) as r:
            return r.status, r.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


def main():
    if len(sys.argv) < 4:
        sys.exit("usage: slack_attach.py <taskId> <sourceKey> <slack file id> [...]")
    task_id, source_key, file_ids = sys.argv[1], sys.argv[2], sys.argv[3:]

    base_url = os.environ.get("AIRTRIBE_URL")
    api_token = os.environ.get("AIRTRIBE_TOKEN")
    if not base_url or not api_token:
        sys.exit("AIRTRIBE_URL / AIRTRIBE_TOKEN are not set in the environment")

    tok = json.load(open(TOKEN))
    _, session = rpc(tok["access_token"], None, "initialize", {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "slack-attach", "version": "1"},
    })
    rpc(tok["access_token"], session, "notifications/initialized", rid=None)

    for file_id in file_ids:
        try:
            name, mime, content, session = download(tok["access_token"], session, file_id)
        except Exception as e:
            print(f"{file_id}: failed to download ({e})")
            continue
        if mime not in MIME_OK:
            print(f"{file_id}: skipped ({mime or 'unknown type'})")
            continue
        if len(content) > FILE_LIMIT:
            print(f"{file_id}: skipped (>8MB, {len(content)} bytes)")
            continue
        status, body = attach(base_url, api_token, task_id, source_key, name, mime, content)
        if status in (200, 201):
            print(f"{file_id}: attached ({name})")
        elif status == 409:
            print(f"{file_id}: already attached")
        else:
            print(f"{file_id}: failed ({status} {body[:200]})")


if __name__ == "__main__":
    main()
