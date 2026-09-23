# Staging on one AWS VM

One small EC2 instance runs everything: Postgres, the server, and Caddy in
front for HTTPS. The server image is built on your Mac and shipped over SSH,
so the VM never compiles Rust and never needs access to the repository.

```
teammate's Mac ──HTTPS──▶ Caddy :443 ──▶ acp-server :8080 ──▶ Postgres
                         (Let's Encrypt)   (not published)     (not published)
```

Budget about 30 minutes the first time.

## 1. Launch the VM

In the EC2 console, **Launch instance**:

| Setting | Value |
| --- | --- |
| AMI | Ubuntu Server 24.04 LTS, **64-bit (Arm)** |
| Type | `t4g.small` (2 vCPU, 2 GB) — plenty; nothing compiles here |
| Key pair | yours, for SSH |
| Storage | 20 GB gp3 |
| Security group | inbound **22** from your IP only; **80** and **443** from anywhere |

Port 80 must be open to the internet even though nobody browses it: Let's
Encrypt proves you own the name by calling it there.

Then **Elastic IPs → Allocate → Associate** with the instance, so the address
survives a stop/start. Call it `$IP` below.

## 2. Give it a name

Either an `A` record, e.g. `acp.airtribe.live → $IP`, or — with no DNS change
at all — use `<ip-with-dashes>.sslip.io`, which resolves to that IP and gets a
real certificate. For `13.201.4.7` that is `13-201-4-7.sslip.io`.

## 3. Install Docker on the VM

```bash
ssh ubuntu@$IP
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker ubuntu
exit        # log out and back in so the group applies
```

## 4. Write the server's secrets

On the VM:

```bash
sudo mkdir -p /opt/acp/deploy && sudo chown -R ubuntu:ubuntu /opt/acp
cat > /opt/acp/deploy/.env <<EOF
ACP_DOMAIN=13-201-4-7.sslip.io
POSTGRES_PASSWORD=$(openssl rand -hex 24)
EOF
chmod 600 /opt/acp/deploy/.env
```

This file lives only on the VM. The deploy script never copies a `.env` from
your Mac, so it cannot overwrite it.

## 5. Deploy

From the repository on your Mac:

```bash
scripts/deploy-staging.sh ubuntu@$IP
```

It builds the image (arm64, native on Apple Silicon), streams it to the VM,
syncs `deploy/`, starts the stack, and waits until the server reports healthy.
Migrations run on every start, so healthy means migrated.

Check it from anywhere:

```bash
curl https://13-201-4-7.sslip.io/health/ready
```

The first request can take ~20 s while Caddy fetches the certificate.

## 6. Onboard the team

On the VM, `acp-admin` runs inside the server container. Shorthand:

```bash
cd /opt/acp/deploy
alias acp-admin='docker compose exec server acp-admin'
```

Create the first admin — it prints a setup code for them:

```bash
acp-admin bootstrap-admin anmol.srivastava@airtribe.live "Anmol"
```

Add everyone else from a file of `email,Name` lines:

```bash
cat > /tmp/team.csv <<'EOF'
dhaval@airtribe.live,Dhaval
chinmay.kunkikar@airtribe.live,Chinmay
evana.sajan@airtribe.live,Evana
pratik.vishwakarma@airtribe.live,Pratik
EOF
docker compose cp /tmp/team.csv server:/tmp/team.csv
acp-admin seed-team /tmp/team.csv
```

Departments and roles — a task's track follows its assignee's department, so
set these before anyone is given work:

```bash
acp-admin set-department anmol.srivastava@airtribe.live backend
acp-admin set-department dhaval@airtribe.live backend
acp-admin set-role dhaval@airtribe.live manager
acp-admin set-department chinmay.kunkikar@airtribe.live frontend
acp-admin set-department evana.sajan@airtribe.live design
acp-admin set-department pratik.vishwakarma@airtribe.live design
```

A setup code for each person (valid 48 hours; send each one privately):

