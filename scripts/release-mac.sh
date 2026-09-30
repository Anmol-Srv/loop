#!/usr/bin/env bash
# Publish a new Loop build to the Loop server. Teammates get it by re-running
#
#   curl -fsSL <server>/install.sh | bash
#
# which always fetches the latest upload. The server hands the files out from
# LOOP_DIST_DIR (see src/routes/downloads.rs); this uploads them there over SSH.
#
#   scripts/release-mac.sh               # version = today's date and time
#   LOOP_VERSION=2026.10.01 scripts/release-mac.sh
#
# Releases are cut from what is pushed, so a teammate's build always matches a
# commit on origin: the script refuses a dirty tree or unpushed commits.
set -euo pipefail
cd "$(dirname "$0")/.."

# Where the builds go, and the address teammates install from.
HOST="${LOOP_RELEASE_HOST:-ubuntu@staging-airtribe-api}"
DIST_DIR="${LOOP_RELEASE_DIR:-loop-dist}"
export LOOP_DEFAULT_SERVER="${LOOP_DEFAULT_SERVER:-https://api-1.mycohort.live/loop}"
export LOOP_VERSION="${LOOP_VERSION:-$(date +%Y.%m.%d-%H%M)}"

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "Uncommitted changes; commit and push before releasing." >&2
  exit 1
fi
git fetch -q origin
if [ "$(git rev-parse HEAD)" != "$(git rev-parse '@{u}')" ]; then
  echo "HEAD is not what origin has; push before releasing." >&2
  exit 1
fi

# Universal, so Intel and Apple Silicon Macs install the same download.
UNIVERSAL=1 DIST=1 scripts/bundle-mac.sh

# The installer, pointed at this server, travels with the build.
sed "s#^SERVER=.*#SERVER=\"$LOOP_DEFAULT_SERVER\"#" scripts/install.sh > target/install.sh
printf '%s\n' "$LOOP_VERSION" > target/latest

# Upload under temporary names, then swap in place: a teammate installing
# mid-upload gets the old build whole, never half of the new one.
ssh "$HOST" "mkdir -p $DIST_DIR"
# Each build under its own name, so no cache can serve a stale one; `latest`
# (never cached) says which to fetch, and flips last, once the build is whole.
ZIP="Loop-$LOOP_VERSION.zip"
scp -q target/Loop.zip "$HOST:$DIST_DIR/.$ZIP.part"
scp -q target/install.sh "$HOST:$DIST_DIR/.install.sh.part"
scp -q target/latest "$HOST:$DIST_DIR/.latest.part"
ssh "$HOST" "cd $DIST_DIR && mv .$ZIP.part $ZIP && cp $ZIP Loop.zip && mv .install.sh.part install.sh && mv .latest.part latest \
  && ls -t Loop-*.zip | tail -n +4 | xargs -r rm -f"

echo
echo "released $LOOP_VERSION — teammates install or update with:"
echo "  curl -fsSL $LOOP_DEFAULT_SERVER/install.sh | bash"
