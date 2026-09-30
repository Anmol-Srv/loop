#!/usr/bin/env bash
# Set up a private Loop workspace on this Mac: its own database and its own
# server, listening on this Mac only. Tasks and projects you create there never
# reach the team server. Afterwards it is in Loop's workspace switcher (the
# brand row, top left; ⌘1 / ⌘2).
#
# From anywhere (no repository needed; this is what Settings > Workspaces gives):
#   curl -fsSL <team server>/private-workspace.sh | bash -s -- you@airtribe.live "Your Name"
# From the repository:
#   scripts/private-workspace.sh you@airtribe.live "Your Name"
#
# Safe to run again: it updates and restarts the private server, keeps its data,
# and reconnects the workspace in Loop if it was removed or signed out.
set -euo pipefail

EMAIL=${1:?usage: private-workspace.sh you@airtribe.live "Your Name"}
NAME=${2:-${EMAIL%%@*}}
PORT=${LOOP_PRIVATE_PORT:-8181}
DB=${LOOP_PRIVATE_DB:-loop_private}
# release-mac.sh rewrites this line to the server it uploads to; the server
# programs are downloaded from there when this runs outside the repository.
DOWNLOADS="https://api-1.mycohort.live/loop"

SUPPORT="$HOME/Library/Application Support/airtribe-control-plane"
DIR="$SUPPORT/private"
LABEL="live.airtribe.loop-private"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
SERVER="http://127.0.0.1:$PORT"

[ "$(uname -s)" = "Darwin" ] || { echo "Loop's private workspace runs on a Mac." >&2; exit 1; }

# --- Postgres -----------------------------------------------------------------
# Use one already running (the repository's on 5433, Homebrew's default on
# 5432); otherwise install Homebrew's and start it.
for bin in /opt/homebrew/opt/postgresql@17/bin /usr/local/opt/postgresql@17/bin /opt/homebrew/bin /usr/local/bin; do
  [ -x "$bin/pg_isready" ] && PATH="$bin:$PATH"
done
PGPORT=""
for p in ${LOOP_PRIVATE_PGPORT:-} 5433 5432; do
  if command -v pg_isready >/dev/null && pg_isready -q -h localhost -p "$p"; then PGPORT=$p; break; fi
done
if [ -z "$PGPORT" ]; then
  if ! command -v brew >/dev/null; then
    echo "Postgres is not running and Homebrew is not installed." >&2
    echo "Install Homebrew (https://brew.sh), then run this again." >&2
    exit 1
  fi
  echo "postgres  installing postgresql@17 with Homebrew"
  brew install --quiet postgresql@17
  brew services start postgresql@17 >/dev/null
  PATH="$(brew --prefix postgresql@17)/bin:$PATH"
  for _ in $(seq 1 30); do pg_isready -q -h localhost -p 5432 && break; sleep 1; done
  PGPORT=5432
fi
DATABASE_URL="postgres://localhost:$PGPORT/$DB"

echo "database  $DB (Postgres on $PGPORT)"
if ! psql "postgres://localhost:$PGPORT/postgres" -Atc "select 1 from pg_database where datname = '$DB'" | grep -q 1; then
  createdb -h localhost -p "$PGPORT" "$DB"
fi

# --- the server programs ------------------------------------------------------
# A stable home for them: the server must keep starting at login whatever
# happens to a checkout.
mkdir -p "$DIR/bin" "$DIR/logs"
REPO=""
if [ -f "${BASH_SOURCE[0]:-}" ]; then
  candidate="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  [ -f "$candidate/Cargo.toml" ] && command -v cargo >/dev/null && REPO="$candidate"
fi
if [ -n "$REPO" ]; then
  echo "build     acp-server, acp-admin (from $REPO)"
  (cd "$REPO" && cargo build --quiet --release --bin acp-server --bin acp-admin)
  cp "$REPO/target/release/acp-server" "$REPO/target/release/acp-admin" "$DIR/bin/"
else
  VERSION=$(curl -fsS -m 15 "$DOWNLOADS/download/latest" | tr -d '[:space:]')
  echo "download  Loop server $VERSION"
  TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
  curl -fsSL "$DOWNLOADS/download/loop-server-$VERSION.tar.gz" -o "$TMP/server.tar.gz"
  tar -xzf "$TMP/server.tar.gz" -C "$DIR/bin"
  xattr -dr com.apple.quarantine "$DIR/bin" 2>/dev/null || true
fi

# --- run it at login, on this Mac only ------------------------------------------
echo "service   $LABEL on $SERVER (starts at login)"
mkdir -p "$(dirname "$PLIST")"
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

# --- you, and Loop's switcher ---------------------------------------------------
echo "account   $EMAIL (admin of this workspace)"
# Refused on a re-run, when you already are its admin: that is fine.
DATABASE_URL="$DATABASE_URL" "$DIR/bin/acp-admin" bootstrap-admin "$EMAIL" "$NAME" >/dev/null 2>&1 || true
TOKEN=$(DATABASE_URL="$DATABASE_URL" "$DIR/bin/acp-admin" session "$EMAIL" | tail -1)

echo "register  in Loop's workspace switcher"
python3 - "$SUPPORT" "$SERVER" "$TOKEN" <<'PY'
import json, os, sys
support, server, token = sys.argv[1:4]
os.makedirs(support, exist_ok=True)
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