```bash
for e in dhaval@airtribe.live chinmay.kunkikar@airtribe.live \
         evana.sajan@airtribe.live pratik.vishwakarma@airtribe.live; do
  acp-admin invite "$e"
done
```

Each person sets their own password (12+ characters) with their code on first
sign-in. A forgotten password is the same step: `acp-admin invite <email>`
issues a fresh code, and redeeming it signs out every existing session.

## 7. Hand out the app

Build the zip on your Mac:

```bash
DIST=1 scripts/bundle-mac.sh          # UNIVERSAL=1 too if anyone is on an Intel Mac
# → target/Airtribe-Control-Plane.zip
```

Send it with these steps for the teammate:

1. Unzip, and drag **Airtribe Control Plane** into **Applications**.
2. The app is not signed with an Apple developer ID yet, so macOS blocks it
   the first time. Run this once in Terminal:
   ```bash
   xattr -dr com.apple.quarantine "/Applications/Airtribe Control Plane.app"
   ```
3. Open it and click **First time here?**
4. Enter your email, the setup code you were sent, and a password of 12 or
   more characters (twice). In **Server** at the bottom, replace
   `http://localhost:8080` with `https://13-201-4-7.sslip.io`. Then
   **Set your password**.

After that it is the plain **Sign in** form, and the server is remembered.

The sidebar's footer shows which server you are signed in to.

## 8. Backups

The database lives in a Docker volume on this VM. Dump it nightly:

```bash
crontab -e
# add:
15 3 * * * /opt/acp/deploy/backup.sh >> /opt/acp/backups/backup.log 2>&1
```

Dumps land in `/opt/acp/backups/`, fourteen days kept. A copy on the same disk
is not a backup of the disk: set `BACKUP_S3=s3://bucket/acp` in the crontab
line (and give the instance an IAM role with `s3:PutObject`) to copy each one
off the machine.

Restore a dump — into an empty database, or the dump's `CREATE TABLE`s
collide with the tables already there:

```bash
cd /opt/acp/deploy
docker compose stop server
docker compose exec db dropdb -U acp acp
docker compose exec db createdb -U acp acp
gunzip -c /opt/acp/backups/acp-2026-09-24-0315.sql.gz \
  | docker compose exec -T db psql -U acp -d acp --single-transaction -v ON_ERROR_STOP=1
docker compose start server
```

This exact sequence was tested: a dump restored into a fresh database came
back with every row count identical.

## 9. Day to day

| To | Run on the VM, in `/opt/acp/deploy` |
| --- | --- |
| Ship a new version | on your Mac: `scripts/deploy-staging.sh ubuntu@$IP` |
| Watch the server | `docker compose logs -f server` |
| Restart it | `docker compose restart server` — in-flight requests finish first |
| Stop everything | `docker compose down` — data stays in the volume |
| Open a SQL shell | `docker compose exec db psql -U acp acp` |

An old copy of the app keeps working against a newer server as long as no
route it uses was removed; when one is, send the new zip.

## When it does not work

- **`curl` hangs or the certificate fails.** Ports 80 and 443 must be open in
  the security group, and the name must resolve to the Elastic IP
  (`dig +short <name>`). `docker compose logs caddy` says which.
- **The deploy script waits and then prints logs.** The server could not
  start — usually `deploy/.env` is missing or the password has characters
  other than letters and digits.
- **The app says it cannot reach the server.** Check the Server field is the
  `https://` address, not `http://`, and that `/health/ready` answers from the
  same Mac.

## What staging is not

This is one VM with the database on its own disk: fine for the team to use
while the product settles, not a production setup. Before this is relied on,
move Postgres to RDS with point-in-time recovery, put a per-IP rate limit on
`/api/auth/*` at the proxy, and sign and notarize the app so step 7.2
disappears. The ship-readiness report lists what is left:
`docs/superpowers/reports/2026-09-23-ship-readiness.md`.
