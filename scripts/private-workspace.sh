#!/usr/bin/env bash
# Set up a private Loop workspace on this Mac: its own database and its own
# server, listening on this Mac only. Tasks and projects you create there never
# reach the team server. Afterwards it is in Loop's workspace switcher (the
# brand row, top left; ⌘1 / ⌘2).
#
#   scripts/private-workspace.sh you@airtribe.live "Your Name"
#
# Safe to run again: it rebuilds and restarts the private server (to pick up a
# newer Loop) and keeps its data and your sign-in.
#
# Needs the local Postgres the repository already develops against (port 5433).
set -euo pipefail
cd "$(dirname "$0")/.."

EMAIL=${1:?usage: scripts/private-workspace.sh you@airtribe.live "Your Name"}
NAME=${2:-${EMAIL%%@*}}
PORT=${LOOP_PRIVATE_PORT:-8181}
PGPORT=${LOOP_PRIVATE_PGPORT:-5433}
DB=${LOOP_PRIVATE_DB:-loop_private}

SUPPORT="$HOME/Library/Application Support/airtribe-control-plane"
DIR="$SUPPORT/private"
LABEL="live.airtribe.loop-private"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
DATABASE_URL="postgres://localhost:$PGPORT/$DB"
SERVER="http://127.0.0.1:$PORT"

echo "database  $DB"
if ! psql "postgres://localhost:$PGPORT/postgres" -Atc "select 1 from pg_database where datname = '$DB'" | grep -q 1; then
  createdb -p "$PGPORT" "$DB"
fi

echo "build     acp-server, acp-admin"
cargo build --quiet --release --bin acp-server --bin acp-admin
# A stable home for the binaries: target/ is emptied by `cargo clean`, and the
# server has to keep starting at login regardless.
mkdir -p "$DIR/bin" "$DIR/logs"
cp target/release/acp-server target/release/acp-admin "$DIR/bin/"

echo "service   $LABEL on $SERVER (starts at login)"
cat > "$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array><string>$DIR/bin/acp-server</string></array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>DATABASE_URL</key><string>$DATABASE_URL</string>
    <!-- This Mac only: nothing on the network can reach it. -->
    <key>HOST</key><string>127.0.0.1</string>
    <key>PORT</key><string>$PORT</string>
    <key>PUBLIC_URL</key><string>$SERVER</string>
  </dict>
  <key>WorkingDirectory</key><string>$DIR</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$DIR/logs/server.log</string>
  <key>StandardErrorPath</key><string>$DIR/logs/server.log</string>
</dict>
</plist>
PLIST
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"

# Migrations run on boot, so ready means migrated.
printf "wait      for the private server"
for _ in $(seq 1 60); do
  if curl -fsS -m 2 "$SERVER/health/ready" >/dev/null 2>&1; then echo " — ready"; break; fi
  printf "."; sleep 1
done
curl -fsS -m 2 "$SERVER/health/ready" >/dev/null || { echo; echo "It did not start; see $DIR/logs/server.log" >&2; exit 1; }

echo "account   $EMAIL (admin of this workspace)"
# Refused on a re-run, when you already are its admin: that is fine.
DATABASE_URL="$DATABASE_URL" "$DIR/bin/acp-admin" bootstrap-admin "$EMAIL" "$NAME" >/dev/null 2>&1 || true
TOKEN=$(DATABASE_URL="$DATABASE_URL" "$DIR/bin/acp-admin" session "$EMAIL" | tail -1)

echo "register  in Loop's workspace switcher"
python3 - "$SUPPORT" "$SERVER" "$TOKEN" <<'PY'
import json, os, sys
support, server, token = sys.argv[1:4]
path = os.path.join(support, "workspaces.json")

def read(name):
    try:
        return open(os.path.join(support, name)).read().strip()
    except OSError:
        return ""

if os.path.exists(path):
    spaces = json.load(open(path))
else:
    # First time: keep the sign-in the app already has as the team workspace,
    # the same way the app seeds its list.
    spaces = []
    current, current_token = read("server"), read("credentials")
    if current and current_token:
        spaces.append({"name": "Airtribe", "server": current, "token": current_token, "private": False})

spaces = [w for w in spaces if not w.get("private")]
spaces.append({"name": "Private", "server": server, "token": token, "private": True})

fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    json.dump(spaces, f, indent=2)
PY

echo
echo "Private workspace ready at $SERVER."
echo "In Loop: click the workspace name at the top left, or press ⌘2, and choose Private."
