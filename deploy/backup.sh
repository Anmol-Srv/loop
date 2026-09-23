#!/usr/bin/env bash
# Nightly Postgres dump, kept 14 days. Run from cron on the VM:
#   15 3 * * * /opt/acp/deploy/backup.sh >> /opt/acp/backups/backup.log 2>&1
#
# Set BACKUP_S3=s3://bucket/prefix to also copy each dump off the machine —
# a backup on the same disk as the database is not a backup of the disk.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p ../backups
out="../backups/acp-$(date +%F-%H%M).sql.gz"

docker compose exec -T db pg_dump -U acp --no-owner acp | gzip > "$out"
# An empty dump means pg_dump failed after the pipe opened; do not keep it.
if [ "$(gzip -dc "$out" | head -c 1 | wc -c)" -eq 0 ]; then
  rm -f "$out"
  echo "$(date -Is) backup FAILED: empty dump" >&2
  exit 1
fi

if [ -n "${BACKUP_S3:-}" ]; then
  aws s3 cp "$out" "$BACKUP_S3/$(basename "$out")"
fi
find ../backups -name 'acp-*.sql.gz' -mtime +14 -delete
echo "$(date -Is) backup ok: $out ($(du -h "$out" | cut -f1))"
