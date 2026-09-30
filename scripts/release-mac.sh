#!/usr/bin/env bash
# Publish a new Loop build as the latest GitHub release. Teammates then get it
# by re-running the install command (scripts/install.sh), which always fetches
# the latest release.
#
#   scripts/release-mac.sh               # version = today's date and time
#   LOOP_VERSION=2026.10.01 scripts/release-mac.sh
#
# Releases are cut from what is pushed, so a teammate's build always matches a
# commit on origin: the script refuses a dirty tree or unpushed commits.
set -euo pipefail
cd "$(dirname "$0")/.."

REPO="Anmol-Srv/loop"
export LOOP_VERSION="${LOOP_VERSION:-$(date +%Y.%m.%d-%H%M)}"
TAG="v$LOOP_VERSION"

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

gh release create "$TAG" target/Loop.zip \
  --repo "$REPO" \
  --target "$(git rev-parse HEAD)" \
  --title "Loop $LOOP_VERSION" \
  --notes "Install or update: \`curl -fsSL https://raw.githubusercontent.com/$REPO/master/scripts/install.sh | bash\`

Built from $(git rev-parse --short HEAD): $(git log -1 --format=%s)" \
  --latest

echo
echo "released $TAG — teammates update with:"
echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/master/scripts/install.sh | bash"
