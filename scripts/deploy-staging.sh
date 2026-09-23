#!/usr/bin/env bash
# Build the server image here and ship it to the staging VM over SSH.
#
#   scripts/deploy-staging.sh ubuntu@13.201.4.7
#
# The image is built on this Mac — natively arm64 on Apple Silicon, which is
# what a Graviton (t4g) instance runs — then streamed to the VM with
# `docker save | docker load`. The VM never compiles Rust and never needs
# access to the repository. Set PLATFORM=linux/amd64 for an Intel instance.
#
# The first run expects deploy/.env to already exist on the VM (see
# docs/deploy/staging-aws.md); it is never copied from here, so a secret on
# this machine cannot overwrite the one on the server.
set -euo pipefail
cd "$(dirname "$0")/.."

HOST=${1:?usage: scripts/deploy-staging.sh user@host}
PLATFORM=${PLATFORM:-linux/arm64}
REMOTE=/opt/acp

echo "build   acp-server:staging for $PLATFORM"
docker build --platform "$PLATFORM" -t acp-server:staging .

echo "ship    image to $HOST"
docker save acp-server:staging | gzip | ssh "$HOST" 'gunzip | docker load'

echo "sync    deploy/ to $HOST:$REMOTE/deploy"
ssh "$HOST" "mkdir -p $REMOTE/deploy $REMOTE/backups"
tar -C deploy --exclude .env -cf - . | ssh "$HOST" "tar -C $REMOTE/deploy -xf -"

echo "restart"
ssh "$HOST" "cd $REMOTE/deploy && test -f .env || { echo 'no deploy/.env on the server — see docs/deploy/staging-aws.md'; exit 1; }"
ssh "$HOST" "cd $REMOTE/deploy && docker compose up -d --no-build --remove-orphans"

# Migrations run on boot, so healthy means migrated too.
echo -n "wait    for the server to report healthy"
for _ in $(seq 1 60); do
  state=$(ssh "$HOST" "docker inspect -f '{{.State.Health.Status}}' acp-server-1" 2>/dev/null || true)
  if [ "$state" = "healthy" ]; then echo " — healthy"; exit 0; fi
  echo -n "."; sleep 2
done
echo
echo "the server did not become healthy; last logs:"
ssh "$HOST" "cd $REMOTE/deploy && docker compose logs --tail 40 server"
exit 1
